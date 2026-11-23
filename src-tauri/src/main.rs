mod catalog;
mod controls;
mod cooler;
mod desktop;
mod lighting;
mod linux;
mod system;
mod telemetry;
mod turbo;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, ErrorKind, Write},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, ExitCode, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

const PKEXEC: &str = "/usr/bin/pkexec";
const RESTART_REQUIRED: &str = "RevoPanel 的程序文件已被替换。请从托盘退出后重新打开。";

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Service {
    Lighting,
    Cooler,
    Turbo,
}

#[derive(Clone, Serialize)]
struct WorkerEvent {
    service: Service,
    kind: &'static str,
    data: Value,
}

struct Running {
    stdin: Option<ChildStdin>,
    pending_auth: Option<Arc<Mutex<Child>>>,
}

impl Running {
    fn stop(&mut self) {
        // EOF 会结束会话。睿频助手恢复其改过的值后才退出。
        self.stdin.take();
        // 尚未完成的系统认证不会读取 stdin，需要一并取消。
        if let Some(child) = self.pending_auth.take() {
            if let Ok(mut child) = child.try_lock() {
                let _ = child.kill();
            }
        }
    }
}

type Worker = Arc<Mutex<Option<Running>>>;
type ControlWorker = Arc<Mutex<Option<AuthorizedControls>>>;

struct AuthorizedControls {
    process: Running,
    ready: bool,
    services: Vec<Service>,
}

