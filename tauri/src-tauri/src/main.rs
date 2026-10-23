//! dsh-pet 桌宠的 Tauri 外壳 —— 让 Electron 桌面渲染端脱离 DSH 独立跑在 Linux 桌面。
//!
//! 结构：
//!   web/                 复用插件 runtime/electron-helper 的渲染端（sprite/事件/菜单全保留）
//!   http.rs              极简本机 HTTP 宿主（配置 / 状态 / 动画 / 表情包 / 字体）
//!   config.rs            JSONC（内置默认 + 用户 main-config.jsonc）合并
//!   wl.rs                原生 Wayland 点击穿透（wl_surface.set_input_region）
//!   pointer.rs           X11 指针轮询（仅 XWayland 兜底模式）
//!
//! 窗口模型：**全屏透明覆盖层**
//!   Wayland 的 xdg-shell 不允许客户端定位自己的窗口，而 GNOME 会把"需要部分出屏"的托管窗口
//!   夹回屏内（实测 XMoveWindow 1203,489 → 996,336），所以"窗口 = 宠物 + 外扩边距、跟着宠物跑"
//!   的 Electron 模型在 Linux 上不可移植。这里改成：窗口固定铺满主屏（0,0 起），宠物在层内移动，
//!   透明区通过 input region 穿透——这也正是浏览器端本来就在用的坐标模型。
//!
//!   - 原生 Wayland：wl_surface.set_input_region（C shim，GTK3 无此 API）
//!   - XWayland 兜底：XShape + XQueryPointer 轮询（DSH_PET_XWAYLAND=1 强制）
//!
//! 运行：
//!   dsh-pet-desktop              # 桌宠（自动选择 Wayland / XWayland）
//!   dsh-pet-desktop --selftest   # 只验证 VP9-alpha 解码/合成，打印结果后退出

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod http;
mod pointer;
mod wl;

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// 详细日志开关（默认只打 warn/error，避免刷屏）
fn verbose() -> bool {
    std::env::var("DSH_PET_VERBOSE").map(|v| v == "1").unwrap_or(false)
}

#[derive(Default)]
struct Selftest {
    result: Mutex<Option<Value>>,
}

/// 会话里有 Wayland 就用原生 Wayland；显式 DSH_PET_XWAYLAND=1 或只有 DISPLAY 时走 XWayland
fn use_wayland_session() -> bool {
    if std::env::var("DSH_PET_XWAYLAND").map(|v| v == "1").unwrap_or(false) {
        return false;
    }
    if let Ok(backend) = std::env::var("GDK_BACKEND") {
        return backend.split(',').any(|b| b.trim() == "wayland");
    }
    std::env::var("WAYLAND_DISPLAY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

/// GTK3/Wayland 的透明窗口复用一个 shm 缓冲，且不做背景清理；WebKit 用 source-over 往上叠，
/// 透明像素盖不掉上一帧的内容 → 宠物一动就拖出残影。这里在窗口 draw 的最前面（子控件绘制之前）
/// 用 CLEAR/Source 把整块区域写成全透明，残影随即消失。
fn clear_to_transparent(w: &gtk::Widget, cr: &gtk::cairo::Context) {
    use gtk::cairo::Operator;
    use gtk::prelude::*;
    let alloc = w.allocation();
    let _ = cr.save();
    let _ = cr.set_operator(Operator::Source);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
    cr.rectangle(0.0, 0.0, alloc.width() as f64, alloc.height() as f64);
    let _ = cr.fill();
    let _ = cr.restore();
}

fn hook_clear_recursive(widget: &gtk::Widget) {
    use gtk::prelude::*;
    let is_web = widget.type_().name().contains("WebKit");
    if widget.has_window() || is_web {
        widget.connect_draw(|w, cr| {
            clear_to_transparent(w, cr);
            gtk::glib::Propagation::Proceed
        });
    }
    if let Ok(container) = widget.clone().downcast::<gtk::Container>() {
        for child in container.children() {
            hook_clear_recursive(&child);
        }
    }
}

/// GTK3/Wayland 的透明窗口复用一个 shm 缓冲，且不做背景清理；WebKit 用 source-over 往上叠，
/// 透明像素盖不掉上一帧的内容 → 宠物一动就拖出残影。这里在窗口**以及 WebKit 子控件**的 draw
/// 最前面把当前区域写成全透明（子控件常常有自己的 GdkWindow，父级清屏碰不到它）。
fn install_transparent_clear(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        let Ok(gw) = window.gtk_window() else {
            eprintln!("[dsh-pet] 无法拿到 GtkWindow，跳过清屏钩子");
            return;
        };
        let widget: gtk::Widget = gw.clone().upcast();
        hook_clear_recursive(&widget);
        eprintln!("[dsh-pet] 已安装透明清屏钩子（窗口 + 子控件）");
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = window;
    }
}

/// Wayland 软件合成下的重影修正。
///
/// WebKitGTK 逐帧只为"变化的小块"上报 damage，而 GTK 在 Wayland 上会把整个缓冲区 attach 给
/// 合成器（协议只声明哪块变了，不会擦除其它像素）。宠物一走，上一帧的像素就留在共享缓冲里，
/// 屏幕上出现拖尾/重影。这里以 30fps 在 GTK 主线程整窗 queue_draw，让每一帧都把整块
/// surface 重绘（透明区域重新写回全透明），拖尾即被清掉。
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn spawn_repaint_pump(window: tauri::WebviewWindow, state: Arc<pointer::InputState>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(33));
        let _ = &state;
        let w = window.clone();
        let w2 = w.clone();
        let sent = w.run_on_main_thread(move || {
            if let Ok(gw) = w2.gtk_window() {
                use gtk::prelude::*;
                gw.queue_draw();
            }
        });
        if sent.is_err() {
            break;
        }
    });
}

