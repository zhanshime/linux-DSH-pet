//! 原生 Wayland 下"可点区域"的收口：把 [x,y,w,h] 列表交给 C shim，落到 wl_surface input region。
//!
//! 为什么不用 `gdk_window_input_shape_combine_region`：GTK3 的 Wayland 后端并不实现它，
//! 调了之后合成器看到的仍然是 GTK 自己的默认 shape（整窗 -10,-10,1940,1068），等于没有穿透。
//! 这里直接对 GTK 已经创建好的 wl_surface 下发 region（同一连接，不新开），
//! 并由主进程周期性重发，防止 GTK 在 map/commit 时把它覆盖回默认值。
//!
//! 所有坐标都是 surface 本地逻辑像素；本壳的窗口铺满工作区且位于工作区原点，
//! 因此页面 CSS px 与屏幕坐标一一对应（scale=1 时），无需再换算。

use std::ffi::c_void;

use tauri::WebviewWindow;

#[cfg(target_os = "linux")]
extern "C" {
    fn dsh_pet_set_input_region(win: *mut c_void, rects: *const i32, count: i32) -> i32;
}

/// 当前 GDK 显示后端是否 Wayland。
/// 不能看窗口对象：窗口在 show 之前还没 realize，`gtk_window().window()` 是 None。
pub fn display_is_wayland() -> bool {
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        gtk::gdk::Display::default()
            .map(|d| d.type_().name().contains("Wayland"))
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// 应用可点区域；空列表 = 整窗穿透。返回是否真正生效（Wayland 后端且拿到 wl_surface）。
pub fn set_input_region(window: &WebviewWindow, rects: &[[f64; 4]]) -> bool {
    #[cfg(target_os = "linux")]
    {
        use gtk::glib::translate::ToGlibPtr;
        use gtk::prelude::*;

        let Ok(win) = window.gtk_window() else {
            return false;
        };
        if win.window().is_none() {
            // 首帧 show 之前先 realize，保证 GdkWindow / wl_surface 已经存在
            win.realize();
        }
        let Some(gdk_win) = win.window() else {
            return false;
        };
        let raw: *mut gtk::gdk::ffi::GdkWindow = gdk_win.to_glib_none().0;
        let ptr = raw as *mut c_void;

        let mut ints: Vec<i32> = Vec::with_capacity(rects.len() * 4);
        for r in rects {
            let w = r[2].max(1.0).round() as i32;
            let h = r[3].max(1.0).round() as i32;
            if w <= 0 || h <= 0 {
                continue;
            }
            ints.push(r[0].round() as i32);
            ints.push(r[1].round() as i32);
            ints.push(w);
            ints.push(h);
        }
        let count = (ints.len() / 4) as i32;
        let applied = unsafe { dsh_pet_set_input_region(ptr, ints.as_ptr(), count) } != 0;
        if applied {
            win.queue_draw();
        }
        applied
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (window, rects);
        false
    }
}