impl AuthorizedControls {
    fn start(&mut self, service: Service) -> Result<(), String> {
        let input = self.process.stdin.as_mut().ok_or("硬件助手正在退出")?;
        if !self.services.contains(&service) {
            self.services.push(service);
        }
        if self.ready {
            start_control(input, service)?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct Hardware {
    lighting: Worker,
    cooler: Worker,
    turbo: Worker,
    authorized: ControlWorker,
    closing: AtomicBool,
    system: Arc<Mutex<()>>,
}

impl Hardware {
    fn worker(&self, service: Service) -> &Worker {
        match service {
            Service::Lighting => &self.lighting,
            Service::Cooler => &self.cooler,
            Service::Turbo => &self.turbo,
        }
    }

    fn stop_all(&self) {
        self.closing.store(true, Ordering::Release);
        for worker in [&self.lighting, &self.cooler, &self.turbo] {
            if let Ok(mut slot) = worker.lock() {
                if let Some(running) = slot.as_mut() {
                    running.stop();
                }
            }
        }
        if let Ok(mut slot) = self.authorized.lock() {
            if let Some(running) = slot.as_mut() {
                running.process.stop();
            }
        }
    }
}

#[tauri::command]
async fn telemetry() -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(telemetry::snapshot)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn device_catalog() -> Value {
    catalog::snapshot()
}

#[tauri::command]
async fn open_about_link(url: String) -> Result<(), String> {
    // 只把关于页列出的地址交给系统浏览器。
    match url.as_str() {
        "https://github.com/leychan/revo-panel"
        | "https://aur.archlinux.org/packages/revo-panel-bin"
        | "https://github.com/leychan" => {}
        _ => return Err("未知的项目链接".into()),
    }
    tauri::async_runtime::spawn_blocking(move || {
        gio::AppInfo::launch_default_for_uri(&url, None::<&gio::AppLaunchContext>)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn system_request(
    hardware: State<'_, Hardware>,
    request: system::Request,
) -> Result<Value, String> {
    let system = hardware.system.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // 跨页面操作也依次执行，每个系统设置在读回后才处理下一条。
        let _guard = system.lock().map_err(|error| error.to_string())?;
        system::run(request)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn hardware_executable() -> Result<PathBuf, String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("无法定位 RevoPanel 执行文件：{error}"))?;
    let on_disk = fs::metadata(&executable).map_err(|error| match error.kind() {
        ErrorKind::NotFound => RESTART_REQUIRED.to_owned(),
        _ => format!("无法访问 RevoPanel 执行文件：{error}"),
    })?;
    // 托盘中的旧进程可能还活着，硬件会话必须与正在运行的桌面版本一致。
    let running = fs::metadata("/proc/self/exe")
        .map_err(|error| format!("无法检查运行中的 RevoPanel：{error}"))?;
    if (on_disk.dev(), on_disk.ino()) != (running.dev(), running.ino()) {
        return Err(RESTART_REQUIRED.to_owned());
    }
    Ok(executable)
}

struct SpawnedWorker {
    process: Running,
    stdout: ChildStdout,
    child: Arc<Mutex<Child>>,
    errors: thread::JoinHandle<String>,
}

fn spawn_worker(mode: &str, elevate: bool) -> Result<SpawnedWorker, String> {
    let executable = hardware_executable()?;
    let mut command = if elevate {
        let mut command = Command::new(PKEXEC);
        command.arg("--disable-internal-agent").arg(&executable);
        command
    } else {
        Command::new(&executable)
    };
    // 同一个二进制运行独立驱动会话，仅在打开受限句柄时短暂提权。
    command.arg(mode);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == ErrorKind::NotFound && !executable.exists() {
                RESTART_REQUIRED.to_owned()
            } else if elevate && error.kind() == ErrorKind::NotFound && !Path::new(PKEXEC).exists()
            {
                "找不到 pkexec，请安装 polkit 后重试".into()
            } else if elevate {
                format!("无法启动系统授权：{error}")
            } else {
                format!("无法启动硬件助手：{error}")
            }
        })?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let stdin = child.stdin.take();
    let child = Arc::new(Mutex::new(child));

    // 持续读取 stderr，避免错误输出占满管道而阻塞硬件读回。
    let errors = thread::spawn(move || {
        BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
            .last()
            .unwrap_or_default()
    });
    Ok(SpawnedWorker {
        process: Running {
            stdin,
            pending_auth: elevate.then(|| child.clone()),
        },
        stdout,
        child,
        errors,
    })
}

fn worker_exit(child: Arc<Mutex<Child>>, errors: thread::JoinHandle<String>) -> Value {
    let code = child
        .lock()
        .ok()
        .and_then(|mut child| child.wait().ok())
        .and_then(|status| status.code());
    serde_json::json!({ "code": code, "detail": errors.join().unwrap_or_default() })
}

fn write_request(input: &mut ChildStdin, request: &Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    if bytes.len() >= 1024 {
        return Err("硬件请求过长".into());
    }
    bytes.push(b'\n');
    input.write_all(&bytes).map_err(|error| error.to_string())
}

fn start_control(input: &mut ChildStdin, service: Service) -> Result<(), String> {
    let request = match service {
        Service::Lighting => Value::Null,
        Service::Turbo => serde_json::json!({"action": "turbo_policy", "enabled": true}),
        Service::Cooler => return Err("未知的硬件操作".into()),
    };
    write_request(
        input,
        &serde_json::json!({"service": service, "request": request}),
    )
}

fn start_authorized(
    app: &AppHandle,
    worker: &ControlWorker,
    service: Service,
    elevate: bool,
) -> Result<bool, String> {
    let mut slot = worker.lock().map_err(|error| error.to_string())?;
    if let Some(running) = slot.as_mut() {
        running.start(service)?;
        return Ok(true);
    }
    if !elevate {
        return Ok(false);
    }
    let SpawnedWorker {
        process,
        stdout,
        child,
        errors,
    } = spawn_worker("--controls-session", true)?;
    *slot = Some(AuthorizedControls {
        process,
        ready: false,
        services: vec![service],
    });

    // 两项控制共享这个已授权的进程。只启用已经由桌面请求的控制。
    let worker = worker.clone();
    let app = app.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(event) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if event["ready"] == true {
                if let Ok(mut slot) = worker.lock() {
                    if let Some(running) = slot.as_mut().filter(|running| !running.ready) {
                        running.process.pending_auth = None;
                        running.ready = true;
                        if let Some(input) = running.process.stdin.as_mut() {
                            for &service in &running.services {
                                if start_control(input, service).is_err() {
                                    running.process.stdin.take();
                                    break;
                                }
                            }
                        }
                    }
                }
            } else {
                if event["kind"] == "exit" {
                    let service = serde_json::from_value::<Service>(event["service"].clone());
                    if let (Ok(service), Ok(mut slot)) = (service, worker.lock()) {
                        if let Some(running) = slot.as_mut() {
                            running.services.retain(|active| *active != service);
                        }
                    }
                }
                let _ = app.emit("hardware-event", event);
            }
        }
        let detail = worker_exit(child, errors);
        let running = worker.lock().ok().and_then(|mut slot| slot.take());
        if let Some(running) = running {
            for service in running.services {
                let _ = app.emit(
                    "hardware-event",
                    WorkerEvent {
                        service,
                        kind: "exit",
                        data: detail.clone(),
                    },
                );
            }
        }
    });
    Ok(true)
}

