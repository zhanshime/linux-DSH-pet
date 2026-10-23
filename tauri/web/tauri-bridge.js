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

  window.petBridge = {
    setBounds: function (x, y, width, height, boxX, boxY, size, bottomPad, vx, vy) {
      // 覆盖层模式：窗口固定铺满工作区不再移动，改由这里把整个宠物层平移「窗口本该在的位置」，
      // 与 Electron 里移动窗口视觉等价（sprite 在窗口内钉在 margin 上，平移后正好落在 pos）。
      if (__petShift.x !== boxX || __petShift.y !== boxY) {
        __petShift.x = boxX;
        __petShift.y = boxY;
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

  call('pet_frontend_ready');
})();