/// 主屏工作区（逻辑像素）与缩放因子。
///
/// 为什么不能只用 GDK：GNOME Wayland 不给客户端下发 strut，`gdk_monitor_get_workarea()`
/// 返回的是整块屏幕（1920×1080），顶栏那 32px 没被扣掉；若按它设窗口，Mutter 会把窗口
/// 放在工作区原点 (0,32)，底部 32px 直接出屏（这正是"换分辨率就对不上"的根源之一）。
/// XWayland 下 Mutter 会维护 `_NET_WORKAREA`（真实工作区），所以优先读它，读不到再退回 GDK。
/// 主屏整块几何（逻辑像素）与缩放因子。纯 Wayland：只信 GDK，不碰 X11。
fn monitor_geometry() -> Option<(f64, f64, f64)> {
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        let display = gtk::gdk::Display::default()?;
        let monitor = display.primary_monitor().or_else(|| display.monitor(0))?;
        let scale = monitor.scale_factor() as f64;
        if scale <= 0.0 {
            return None;
        }
        let g = monitor.geometry();
        Some((g.width() as f64 / scale, g.height() as f64 / scale, scale))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// 顶栏/任务栏占掉的边距（逻辑像素）。纯 Wayland 拿不到 strut，只能按配置来。
/// 默认 32（GNOME 默认顶栏）；顶栏自动隐藏/全屏使用时设 DSH_PET_PANEL_INSET=0。
fn panel_inset() -> f64 {
    std::env::var("DSH_PET_PANEL_INSET")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(32.0)
}

/// 边距在哪条边：top（默认）/ bottom / left / right。决定扣掉的是高度还是宽度。
fn panel_edge() -> String {
    match std::env::var("DSH_PET_PANEL_EDGE") {
        Ok(v) => match v.trim().to_ascii_lowercase().as_str() {
            "bottom" => "bottom".into(),
            "left" => "left".into(),
            "right" => "right".into(),
            _ => "top".into(),
        },
        Err(_) => "top".into(),
    }
}

/// 可视区（= 宠物能活动的区域，逻辑像素）。
///
/// 窗口本身始终铺满整屏：Mutter 会把「放不进工作区」的窗口夹到工作区原点，于是页面
/// (0,0) 恰好落在工作区左上角；再按 inset 把“被面板占住的那条边”从 VIEW 里扣掉，
/// 页面坐标就等于工作区坐标——不需要知道窗口在屏幕上的真实位置。
fn view_size(mon_w: f64, mon_h: f64) -> (f64, f64) {
    let inset = panel_inset().min(mon_w.min(mon_h) - 1.0).max(0.0);
    match panel_edge().as_str() {
        "left" | "right" => ((mon_w - inset).max(1.0), mon_h),
        _ => (mon_w, (mon_h - inset).max(1.0)),
    }
}

/// 推给渲染端的桌面几何（与启动参数同一套口径：逻辑像素，CONFIG.scale = 1）。
fn displays_payload(w: f64, h: f64) -> Value {
    json!({
        "hull": {"x": 0.0, "y": 0.0, "width": w, "height": h},
        "areas": [{"x": 0.0, "y": 0.0, "width": w, "height": h}],
        "workAreas": [{"x": 0.0, "y": 0.0, "width": w, "height": h}],
        "panels": [{"x": 0.0, "y": 0.0, "width": w, "height": h}],
        "primaryIndex": 0,
        "topInsetRatio": 0.0
    })
}

/// 分辨率 / 缩放变化监视（含插拔屏、旋转）。
///
/// 覆盖层窗口尺寸是在启动时按工作区算死的，桌面一变就会出现：窗口比新工作区大/小、
/// 渲染端边界还是旧尺寸（宠物被夹在旧范围里，或底部留一条无法穿透/无法活动的带）。
/// 这里每秒比对一次主屏工作区，变化时**先**把新几何推给渲染端（它重挂 VIEW/AREAS 并对
/// 每只宠物 relayout），**再**把窗口尺寸改成新工作区；渲染端收到窗口 resize 后会按新边界
/// 重新定位（固定角落的宠物回到新角落，拖过的按比例落点重算）。
fn spawn_monitor_watch(window: tauri::WebviewWindow, state: Arc<pointer::InputState>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(1000));
        let w = window.clone();
        let w2 = w.clone();
        let st = state.clone();
        let sent = w.run_on_main_thread(move || {
            let Some((nw, nh, scale)) = monitor_geometry() else {
                if verbose() {
                    eprintln!("[dsh-pet] 几何检查：拿不到主屏信息，跳过本轮");
                }
                return;
            };
            let (cw, ch) = st.win_size.lock().map(|g| *g).unwrap_or((0.0, 0.0));
            if verbose() {
                eprintln!("[dsh-pet] 几何检查: 工作区 {nw:.0}×{nh:.0}，记录 {cw:.0}×{ch:.0}");
            }
            if (nw - cw).abs() < 1.0 && (nh - ch).abs() < 1.0 {
                return;
            }
            let (vw, vh) = view_size(nw, nh);
            eprintln!(
                "[dsh-pet] 桌面几何变化: {cw:.0}×{ch:.0} -> {nw:.0}×{nh:.0}\u{ff08}可视区 {vw:.0}×{vh:.0}，scale {scale}\u{ff09}"
            );
            if let Ok(mut g) = st.win_size.lock() {
                *g = (nw, nh);
            }
            // 先推几何，再改窗口：渲染端先更新边界，随后窗口 resize 触发的 position() 用新 VIEW
            let _ = w2.emit("pet_displays", displays_payload(vw, vh));
            if let Err(e) = w2.set_size(tauri::LogicalSize::new(nw, nh)) {
                eprintln!("[dsh-pet] 桌面几何变化后 set_size 失败: {e}");
            }
        });
        if sent.is_err() {
            break;
        }
    });
}