#[tauri::command]
fn start_hardware(
    app: AppHandle,
    hardware: State<'_, Hardware>,
    service: Service,
    elevate: bool,
) -> Result<(), String> {
    let mode = match service {
        Service::Lighting => "--lighting-session",
        Service::Cooler if !elevate => "--cooler-session",
        Service::Cooler => return Err("水冷箱使用系统蓝牙服务，无需管理员权限".into()),
        Service::Turbo => "--turbo-session",
    };
    let worker = hardware.worker(service).clone();
    let mut slot = worker.lock().map_err(|error| error.to_string())?;
    if hardware.closing.load(Ordering::Acquire) {
        return Err("硬件助手正在退出".into());
    }
    if let Some(running) = slot.as_ref() {
        return if running.stdin.is_some() {
            Ok(())
        } else {
            Err("硬件助手正在退出".into())
        };
    }

    if service != Service::Cooler && start_authorized(&app, &hardware.authorized, service, elevate)?
    {
        return Ok(());
    }

    let SpawnedWorker {
        process,
        stdout,
        child,
        errors,
    } = spawn_worker(mode, false)?;
    *slot = Some(process);
    drop(slot);
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let data = serde_json::from_str::<Value>(&line)
                .unwrap_or_else(|_| serde_json::json!({"error": "无法解析设备回复"}));
            if service == Service::Turbo && data["ready"] == true {
                if let Ok(mut slot) = worker.lock() {
                    if let Some(input) = slot.as_mut().and_then(|running| running.stdin.as_mut()) {
                        let _ = write_request(
                            input,
                            &serde_json::json!({"action": "turbo_policy", "enabled": true}),
                        );
                    }
                }
                continue;
            }
            let _ = app.emit(
                "hardware-event",
                WorkerEvent {
                    service,
                    kind: "message",
                    data,
                },
            );
        }
        let data = worker_exit(child, errors);
        if let Ok(mut slot) = worker.lock() {
            *slot = None;
        }
        let _ = app.emit(
            "hardware-event",
            WorkerEvent {
                service,
                kind: "exit",
                data,
            },
        );
    });
    Ok(())
}

#[tauri::command]
fn send_hardware(
    hardware: State<'_, Hardware>,
    service: Service,
    request: Value,
) -> Result<(), String> {
    let action = request
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let allowed = match service {
        Service::Lighting => matches!(action, "status" | "brightness" | "effect" | "chassis"),
        Service::Cooler => matches!(
            action,
            "list" | "scan" | "connect" | "disconnect" | "pump" | "fan" | "lighting"
        ),
        Service::Turbo => action == "turbo_policy",
    };
    if !allowed {
        return Err("未知的硬件操作".into());
    }
    let mut slot = hardware
        .worker(service)
        .lock()
        .map_err(|error| error.to_string())?;
    if let Some(input) = slot.as_mut().and_then(|running| running.stdin.as_mut()) {
        return write_request(input, &request);
    }
    if service != Service::Cooler {
        let mut slot = hardware
            .authorized
            .lock()
            .map_err(|error| error.to_string())?;
        if let Some(input) = slot
            .as_mut()
            .filter(|running| running.ready && running.services.contains(&service))
            .and_then(|running| running.process.stdin.as_mut())
        {
            return write_request(
                input,
                &serde_json::json!({"service": service, "request": request}),
            );
        }
    }
    Err("硬件助手未连接".into())
}

