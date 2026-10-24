//! 系统托盘：KDE/freedesktop StatusNotifierItem（GNOME 由 AppIndicator 扩展托管）。
//!
//! 用 `ksni` 的纯 DBus 实现（zbus），不依赖 libayatana-appindicator / libdbusmenu，
//! Flatpak 运行时里不需要额外打包任何 C 库。
//!
//! 沙箱（Flatpak）里不能拥有 well-known 名字 `org.kde.StatusNotifierItem-PID-1`，
//! 所以用 `disable_dbus_name(true)` 以唯一名注册；GNOME 的 AppIndicator 扩展支持这种形式。
//! 另外用 `assume_sni_available(true)`：机器上没装扩展/没有宿主时只记日志，不阻碍启动。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{MenuItem, ToolTip, Tray};
use tauri::{AppHandle, Manager};

use crate::http::Whisper;

/// 托盘与页面共享的“宠物是否走动”状态（true = 会自己走动/漫游）。
pub struct Roaming(pub Arc<AtomicBool>);

const ICON_NAME: &str = "com.pc2005.dshpet.desktop";
const DSH_URL: &str = "http://127.0.0.1:3080";

pub struct PetTray {
    app: AppHandle,
    visible: Arc<AtomicBool>,
    whisper: Arc<Mutex<Whisper>>,
    roaming: Arc<AtomicBool>,
}

impl PetTray {
    /// 显示 / 隐藏桌宠：隐藏时停视频（省 CPU），显示时恢复播放并重发一次 input region。
    fn set_visible(&mut self, visible: bool) {
        self.visible.store(visible, Ordering::SeqCst);
        let app = self.app.clone();
        let _ = app.clone().run_on_main_thread(move || {
            let Some(win) = app.get_webview_window("pet") else {
                return;
            };
            if visible {
                let _ = win.show();
                // 隐藏期间视频被暂停了，恢复播放；input region 由 400ms 刷新线程补回。
                let _ = win.eval(
                    "document.querySelectorAll('video').forEach(function (v) { \
                       var p = v.play(); if (p && p.catch) p.catch(function () {}); });",
                );
                eprintln!("[dsh-pet] 托盘：显示宠物");
            } else {
                let _ = win.eval(
                    "document.querySelectorAll('video').forEach(function (v) { v.pause(); });",
                );
                let _ = win.hide();
                eprintln!("[dsh-pet] 托盘：隐藏宠物");
            }
        });
    }

    fn toggle_whisper(&mut self) {
        if let Ok(mut w) = self.whisper.lock() {
            let on = !w.is_auto();
            w.set_auto(on);
            eprintln!("[dsh-pet] 托盘：碎碎念 {}", if on { "开" } else { "关" });
        }
    }

    /// 切换“宠物是否走动”：页面里的 __dshPetRoaming 直接改每只宠物的 fixedEnabled，
    /// 立刻生效（正在走的那只先停下），不需要重启或重载配置。
    fn toggle_roaming(&mut self) {
        let on = !self.roaming.load(Ordering::SeqCst);
        self.roaming.store(on, Ordering::SeqCst);
        let app = self.app.clone();
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(win) = app2.get_webview_window("pet") {
                let js = format!(
                    "window.__dshPetRoaming && window.__dshPetRoaming.set({});",
                    if on { "true" } else { "false" }
                );
                let _ = win.eval(&js);
            }
        });
        eprintln!("[dsh-pet] 托盘：宠物走动 {}", if on { "开" } else { "关" });
    }

    fn reload(&self) {
        if let Some(win) = self.app.get_webview_window("pet") {
            // 页面重新拉取 /config（HTTP 宿主每次请求都会重新读 main-config.jsonc）。
            let _ = win.eval("location.reload();");
            eprintln!("[dsh-pet] 托盘：重载配置");
        }
    }

    fn open_site(&self) {
        if let Err(e) = std::process::Command::new("xdg-open").arg(DSH_URL).spawn() {
            eprintln!("[dsh-pet] 托盘：xdg-open 失败 {e}");
        }
    }
}

impl Tray for PetTray {
    fn id(&self) -> String {
        "dsh-pet".into()
    }

    fn title(&self) -> String {
        "dsh-pet 桌宠".into()
    }

    fn icon_name(&self) -> String {
        ICON_NAME.into()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "dsh-pet 桌宠".into(),
            description: "左键显示/隐藏宠物，右键打开菜单".into(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let vis = !self.visible.load(Ordering::SeqCst);
        self.set_visible(vis);
    }

    /// 每次打开菜单前 ksni 会重新调用 `menu()`（只要实现了这个钩子），
    /// 所以「隐藏/显示」「碎碎念」的文案与勾选状态始终是最新的，不需要在回调里 update()。
    fn menu_about_to_show(&mut self) {}

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let vis = self.visible.load(Ordering::SeqCst);
        let whisper_auto = self.whisper.lock().map(|w| w.is_auto()).unwrap_or(false);
        let roaming = self.roaming.load(Ordering::SeqCst);
        vec![
            StandardItem {
                label: if vis { "隐藏宠物".into() } else { "显示宠物".into() },
                activate: Box::new(|t: &mut Self| {
                    let v = !t.visible.load(Ordering::SeqCst);
                    t.set_visible(v);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "打开 DSH 网站".into(),
                activate: Box::new(|t: &mut Self| t.open_site()),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "宠物走动".into(),
                checked: roaming,
                activate: Box::new(|t: &mut Self| t.toggle_roaming()),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "碎碎念".into(),
                checked: whisper_auto,
                activate: Box::new(|t: &mut Self| t.toggle_whisper()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "重载宠物配置".into(),
                activate: Box::new(|t: &mut Self| t.reload()),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "退出".into(),
                activate: Box::new(|t: &mut Self| t.app.exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// 启动托盘。失败（无 DBus / 无宿主）时只打日志，不影响桌宠本体。
pub fn spawn(
    app: AppHandle,
    visible: Arc<AtomicBool>,
    whisper: Arc<Mutex<Whisper>>,
    roaming: Arc<AtomicBool>,
) {
    let tray = PetTray {
        app,
        visible,
        whisper,
        roaming,
    };
    match tray
        .disable_dbus_name(true)
        .assume_sni_available(true)
        .spawn()
    {
        Ok(_handle) => {
            eprintln!("[dsh-pet] 托盘已注册（StatusNotifierItem）");
        }
        Err(e) => {
            eprintln!("[dsh-pet] 托盘不可用：{e}（GNOME 需安装 AppIndicator 扩展）");
        }
    }
}