/// 按当前宠物包围盒 / 输入忙状态刷新"可点区域"
fn current_region_rects(state: &pointer::InputState) -> Vec<[f64; 4]> {
    let busy = state.busy.load(Ordering::Relaxed);
    let box_rect = state.box_rect.lock().ok().and_then(|g| *g);
    let hit_rect = state.hit_rect.lock().ok().and_then(|g| *g);
    let mut rects: Vec<[f64; 4]> = Vec::new();
    // 空闲：优先用命中盒（.pet-hit，比视频框小 ~70%）；拖拽/菜单期间退回整框，
    // 避免指针滑出命中盒导致拖拽断掉。
    let pet = if busy {
        box_rect.or(hit_rect)
    } else {
        hit_rect.or(box_rect)
    };
    if let Some(r) = pet {
        rects.push(r);
    }
    if busy {
        // 菜单/对话弹窗：把渲染端上报的控件矩形也并进来（sprite 已把它们换算成页面坐标），
        // 这样弹窗可点、其余区域仍然穿透。
        if let Ok(extra) = state.rects.lock() {
            rects.extend(extra.iter().copied());
        }
    }
    rects
}

fn apply_input_region(window: &tauri::WebviewWindow, state: &pointer::InputState, force: bool) {
    if !state.wayland.load(Ordering::Relaxed) {
        return;
    }
    let rects = current_region_rects(state);
    if !force {
        let same = state
            .last_region
            .lock()
            .map(|g| *g == rects)
            .unwrap_or(false);
        if same {
            return;
        }
    }
    if wl::set_input_region(window, &rects) {
        if let Ok(mut g) = state.last_region.lock() {
            *g = rects;
        }
    }
}

