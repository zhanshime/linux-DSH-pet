/**
 * Tauri 宿主的 petBridge 兼容层 —— 逐项对齐 Electron preload 暴露的原语。
 *
 * 只负责把渲染端调用转成 Tauri 命令（Rust 侧 src-tauri/src/main.rs）：
 *   setBounds        → pet_set_bounds（移动/缩放透明窗口）
 *   setInteractive   → pet_set_interactive（点击穿透；Linux 走 GTK input shape）
 *   setInputBusy     → pet_set_input_busy（拖拽/菜单/对话占用输入）
 *   setInputRegions  → pet_set_input_regions（控件矩形，指针轮询用）
 *   openDshSite      → pet_open_url（系统默认浏览器）
 *   reportFlight / reportCollide / onFlightStates / onPetHit → 单品宠物暂不启用（空实现）
 *   onDisplays       → 单屏几何由 Rust 启动时一次性下发（空实现，保留接口）
 *
 * 注意：不做任何兜底或模拟数据 —— 与插件"显式失败"的风格一致。
 */
(function () {
  'use strict';

  // 页面在 tauri 自定义协议下，视频走 http://127.0.0.1 属跨源；不给 <video> 加
  // crossOrigin=anonymous 的话 canvas 会被污染，getImageData 直接抛 SecurityError，
  // 轮廓采样就拿不到 alpha。HTTP 侧已回 Access-Control-Allow-Origin: *。
  (function patchVideoCors() {
    var create = document.createElement.bind(document);
    document.createElement = function (tag) {
      var el = create(tag);
      if (String(tag).toLowerCase() === 'video') {
        try {
          el.crossOrigin = 'anonymous';
        } catch (e) {}
      }
      return el;
    };
  })();

  var TAURI = window.__TAURI__ || {};
  var invoke = TAURI.core && TAURI.core.invoke ? TAURI.core.invoke : null;

  function call(cmd, args) {
    if (!invoke) {
      console.warn('[dsh-pet-tauri] __TAURI__.core.invoke 不可用，命令 ' + cmd + ' 被跳过');
      return;
    }
    try {
      var p = invoke(cmd, args || {});
      if (p && typeof p.catch === 'function') {
        p.catch(function (e) {
          console.error('[dsh-pet-tauri] 命令 ' + cmd + ' 失败：', e);
        });
      }
    } catch (e) {
      console.error('[dsh-pet-tauri] 命令 ' + cmd + ' 抛错：', e);
    }
  }

  var __petShift = { x: null, y: null };
  // 轮廓坐标是"采样那一刻"的绝对坐标。宠物被拖动/走动后若等下一次采样（最多 500ms）才更新，
  // 这期间点在新位置会全部穿透；所以记下采样原点，一移动就把整份轮廓平移过去。
  var __silRects = [];
  var __silAnchor = { x: 0, y: 0 };

  window.petBridge = {
    setBounds: function (x, y, width, height, boxX, boxY, size, bottomPad, vx, vy) {
      // 覆盖层模式：窗口固定铺满工作区不再移动，改由这里把整个宠物层平移「窗口本该在的位置」，
      // 与 Electron 里移动窗口视觉等价（sprite 在窗口内钉在 margin 上，平移后正好落在 pos）。
      if (__petShift.x !== boxX || __petShift.y !== boxY) {
        var dx = __petShift.x === null ? 0 : boxX - __petShift.x;
        var dy = __petShift.y === null ? 0 : boxY - __petShift.y;
        __petShift.x = boxX;
        __petShift.y = boxY;
        if ((dx || dy) && __silRects.length) {
          __silRects = __silRects.map(function (r) {
            return [r[0] + dx, r[1] + dy, r[2], r[3]];
          });
          call('pet_set_silhouette', { rects: __silRects });
        }
        var el = document.querySelector('.pet-sprite');
        if (el) {
          // 覆盖层模式：窗口不动，把 sprite 摆到它"本该在的屏幕位置"（= boxX/boxY）。
          el.style.left = boxX + 'px';
          el.style.top = boxY + 'px';
        }
      }
      call('pet_set_bounds', {
        x: x,
        y: y,
        width: width,
        height: height,
        boxX: boxX,
        boxY: boxY,
        size: size,
        bottomPad: bottomPad,
        vx: vx,
        vy: vy,
      });
    },
    setInteractive: function (interactive) {
      call('pet_set_interactive', { interactive: !!interactive });
    },
    setInputBusy: function (busy) {
      call('pet_set_input_busy', { busy: !!busy });
    },
    setInputRegions: function (input) {
      call('pet_set_input_regions', { input: input || null });
    },
    openDshSite: function (url) {
      call('pet_open_url', { url: String(url || '') });
    },
    // 多宠物/跨窗碰撞在 Tauri 单宠物版本里不启用（接口保留，避免 sprite.js 判空分支差异）
    reportRoaming: function (on) {
      call('pet_roaming_report', { on: !!on });
    },
    reportFlight: function () {},
    reportCollide: function () {},
    onFlightStates: function () {},
    onPetHit: function () {},
    onDisplays: function (cb) {
      if (typeof cb !== 'function') return;
      var T = window.__TAURI__;
      if (!T || !T.event || !T.event.listen) return;
      // 主进程检测到分辨率/缩放变化后推 pet_displays，渲染端 applyDeskGeometry + relayout
      T.event.listen('pet_displays', function (ev) {
        try {
          cb(ev && ev.payload);
        } catch (e) {
          console.error('[dsh-pet-tauri] onDisplays 回调出错：', e);
        }
      });
    },
  };

  // 把 .pet-hit 放大到整个 sprite 框：可点像素已由"视频帧 alpha 轮廓"精确限定，
  // 元素本身再只用身体中间那 37%，点头发/道具就收不到 pointerdown，拖不动宠物。
  (function expandHitArea() {
    function apply() {
      var st = document.createElement('style');
      st.textContent =
        '.pet-hit{left:0!important;top:0!important;width:100%!important;height:100%!important;}';
      (document.head || document.documentElement).appendChild(st);
    }
    if (document.head) apply();
    else document.addEventListener('DOMContentLoaded', apply, { once: true });
  })();

  // 命中盒上报：输入区域默认是整个视频框（462×281），但人物实际只占中间一块。
  // 用渲染端现成的 .pet-hit（HIT_BOX，约 173×223）当"可点区域"，透明边角就能穿透。
  // 100ms 轮询 + 只在变化时发；拖拽/菜单期间主进程会改用整框，避免指针滑出区域。
  (function reportHitRect() {
    var last = null;
    function tick() {
      var el = document.querySelector('.pet-hit');
      if (!el) return;
      var r = el.getBoundingClientRect();
      if (!(r.width >= 8 && r.height >= 8)) return;
      var pad = 6; // 给指针留一点余量，避免边界抖动
      var x = Math.round(r.left - pad);
      var y = Math.round(r.top - pad);
      var w = Math.round(r.width + pad * 2);
      var h = Math.round(r.height + pad * 2);
      if (last && last[0] === x && last[1] === y && last[2] === w && last[3] === h) return;
      last = [x, y, w, h];
      call('pet_set_hit_rect', { x: x, y: y, width: w, height: h });
    }
    setInterval(tick, 100);
  })();

  // 轮廓采样：把当前动画帧缩到 24×14 网格读 alpha，按行合并成矩形，当作"精确可点区域"。
  // 目的：整框太挡、.pet-hit 又太窄（只有身体中间 37%），导致点头发/道具拖不动宠物。
  // 轮廓区域能跟着 sprite 走，透明角落依旧穿透。
  (function reportSilhouette() {
    var GW = 24;
    var GH = 14;
    var ALPHA = 24;
    var canvas = null;
    var ctx = null;
    var mask = null;
    var lastSample = 0;
    var lastSig = '';
    var sampleWarned = false;
    function frontVideo() {
      return (
        document.querySelector('.pet-sprite video.is-front') ||
        document.querySelector('.pet-sprite video')
      );
    }
    function mirrored(v) {
      try {
        var t = getComputedStyle(v).transform || '';
        var m = t.match(/^matrix\(([^)]+)\)$/);
        if (m) return parseFloat(m[1].split(',')[0]) < 0;
      } catch (e) {}
      return false;
    }
    function sampleMask(v, key) {
      if (!canvas) {
        canvas = document.createElement('canvas');
        canvas.width = GW;
        canvas.height = GH;
        ctx = canvas.getContext('2d', { willReadFrequently: true });
      }
      if (!ctx) return null;
      try {
        ctx.clearRect(0, 0, GW, GH);
        ctx.drawImage(v, 0, 0, GW, GH);
        var d = ctx.getImageData(0, 0, GW, GH).data;
        var cells = new Uint8Array(GW * GH);
        var solid = 0;
        for (var i = 0; i < cells.length; i++) {
          cells[i] = d[i * 4 + 3] > ALPHA ? 1 : 0;
          solid += cells[i];
        }
        // 视频切帧的瞬间可能抽到全透明首帧：这种"空轮廓"不能当真，
        // 否则 input region 会被清空，整只宠物突然全穿透。
        if (solid < GW * GH * 0.02) return null;
        if (mirrored(v)) {
          for (var j = 0; j < GH; j++) {
            for (var k = 0; k < GW >> 1; k++) {
              var a = j * GW + k;
              var b = j * GW + (GW - 1 - k);
              var t = cells[a];
              cells[a] = cells[b];
              cells[b] = t;
            }
          }
        }
        return { key: key, cells: cells };
      } catch (e) {
        if (!sampleWarned) {
          sampleWarned = true;
          console.warn(
            '[dsh-pet-tauri] 轮廓采样失败：' + e +
            ' | crossOrigin=' + v.crossOrigin +
            ' | readyState=' + v.readyState +
            ' | src=' + String(v.currentSrc || v.src || '').slice(0, 140)
          );
        }
        return null;
      }
    }
    function rectsFor(r, cells) {
      var cw = r.width / GW;
      var ch = r.height / GH;
      var out = [];
      var prevX0 = -1;
      var prevX1 = -1;
      var bandStart = 0;
      function flush(endRow) {
        if (prevX0 >= 0 && prevX1 > prevX0) {
          out.push([
            Math.round(r.left + prevX0 * cw - 4),
            Math.round(r.top + bandStart * ch - 4),
            Math.round((prevX1 - prevX0) * cw + 8),
            Math.round((endRow - bandStart) * ch + 8),
          ]);
        }
      }
      for (var j = 0; j <= GH; j++) {
        var s0 = -1;
        var s1 = -1;
        if (j < GH) {
          for (var i = 0; i < GW; i++) {
            if (cells[j * GW + i]) {
              if (s0 < 0) s0 = i;
              s1 = i + 1;
            }
          }
        }
        if (s0 !== prevX0 || s1 !== prevX1) {
          flush(j);
          prevX0 = s0;
          prevX1 = s1;
          bandStart = j;
        }
      }
      return out;
    }
    function tick() {
      var v = frontVideo();
      if (!v || v.readyState < 2 || !v.videoWidth) return;
      var r = v.getBoundingClientRect();
      if (!(r.width >= 8 && r.height >= 8)) return;
      var key = v.currentSrc || v.src || '';
      var now = Date.now();
      if (!mask || mask.key !== key || now - lastSample > 200) {
        var next = sampleMask(v, key);
        lastSample = now;
        if (next) mask = next;
        else return;
      }
      var rects = rectsFor(r, mask.cells);
      var sig = rects.length
        ? rects
            .map(function (q) {
              return q.join(',');
            })
            .join(';')
        : 'empty';
      if (sig === lastSig) return;
      lastSig = sig;
      __silRects = rects;
      __silAnchor.x = r.left;
      __silAnchor.y = r.top;
      call('pet_set_silhouette', { rects: rects });
    }
    setInterval(tick, 80);
  })();

  call('pet_frontend_ready');
})();
