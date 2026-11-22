use crate::telemetry::{nvidia_runtime_status, GpuRuntime};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    App, AppHandle, Emitter, Manager, State, Window, WindowEvent,
};

const MAIN_WINDOW: &str = "main";
const TRAY_ID: &str = "revo-panel";
const EXIT_EVENT: &str = "desktop-exit";

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq)]
pub enum Language {
    #[serde(rename = "zh-CN")]
    Chinese,
    #[serde(rename = "en")]
    English,
}

impl Language {
    fn menu(self) -> [&'static str; 3] {
        match self {
            Self::Chinese => ["打开控制台", "隐藏窗口", "退出程序"],
            Self::English => ["Open RevoPanel", "Hide window", "Quit RevoPanel"],
        }
    }

    fn gpu(self, state: Option<GpuRuntime>) -> &'static str {
        match (self, state) {
            (Self::Chinese, None) => "NVIDIA 独显 · 等待读取",
            (Self::Chinese, Some(GpuRuntime::Active)) => "NVIDIA 独显 · 活动",
            (Self::Chinese, Some(GpuRuntime::Suspended)) => "NVIDIA 独显 · 休眠",
            (Self::Chinese, Some(GpuRuntime::Transition)) => "NVIDIA 独显 · 切换中",
            (Self::Chinese, Some(GpuRuntime::NotDetected)) => "NVIDIA 独显 · 未检测到",
            (Self::Chinese, Some(GpuRuntime::Unknown)) => "NVIDIA 独显 · 状态未知",
            (Self::English, None) => "NVIDIA GPU · Waiting for data",
            (Self::English, Some(GpuRuntime::Active)) => "NVIDIA GPU · Active",
            (Self::English, Some(GpuRuntime::Suspended)) => "NVIDIA GPU · Suspended",
            (Self::English, Some(GpuRuntime::Transition)) => "NVIDIA GPU · Transitioning",
            (Self::English, Some(GpuRuntime::NotDetected)) => "NVIDIA GPU · Not detected",
            (Self::English, Some(GpuRuntime::Unknown)) => "NVIDIA GPU · Unknown",
        }
    }
}

#[tauri::command]
pub fn system_language() -> Language {
    for name in ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
        if let Ok(value) = std::env::var(name) {
            for locale in value.split(':') {
                match locale.split(['_', '-', '.', '@']).next() {
                    Some("zh") => return Language::Chinese,
                    Some("en") => return Language::English,
                    _ => {}
                }
            }
        }
    }
    Language::English
}

struct TrayMenu {
    show: MenuItem<tauri::Wry>,
    hide: MenuItem<tauri::Wry>,
    exit: MenuItem<tauri::Wry>,
    gpu: MenuItem<tauri::Wry>,
    state: Option<GpuRuntime>,
    language: Language,
}

#[derive(Default)]
pub struct Desktop {
    ready: AtomicBool,
    exiting: AtomicBool,
    tray: Mutex<Option<TrayMenu>>,
}