/// 周期性重发 input region：GTK 在 map / commit 时会用默认 shape（整窗）覆盖掉我们的设置，
/// 只发一次会导致"用着用着就不能穿透"。每 400ms 强制重发一次（Wayland 请求很轻）。
fn spawn_region_refresh(window: tauri::WebviewWindow, state: Arc<pointer::InputState>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(400));
        let w = window.clone();
        let w2 = w.clone();
        let st = state.clone();
        let sent = w.run_on_main_thread(move || {
            apply_input_region(&w2, &st, true);
        });
        if sent.is_err() {
            break;
        }
    });
}

#[tauri::command]
fn pet_set_bounds(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Arc<pointer::InputState>>,
    _x: f64,
    _y: f64,
    _width: f64,
    _height: f64,
    box_x: Option<f64>,
    box_y: Option<f64>,
    size: Option<f64>,
    bottom_pad: Option<f64>,
) -> Result<(), String> {
    // 窗口不再移动/缩放：覆盖层固定铺满主屏，这里只记宠物包围盒（屏幕坐标 = 页面坐标，
    // 因为窗口左上角就是 (0,0)，且 VIEW 原点为 0）。
    let bx = box_x.unwrap_or(0.0);
    let by = box_y.unwrap_or(0.0);
    let s = size.unwrap_or(0.0);
    let bp = bottom_pad.unwrap_or(0.0);
    if s > 0.0 && bx.is_finite() && by.is_finite() && bx.abs() < 20000.0 && by.abs() < 20000.0 {
        if let Ok(mut guard) = state.box_rect.lock() {
            *guard = Some([bx, by, s, s * 9.0 / 16.0 + bp]);
        }
        state.last_bounds_ms.store(now_ms(), Ordering::Relaxed);
        if verbose() {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            if N.fetch_add(1, Ordering::Relaxed) % 40 == 0 {
                eprintln!("[dsh-pet] bounds box=({bx:.0},{by:.0}) size={s:.0}");
            }
        }
    }

    if !state.shown.swap(true, Ordering::SeqCst) {
        let _ = window.show();
        if !state.wayland.load(Ordering::Relaxed) {
            // GTK 窗口 realized 之后才能设 input shape（tao 里 window().unwrap()）；
            // 先整窗穿透，50ms 内指针轮询会按光标位置纠正。
            let _ = window.set_ignore_cursor_events(true);
        }
    }
    apply_input_region(&window, &state, false);
    Ok(())
}

