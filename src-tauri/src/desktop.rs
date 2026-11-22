use crate::telemetry::{nvidia_runtime_status, GpuRuntime};
use std::{
    sync::atomic::{AtomicBool, Ordering},
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
const GPU_PENDING: &str = "NVIDIA 独显 · 等待读取";

#[derive(Default)]
pub struct Desktop {
    ready: AtomicBool,
    exiting: AtomicBool,
}

pub fn install_tray(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "tray-show", "打开控制台", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "tray-hide", "隐藏窗口", true, None::<&str>)?;
    let gpu = MenuItem::with_id(app, "tray-gpu-status", GPU_PENDING, false, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let exit = MenuItem::with_id(app, "tray-exit", "退出程序", true, None::<&str>)?;
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
    // 托盘独立读取 PCI 状态，窗口隐藏后无需依赖 WebView 的定时器。
    let app = app.handle().clone();
    thread::spawn(move || {
        let mut previous = GPU_PENDING;
        while !app.state::<Desktop>().exiting.load(Ordering::Acquire) {
            let label = match nvidia_runtime_status() {
                GpuRuntime::Active => "NVIDIA 独显 · 活动",
                GpuRuntime::Suspended => "NVIDIA 独显 · 休眠",
                GpuRuntime::Transition => "NVIDIA 独显 · 切换中",
                GpuRuntime::NotDetected => "NVIDIA 独显 · 未检测到",
                GpuRuntime::Unknown => "NVIDIA 独显 · 状态未知",
            };
            if previous != label && gpu.set_text(label).is_ok() {
                previous = label;
            }
            thread::sleep(Duration::from_secs(5));
        }
    });
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
