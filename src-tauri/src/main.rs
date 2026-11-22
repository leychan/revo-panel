mod catalog;
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
    io::{BufRead, BufReader, ErrorKind, Write},
    process::{Child, ChildStdin, Command, ExitCode, Stdio},
    sync::{Arc, Mutex},
    thread,
};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

#[derive(Clone, Copy, Deserialize, Serialize)]
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

type Worker = Arc<Mutex<Option<Running>>>;

#[derive(Default)]
struct Hardware {
    lighting: Worker,
    cooler: Worker,
    turbo: Worker,
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

    fn stop(&self, service: Service) {
        if let Ok(mut slot) = self.worker(service).lock() {
            if let Some(running) = slot.as_mut() {
                // EOF 会结束会话。睿频助手恢复其改过的值后才退出。
                running.stdin.take();
                // 尚未完成的系统认证不会读取 stdin，需要一并取消。
                if let Some(child) = running.pending_auth.take() {
                    if let Ok(mut child) = child.try_lock() {
                        let _ = child.kill();
                    }
                }
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
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let worker = hardware.worker(service).clone();
    let mut slot = worker.lock().map_err(|error| error.to_string())?;
    if let Some(running) = slot.as_ref() {
        return if running.stdin.is_some() {
            Ok(())
        } else {
            Err("硬件助手正在退出".into())
        };
    }

    let mut command = if elevate {
        let mut command = Command::new("/usr/bin/pkexec");
        command.arg("--disable-internal-agent").arg(executable);
        command
    } else {
        Command::new(executable)
    };
    // 同一个二进制运行独立驱动会话，仅在打开受限句柄时短暂提权。
    command.arg(mode);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let stdin = child.stdin.take();
    let child = Arc::new(Mutex::new(child));
    *slot = Some(Running {
        stdin,
        pending_auth: elevate.then(|| child.clone()),
    });
    drop(slot);

    // 持续读取 stderr，避免错误输出占满管道而阻塞蓝牙通知和 HID 读回。
    let errors = thread::spawn(move || {
        BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
            .last()
            .unwrap_or_default()
    });
    thread::spawn(move || {
        let mut awaiting_auth = elevate;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let event = match serde_json::from_str(&line) {
                Ok(data) => WorkerEvent {
                    service,
                    kind: "message",
                    data,
                },
                Err(_) => WorkerEvent {
                    service,
                    kind: "message",
                    data: serde_json::json!({"error": "无法解析设备回复"}),
                },
            };
            if matches!(service, Service::Turbo) && event.data["ready"] == true {
                awaiting_auth = false;
                if let Ok(mut slot) = worker.lock() {
                    if let Some(running) = slot.as_mut() {
                        // 先清除认证中的标记，再允许助手写入，确保停止时能让它恢复睿频。
                        running.pending_auth = None;
                        if let Some(input) = running.stdin.as_mut() {
                            if input
                                .write_all(b"{\"action\":\"turbo_policy\",\"enabled\":true}\n")
                                .is_err()
                            {
                                running.stdin.take();
                            }
                        }
                    }
                }
                continue;
            }
            if awaiting_auth && event.data.get("state").is_some() {
                awaiting_auth = false;
                if let Ok(mut slot) = worker.lock() {
                    if let Some(running) = slot.as_mut() {
                        running.pending_auth = None;
                    }
                }
            }
            let _ = app.emit("hardware-event", event);
        }
        let code = child
            .lock()
            .ok()
            .and_then(|mut child| child.wait().ok())
            .and_then(|status| status.code());
        let detail = errors.join().unwrap_or_default();
        if let Ok(mut slot) = worker.lock() {
            *slot = None;
        }
        let _ = app.emit(
            "hardware-event",
            WorkerEvent {
                service,
                kind: "exit",
                data: serde_json::json!({ "code": code, "detail": detail }),
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
    let mut bytes = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    if bytes.len() >= 1024 {
        return Err("硬件请求过长".into());
    }
    bytes.push(b'\n');
    let mut slot = hardware
        .worker(service)
        .lock()
        .map_err(|error| error.to_string())?;
    let input = slot
        .as_mut()
        .and_then(|running| running.stdin.as_mut())
        .ok_or_else(|| "硬件助手未连接".to_string())?;
    input.write_all(&bytes).map_err(|error| error.to_string())
}

#[tauri::command]
fn stop_hardware(hardware: State<'_, Hardware>, service: Service) {
    hardware.stop(service);
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
            system_request,
            start_hardware,
            send_hardware,
            stop_hardware,
            desktop::desktop_ready,
            desktop::finish_exit
        ])
        .build(tauri::generate_context!())
        .expect("无法初始化 RevoPanel")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                let hardware = app.state::<Hardware>();
                hardware.stop(Service::Lighting);
                hardware.stop(Service::Cooler);
                hardware.stop(Service::Turbo);
            }
        });
    ExitCode::SUCCESS
}