#[tauri::command]
fn pet_set_interactive(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Arc<pointer::InputState>>,
    interactive: bool,
) {
    state.interactive.store(interactive, Ordering::Relaxed);
    if state.wayland.load(Ordering::Relaxed) {
        // Wayland：可点区域由 input region 决定，不需要（也无法）用 set_ignore_cursor_events
        return;
    }
    if !pointer::available() {
        if let Err(e) = window.set_ignore_cursor_events(!interactive) {
            eprintln!("[dsh-pet] set_ignore_cursor_events 失败: {e}");
        }
    }
    // X11 指针轮询可用时由轮询统一裁定（避免两条通道抢着翻）
}

#[tauri::command]
fn pet_set_input_busy(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Arc<pointer::InputState>>,
    busy: bool,
) {
    let prev = state.busy.swap(busy, Ordering::Relaxed);
    if prev != busy {
        if verbose() {
            eprintln!("[dsh-pet] inputBusy -> {busy}");
        }
        apply_input_region(&window, &state, false);
    }
}

/// 渲染端上报命中盒（`.pet-hit` 的屏幕坐标），空闲时据此收窄可点区域。
#[tauri::command]
fn pet_set_hit_rect(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Arc<pointer::InputState>>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) {
    if !(x.is_finite() && y.is_finite() && width.is_finite() && height.is_finite()) {
        return;
    }
    if width < 8.0 || height < 8.0 {
        return;
    }
    let rect = [x, y, width, height];
    let changed = state
        .hit_rect
        .lock()
        .map(|g| *g != Some(rect))
        .unwrap_or(false);
    if !changed {
        return;
    }
    if let Ok(mut g) = state.hit_rect.lock() {
        *g = Some(rect);
    }
    if verbose() {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 5 {
            eprintln!("[dsh-pet] hit rect -> {rect:?}");
        }
    }
    apply_input_region(&window, &state, false);
}

#[tauri::command]
fn pet_set_input_regions(state: tauri::State<'_, Arc<pointer::InputState>>, input: Option<Value>) {
    let mut rects = Vec::new();
    if let Some(Value::Array(list)) = input.as_ref().and_then(|v| v.get("rects")).cloned() {
        for r in list {
            let x = r.get("x").and_then(|v| v.as_f64());
            let y = r.get("y").and_then(|v| v.as_f64());
            let w = r.get("width").and_then(|v| v.as_f64());
            let h = r.get("height").and_then(|v| v.as_f64());
            if let (Some(x), Some(y), Some(w), Some(h)) = (x, y, w, h) {
                if w > 0.0 && h > 0.0 {
                    rects.push([x, y, w, h]);
                }
            }
        }
    }
    if let Ok(mut guard) = state.rects.lock() {
        *guard = rects;
    }
}

#[tauri::command]
fn pet_open_url(url: String) {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        eprintln!("[dsh-pet] 拒绝打开非 http(s) URL: {url}");
        return;
    }
    if let Err(e) = std::process::Command::new("xdg-open").arg(&url).spawn() {
        eprintln!("[dsh-pet] xdg-open 失败: {e}");
    }
}

#[tauri::command]
fn pet_console(level: String, text: String) {
    if verbose() || level == "warn" || level == "error" {
        println!("[dsh-pet-console:{level}] {text}");
    }
}

#[tauri::command]
fn pet_frontend_ready() {
    eprintln!("[dsh-pet] 渲染端起桥完成");
}

#[tauri::command]
fn pet_selftest_stage(stage: String, extra: String) {
    println!("[dsh-pet-selftest] stage={stage} {extra}");
}

#[tauri::command]
fn pet_selftest_result(app: tauri::AppHandle, selftest: tauri::State<'_, Selftest>, payload: Value) {
    println!("[dsh-pet-selftest] {}", serde_json::to_string(&payload).unwrap_or_default());
    if let Ok(mut g) = selftest.result.lock() {
        *g = Some(payload);
    }
    app.exit(0);
}