pub fn install_tray(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let language = system_language();
    let [show_label, hide_label, exit_label] = language.menu();
    let show = MenuItem::with_id(app, "tray-show", show_label, true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "tray-hide", hide_label, true, None::<&str>)?;
    let gpu = MenuItem::with_id(
        app,
        "tray-gpu-status",
        language.gpu(None),
        false,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let exit = MenuItem::with_id(app, "tray-exit", exit_label, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &hide, &gpu, &separator, &exit])?;
    let icon = app.default_window_icon().cloned().ok_or("缺少托盘图标")?;
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .title("RevoPanel")
        .tooltip("RevoPanel")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray-show" => show_window(app),
            "tray-hide" => {
                if tray_is_available(app) {
                    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                        let _ = window.hide();
                    }
                }
            }
            "tray-exit" => request_exit(app),
            _ => {}
        });
    #[cfg(target_os = "linux")]
    let builder = {
        // AppIndicator 通过磁盘文件把图标交给 Plasma，选择应用可写的缓存目录。
        let directory = app.path().app_cache_dir()?.join("tray");
        std::fs::create_dir_all(&directory)?;
        builder.temp_dir_path(directory)
    };
    builder.build(app)?;
    *app.state::<Desktop>()
        .tray
        .lock()
        .map_err(|_| "Tray state unavailable")? = Some(TrayMenu {
        show,
        hide,
        exit,
        gpu,
        state: None,
        language,
    });
    // 托盘独立读取 PCI 状态，窗口隐藏后无需依赖 WebView 的定时器。
    let app = app.handle().clone();
    thread::spawn(move || {
        let desktop = app.state::<Desktop>();
        while !desktop.exiting.load(Ordering::Acquire) {
            let current = Some(nvidia_runtime_status());
            if let Ok(mut tray) = desktop.tray.lock() {
                if let Some(tray) = tray.as_mut() {
                    if tray.state != current
                        && tray.gpu.set_text(tray.language.gpu(current)).is_ok()
                    {
                        tray.state = current;
                    }
                }
            }
            thread::sleep(Duration::from_secs(5));
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn set_language(desktop: State<'_, Desktop>, language: Language) -> Result<(), String> {
    let mut tray = desktop.tray.lock().map_err(|_| "Tray state unavailable")?;
    if let Some(tray) = tray.as_mut() {
        if tray.language != language {
            let [show, hide, exit] = language.menu();
            // 一起更新菜单和 GPU 状态，避免等待下一次 PCI 采样才切换语言。
            tray.show
                .set_text(show)
                .map_err(|error| error.to_string())?;
            tray.hide
                .set_text(hide)
                .map_err(|error| error.to_string())?;
            tray.exit
                .set_text(exit)
                .map_err(|error| error.to_string())?;
            tray.gpu
                .set_text(language.gpu(tray.state))
                .map_err(|error| error.to_string())?;
            tray.language = language;
        }
    }
    Ok(())
}

pub fn show_window(app: &AppHandle) {
    if app.state::<Desktop>().exiting.load(Ordering::Acquire) {
        return;
    }
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        // GTK 在隐藏和图标化时使用不同的状态，只恢复真正最小化的窗口。
        if window.is_minimized().unwrap_or(false) {
            let _ = window.unminimize();
        }
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(target_os = "linux")]
fn tray_host_registered() -> bool {
    use dbus::blocking::{stdintf::org_freedesktop_dbus::Properties, Connection};

    let Ok(connection) = Connection::new_session() else {
        return false;
    };
    // 托盘对象可以在没有宿主的桌面上创建，关闭前仍要确认能从托盘恢复窗口。
    connection
        .with_proxy(
            "org.kde.StatusNotifierWatcher",
            "/StatusNotifierWatcher",
            Duration::from_millis(300),
        )
        .get(
            "org.kde.StatusNotifierWatcher",
            "IsStatusNotifierHostRegistered",
        )
        .unwrap_or(false)
}

#[cfg(not(target_os = "linux"))]
fn tray_host_registered() -> bool {
    true
}

fn tray_is_available(app: &AppHandle) -> bool {
    app.tray_by_id(TRAY_ID).is_some() && tray_host_registered()
}

pub fn handle_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != MAIN_WINDOW {
        return;
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let app = window.app_handle();
        if app.state::<Desktop>().exiting.load(Ordering::Acquire) {
            return;
        }
        if !tray_is_available(app) || window.hide().is_err() {
            request_exit(app);
        }
    }
}

fn request_exit(app: &AppHandle) {
    let desktop = app.state::<Desktop>();
    if desktop.exiting.swap(true, Ordering::AcqRel) {
        return;
    }
    if !desktop.ready.load(Ordering::Acquire) || app.emit_to(MAIN_WINDOW, EXIT_EVENT, ()).is_err() {
        app.exit(0);
        return;
    }

    // 优先让界面发送最后的设置，渲染进程失去响应时仍能退出。
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(30));
        app.exit(0);
    });
}

#[tauri::command]
pub fn desktop_ready(desktop: State<'_, Desktop>) {
    desktop.ready.store(true, Ordering::Release);
}

#[tauri::command]
pub fn finish_exit(app: AppHandle, desktop: State<'_, Desktop>) {
    if desktop.exiting.load(Ordering::Acquire) {
        app.exit(0);
    }
}