#[tauri::command]
fn stop_hardware(hardware: State<'_, Hardware>) {
    hardware.stop_all();
}

fn command_line() -> Option<ExitCode> {
    let mut args = std::env::args_os().skip(1);
    let argument = args.next()?;
    if argument.to_str() == Some("--turbo-session") {
        return Some(match turbo::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("睿频限制：{error}");
                ExitCode::from(if error.kind() == ErrorKind::PermissionDenied {
                    77
                } else {
                    1
                })
            }
        });
    }
    let operation = match argument.to_str() {
        Some("--lighting-session") => Some(lighting::Operation::Session),
        Some("--keyboard-status") => Some(lighting::Operation::KeyboardStatus),
        Some("--chassis-status") => Some(lighting::Operation::ChassisStatus),
        Some("--verify-brightness") => Some(lighting::Operation::VerifyBrightness),
        _ => None,
    };
    if let Some(operation) = operation {
        return Some(match lighting::run(operation) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("灯控：{error}");
                ExitCode::from(if error.kind() == ErrorKind::PermissionDenied {
                    77
                } else {
                    1
                })
            }
        });
    }
    let print = |result: Result<Value, String>| {
        result.and_then(|value| linux::emit(&value).map_err(|error| error.to_string()))
    };
    let result = match argument.to_str() {
        Some("--controls-session") => controls::run().map_err(|error| error.to_string()),
        Some("--cooler-session") => cooler::run(false),
        Some("--cooler-list") => cooler::run(true),
        Some("--telemetry") => print(Ok(telemetry::snapshot())),
        Some("--catalog") => print(Ok(catalog::snapshot())),
        Some("--system-status") => print(system::run(system::Request::Read)),
        Some("--gpu-status") => print(system::run(system::Request::Gpu)),
        Some("--system-request") => {
            let request = args
                .next()
                .and_then(|arg| arg.into_string().ok())
                .ok_or_else(|| "需要 JSON 系统请求".to_owned())
                .and_then(|json| serde_json::from_str(&json).map_err(|error| error.to_string()));
            print(request.and_then(system::run))
        }
        Some("--help" | "-h") => {
            println!("RevoPanel\n\n无参数启动桌面。诊断命令：\n  --telemetry            Linux 传感器和电池 JSON\n  --catalog              灯控和水冷箱选项\n  --system-status        电源、屏幕和触控板\n  --gpu-status           NVIDIA 遥测\n  --cooler-list          BlueZ 已知水冷箱\n  --keyboard-status      键盘灯状态\n  --chassis-status       机身灯控分区\n  --verify-brightness    短暂试调并还原亮度");
            Ok(())
        }
        _ => {
            eprintln!("未知参数 {}，请查看 --help", argument.to_string_lossy());
            return Some(ExitCode::from(2));
        }
    };
    Some(match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    })
}

fn main() -> ExitCode {
    // 诊断和硬件会话在初始化 GTK 之前运行，提权子进程不会启动桌面窗口。
    if let Some(code) = command_line() {
        return code;
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            desktop::show_window(app);
        }))
        .manage(Hardware::default())
        .manage(desktop::Desktop::default())
        .setup(|app| {
            if let Err(error) = desktop::install_tray(app) {
                eprintln!("无法安装系统托盘：{error}");
            }
            Ok(())
        })
        .on_window_event(desktop::handle_window_event)
        .invoke_handler(tauri::generate_handler![
            telemetry,
            device_catalog,
            open_about_link,
            system_request,
            start_hardware,
            send_hardware,
            stop_hardware,
            desktop::system_language,
            desktop::set_language,
            desktop::desktop_ready,
            desktop::finish_exit
        ])
        .build(tauri::generate_context!())
        .expect("无法初始化 RevoPanel")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                let hardware = app.state::<Hardware>();
                hardware.stop_all();
            }
        });
    ExitCode::SUCCESS
}