fn resolve_assets(app: &tauri::App) -> PathBuf {
    if let Ok(v) = std::env::var("DSH_PET_ASSETS") {
        let p = PathBuf::from(v);
        if p.join("config.jsonc").exists() {
            return p;
        }
    }
    // 安装版：二进制旁边就有 assets/（install.sh 这么装）；.desktop 启动时 cwd 是 $HOME，
    // 所以必须按 exe 所在目录找，而不是 cwd。
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("assets");
            if p.join("config.jsonc").exists() {
                return p;
            }
        }
    }
    if let Ok(res) = app.path().resource_dir() {
        let p = res.join("assets");
        if p.join("config.jsonc").exists() {
            return p;
        }
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    if dev.join("config.jsonc").exists() {
        return dev;
    }
    if let Ok(cwd) = std::env::current_dir() {
        let p = cwd.join("assets");
        if p.join("config.jsonc").exists() {
            return p;
        }
    }
    eprintln!("[dsh-pet] 未找到素材目录（可用 DSH_PET_ASSETS 指定）");
    PathBuf::from("assets")
}

fn main() {
    let want_wayland = use_wayland_session();
    if std::env::var("GDK_BACKEND").is_err() {
        std::env::set_var("GDK_BACKEND", if want_wayland { "wayland" } else { "x11" });
    }
    // GTK 3.24.5x 的 Wayland 后端会给窗口表面挂上 linux-drm-syncobj（显式同步），
    // 但当 WebKit 走软件合成、附上的是 wl_shm 缓冲时，Mutter 会直接报协议错误并断开连接：
    //   wl_display.error(wp_linux_drm_syncobj_surface_v1, 2, "Explicit Sync only supported on dmabuf buffers")
    // 关掉 GDK 的 GL 让两条路径都退回 shm，即可稳定运行（代价是软件合成）。
    // WebKitGTK 在 Wayland + NVIDIA 上会因显式同步（linux-drm-syncobj）握手指令缺失而直接断开连接：
    //   wl_display.error(wp_linux_drm_syncobj_surface_v1, …, "explicit sync is used, but no acquire point is set")
    // 官方 workaround 是关掉 NVIDIA 的显式同步，DMABUF 渲染与 GPU 合成照常可用：
    //   https://bugs.webkit.org/show_bug.cgi?id=280210
    if want_wayland && std::env::var_os("__NV_DISABLE_EXPLICIT_SYNC").is_none() {
        std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");
        eprintln!("[dsh-pet] 已设置 __NV_DISABLE_EXPLICIT_SYNC=1（规避 WebKitGTK/NVIDIA 显式同步崩溃）");
    }

    let selftest_mode = std::env::args().any(|a| a == "--selftest");

    tauri::Builder::default()
        .manage(Arc::new(pointer::InputState::default()))
        .manage(Selftest::default())
        .invoke_handler(tauri::generate_handler![
            pet_set_bounds,
            pet_set_interactive,
            pet_set_input_busy,
            pet_set_hit_rect,
            pet_set_input_regions,
            pet_open_url,
            pet_frontend_ready,
            pet_console,
            pet_selftest_stage,
            pet_selftest_result,
        ])
        .setup(move |app| {
            let assets = resolve_assets(app);
            let port = http::spawn(http::Host::new(assets.clone()))
            .map_err(|e| format!("启动本机素材服务失败: {e}"))?;
            eprintln!("[dsh-pet] 素材目录: {}", assets.display());
            eprintln!("[dsh-pet] 本机宿主: http://127.0.0.1:{port}/dsh-pet-7340/");

            // 覆盖层尺寸 = 主屏整块（不是工作区）：Wayland 下客户端无法定位窗口，只有让窗口
            // 恰好等于整屏，合成器才会把它放在 (0,0)，页面坐标才与屏幕坐标一一对应。
            // 窗口尺寸取"工作区"（不含顶栏/任务栏）。实测：整屏大小的窗口会被 Mutter 贴到
            // 工作区原点 (0,32)，底部 32px 被挤出屏幕；而窗口 == 工作区时它正好落在工作区原点，
            // 页面坐标 (0,0) 就等于工作区左上角，输入区域坐标也就不需要任何偏移补偿。
            let (mon_w, mon_h, _scale) = monitor_geometry().unwrap_or((1920.0, 1080.0, 1.0));
            let (mw, mh) = view_size(mon_w, mon_h);
            eprintln!(
                "[dsh-pet] 覆盖层窗口: {mon_w}×{mon_h}（整屏）；可视区 {mw}×{mh}（inset={} {}）",
                panel_inset(),
                panel_edge()
            );

            let boot = json!({
                "configUrl": format!("http://127.0.0.1:{port}/dsh-pet-7340/config"),
                "petIndex": "0",
                "scale": "1",
                "workAreaX": "0",
                "workAreaY": "0",
                "workAreaW": mw.to_string(),
                "workAreaH": mh.to_string(),
                "areas": serde_json::to_string(&json!([{
                    "x": 0.0, "y": 0.0, "width": mw, "height": mh
                }])).unwrap_or_else(|_| "[]".into()),
            });
            let console_hook = r#"
(function () {
  var T = window.__TAURI__;
  var inv = T && T.core && T.core.invoke;
  if (!inv) return;
  function send(level, args) {
    var parts = [];
    for (var i = 0; i < args.length; i++) {
      var a = args[i];
      try { parts.push(typeof a === 'string' ? a : JSON.stringify(a)); }
      catch (e) { parts.push(String(a)); }
    }
    try { inv('pet_console', { level: String(level), text: parts.join(' ') }); } catch (e) {}
  }
  ['log', 'info', 'warn', 'error'].forEach(function (k) {
    var orig = console[k];
    console[k] = function () { send(k, arguments); if (orig) orig.apply(console, arguments); };
  });
  window.addEventListener('error', function (e) {
    send('error', [String(e.message) + ' @' + (e.filename || '') + ':' + (e.lineno || 0)]);
  });
  window.addEventListener('unhandledrejection', function (e) {
    send('error', ['unhandledrejection: ' + String(e.reason)]);
  });
})();
"#;
            let mut init_script = format!("window.__DSH_PET_BOOT__ = {boot};\n{console_hook}");
            if want_wayland && std::env::var("DSH_PET_FORCE_REPAINT").map(|v| v == "1").unwrap_or(false) {
                // 强制 WebKit 每帧把整页重新栅格化：GTK3 Wayland 透明窗口只有一块 shm 缓冲，
                // WebKit 的 tile 缓存只重绘变化小块，宠物一走旧帧就留在缓冲里（残影）。
                // 每帧改一次根元素背景图（透明的两种等价写法）能逼整页重绘，肉眼无差异。
                init_script.push_str(r#"
window.addEventListener('DOMContentLoaded', function () {
  var el = document.documentElement;
  var f = false;
  (function tick() {
    f = !f;
    el.style.backgroundImage = f
      ? 'linear-gradient(rgba(0,0,0,0),rgba(0,0,0,0))'
      : 'linear-gradient(rgba(0,0,0,0.004),rgba(0,0,0,0))';
    requestAnimationFrame(tick);
  })();
});
"#);
            }
            if std::env::var("DSH_PET_DEBUG_MARKS").map(|v| v == "1").unwrap_or(false) {
                init_script.push_str(r#"
function dshMark() {
  if (!document.body) return;
  function mark(x, y, w, h, color, label) {
    var d = document.createElement('div');
    d.style.cssText = 'position:fixed;z-index:2147483647;pointer-events:none;font:14px monospace;color:#fff;' +
      'left:' + x + 'px;top:' + y + 'px;width:' + w + 'px;height:' + h + 'px;background:' + color + ';';
    d.textContent = label;
    document.body.appendChild(d);
  }
  var w = document.documentElement.clientWidth || window.innerWidth;
  var h = document.documentElement.clientHeight || window.innerHeight;
  mark(0, 0, 180, 90, 'rgba(255,0,255,0.85)', 'TL 0,0');
  mark(w - 180, h - 90, 180, 90, 'rgba(0,255,255,0.85)', 'BR ' + w + 'x' + h);
}
window.addEventListener('load', function () { setTimeout(dshMark, 600); });
window.addEventListener('resize', dshMark);
"#);
            }

            if selftest_mode {
                let win = WebviewWindowBuilder::new(app, "selftest", WebviewUrl::App("selftest.html".into()))
                    .title("dsh-pet selftest")
                    .inner_size(360.0, 220.0)
                    .visible(true)
                    .initialization_script(init_script)
                    .build()
                    .map_err(|e| format!("创建自检窗口失败: {e}"))?;
                let _ = win;
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(25));
                    println!("[dsh-pet-selftest] 超时未收到结果");
                    handle.exit(2);
                });
                return Ok(());
            }

            let win = WebviewWindowBuilder::new(app, "pet", WebviewUrl::App("index.html".into()))
                .title("dsh-pet")
                .inner_size(mon_w, mon_h)
                .position(0.0, 0.0)
                .transparent(true)
                .decorations(false)
                .always_on_top(!want_wayland) // Wayland 无私协议，置顶自然无效
                .skip_taskbar(true)
                // 运行期要跟随工作区改尺寸；GTK 下 resizable(false) 会锁死 min=max
                .resizable(true)
                .shadow(false)
                .visible(false)
                .focused(false)
                .initialization_script(init_script)
                .build()
                .map_err(|e| format!("创建桌宠窗口失败: {e}"))?;

            let state = app.state::<Arc<pointer::InputState>>().inner().clone();
            let wayland = wl::display_is_wayland();
            state.wayland.store(wayland, Ordering::SeqCst);
            if let Ok(mut g) = state.win_size.lock() {
                *g = (mon_w, mon_h);
            }
            eprintln!(
                "[dsh-pet] 显示后端: {}（穿透方式: {}）",
                if wayland { "Wayland" } else { "X11/XWayland" },
                if wayland { "wl_surface input region" } else { "XShape + 指针轮询" }
            );
            if std::env::var("DSH_PET_CLEAR_HOOK").map(|v| v == "1").unwrap_or(false) {
                install_transparent_clear(&win);
            }
            if wayland {
                let st = app.state::<Arc<pointer::InputState>>().inner().clone();
                spawn_region_refresh(win.clone(), st);
            }
            {
                let st = app.state::<Arc<pointer::InputState>>().inner().clone();
                spawn_monitor_watch(win.clone(), st);
            }

            // 测试钩子：DSH_PET_TEST_GEOM=1280x720 在 6 秒后模拟一次几何变化（随后监视器会
            // 自动改回真实工作区），用来在不动系统分辨率的前提下验证整条重排链路。
            if let Ok(spec) = std::env::var("DSH_PET_TEST_GEOM") {
                if let Some((fw, fh)) = spec.split_once('x') {
                    if let (Ok(fw), Ok(fh)) = (fw.trim().parse::<f64>(), fh.trim().parse::<f64>()) {
                        let win2 = win.clone();
                        let st2 = app.state::<Arc<pointer::InputState>>().inner().clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_secs(6));
                            let w3 = win2.clone();
                            let w4 = w3.clone();
                            let st3 = st2.clone();
                            let _ = w3.run_on_main_thread(move || {
                                eprintln!("[dsh-pet] 测试：模拟工作区变化 -> {fw:.0}×{fh:.0}");
                                if let Ok(mut g) = st3.win_size.lock() {
                                    *g = (fw, fh);
                                }
                                let (tvw, tvh) = view_size(fw, fh);
                                let _ = w4.emit("pet_displays", displays_payload(tvw, tvh));
                                let _ = w4.set_size(tauri::LogicalSize::new(fw, fh));
                            });
                        });
                    }
                }
            }
            if !wayland {
                pointer::spawn(win.clone(), state);
            } else if std::env::var("DSH_PET_REPAINT_PUMP").map(|v| v == "1").unwrap_or(false) {
                let st = app.state::<Arc<pointer::InputState>>().inner().clone();
                spawn_repaint_pump(win.clone(), st);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("启动 dsh-pet Tauri 外壳失败");
}
