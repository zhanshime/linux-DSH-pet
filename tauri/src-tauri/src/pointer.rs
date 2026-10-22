//! 指针轮询 + 点击穿透（Electron 主进程里 `setWindowIgnore` 的等价物）。
//!
//! 为什么必须轮询：窗口一旦整窗穿透，渲染端就收不到 pointermove，光标再次进入宠物身体时
//! 没人能把它翻回来。Electron 主进程用 screen.getCursorScreenPoint() + 兜底定时器解决；
//! Linux 上 Tauri/GTK 没有全局鼠标 API，这里用 X11 的 XQueryPointer（XWayland 下可用）。
//!
//! 判定口径（与 Electron 版一致）：
//!   - 拖拽 / 右键菜单 / 对话弹窗（busy）→ 整窗可交互；
//!   - 光标落在宠物包围盒内 → 可交互；
//!   - 其余 → 穿透（透明外扩区不挡下层应用）。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// 渲染端上报 / 主进程消费的共享状态
#[derive(Default)]
pub struct InputState {
    /// 宠物包围盒（屏幕坐标，逻辑像素）：[x, y, w, h]，由 setBounds 的 boxX/boxY/size/bottomPad 推算
    pub box_rect: Mutex<Option<[f64; 4]>>,
    /// 正在使用窗口输入：拖拽中 / 菜单开着 / 对话弹窗开着
    pub busy: AtomicBool,
    /// 当前窗口是否可交互（仅 X11 轮询模式使用）
    pub interactive: AtomicBool,
    /// 窗口是否已经显示（首帧 setBounds 之后才 show）
    pub shown: AtomicBool,
    /// 渲染端上报的控件矩形（CSS px，窗口内容坐标）——预留给后续菜单命中判定
    pub rects: Mutex<Vec<[f64; 4]>>,
    /// 是否 Wayland 后端（决定用 input region 还是 XShape + 轮询）
    pub wayland: AtomicBool,
    /// 覆盖层窗口尺寸（逻辑像素），busy 时整窗可点用
    pub win_size: Mutex<(f64, f64)>,
    /// 最近一次已应用的 input region（去重，避免逐帧发 Wayland 请求）
    pub last_region: Mutex<Vec<[f64; 4]>>,
    /// 最近一次收到 setBounds 的时间戳（毫秒，epoch）：重绘泵据此判断"宠物在移动"
    pub last_bounds_ms: AtomicU64,
}

impl InputState {
    pub fn box_contains(&self, x: f64, y: f64, pad: f64) -> bool {
        let guard = match self.box_rect.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        match *guard {
            Some([bx, by, bw, bh]) => {
                x >= bx - pad && x <= bx + bw + pad && y >= by - pad && y <= by + bh + pad
            }
            None => false,
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::time::Duration;
    use tauri::WebviewWindow;

    static AVAILABLE: AtomicBool = AtomicBool::new(false);

    /// X11 指针通道是否可用（不可用时 setInteractive 直接翻转，退化为旧行为）
    pub fn available() -> bool {
        AVAILABLE.load(Ordering::SeqCst)
    }

    pub fn spawn(win: WebviewWindow, state: Arc<InputState>) {
        std::thread::spawn(move || {
            let xlib = match x11_dl::xlib::Xlib::open() {
                Ok(x) => x,
                Err(e) => {
                    eprintln!("[dsh-pet-pointer] 无法加载 libX11（{e}）；点击穿透退化为渲染端上报");
                    return;
                }
            };
            unsafe {
                let dpy = (xlib.XOpenDisplay)(std::ptr::null());
                if dpy.is_null() {
                    eprintln!("[dsh-pet-pointer] XOpenDisplay 失败；点击穿透退化为渲染端上报");
                    return;
                }
                AVAILABLE.store(true, Ordering::SeqCst);
                let root = (xlib.XDefaultRootWindow)(dpy);
                eprintln!("[dsh-pet-pointer] X11 指针轮询已启动（50ms）");
                loop {
                    let mut root_ret: std::os::raw::c_ulong = 0;
                    let mut child_ret: std::os::raw::c_ulong = 0;
                    let mut root_x: std::os::raw::c_int = 0;
                    let mut root_y: std::os::raw::c_int = 0;
                    let mut win_x: std::os::raw::c_int = 0;
                    let mut win_y: std::os::raw::c_int = 0;
                    let mut mask: std::os::raw::c_uint = 0;
                    let ok = (xlib.XQueryPointer)(
                        dpy,
                        root,
                        &mut root_ret,
                        &mut child_ret,
                        &mut root_x,
                        &mut root_y,
                        &mut win_x,
                        &mut win_y,
                        &mut mask,
                    );
                    if ok != 0 && state.shown.load(Ordering::Relaxed) {
                        let inside = state.busy.load(Ordering::Relaxed)
                            || state.box_contains(root_x as f64, root_y as f64, 6.0);
                        if inside != state.interactive.load(Ordering::Relaxed) {
                            state.interactive.store(inside, Ordering::Relaxed);
                            let verbose =
                                std::env::var("DSH_PET_VERBOSE").map(|v| v == "1").unwrap_or(false);
                            if verbose {
                                eprintln!(
                                    "[dsh-pet] 穿透 -> {} (cursor {},{} busy={})",
                                    if inside { "可交互" } else { "穿透" },
                                    root_x,
                                    root_y,
                                    state.busy.load(Ordering::Relaxed)
                                );
                            }
                            if let Err(e) = win.set_ignore_cursor_events(!inside) {
                                if verbose {
                                    eprintln!("[dsh-pet-pointer] set_ignore_cursor_events 失败: {e}");
                                }
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        });
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::*;
    use tauri::WebviewWindow;

    pub fn available() -> bool {
        false
    }

    pub fn spawn(_win: WebviewWindow, _state: Arc<InputState>) {}
}

pub use imp::{available, spawn};
