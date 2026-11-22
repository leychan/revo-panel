use crate::linux::{entries, name, text};
use dbus::{
    arg::PropMap,
    blocking::{stdintf::org_freedesktop_dbus::Properties, Connection},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{process::Command, thread, time::Duration};

const POWER: &str = "org.freedesktop.UPower.PowerProfiles";
const POWER_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
const KWIN: &str = "org.kde.KWin";
const INPUT: &str = "org.kde.KWin.InputDevice";
const DBUS_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Request {
    Read,
    Gpu,
    Power { value: String },
    Brightness { value: u8 },
    RefreshRate { value: String },
    Touchpad { enabled: bool },
}

#[derive(Serialize)]
struct PowerOption {
    name: String,
    platform_driver: Option<String>,
}

#[derive(Serialize)]
struct PowerStatus {
    active: String,
    options: Vec<PowerOption>,
}

fn power_profiles(connection: &Connection) -> Result<PowerStatus, String> {
    let proxy = connection.with_proxy(POWER, POWER_PATH, DBUS_TIMEOUT);
    let active: String = proxy
        .get(POWER, "ActiveProfile")
        .map_err(|error| error.to_string())?;
    let profiles: Vec<PropMap> = proxy
        .get(POWER, "Profiles")
        .map_err(|error| error.to_string())?;
    let options = profiles
        .iter()
        .filter_map(|profile| {
            let name = profile.get("Profile")?.0.as_str()?.to_owned();
            let platform_driver = profile
                .get("PlatformDriver")
                .and_then(|value| value.0.as_str())
                .map(str::to_owned);
            Some(PowerOption {
                name,
                platform_driver,
            })
        })
        .collect();
    Ok(PowerStatus { active, options })
}

fn power_status() -> Result<PowerStatus, String> {
    let connection = Connection::new_system().map_err(|error| error.to_string())?;
    power_profiles(&connection)
}

fn set_power(value: String) -> Result<(), String> {
    let connection = Connection::new_system().map_err(|error| error.to_string())?;
    if !power_profiles(&connection)?
        .options
        .iter()
        .any(|profile| profile.name == value)
    {
        return Err("当前系统不提供所选电源档位".into());
    }
    connection
        .with_proxy(POWER, POWER_PATH, DBUS_TIMEOUT)
        .set(POWER, "ActiveProfile", value.as_str())
        .map_err(|error| error.to_string())?;
    if power_profiles(&connection)?.active != value {
        return Err("系统未读回所选电源档位".into());
    }
    Ok(())
}

fn command(program: &str, arguments: &[&str]) -> Result<String, String> {
    // KScreen 和 NVIDIA 驱动工具可能无响应。保留原有的六秒超时，参数直接传给进程。
    let output = Command::new("/usr/bin/timeout")
        .args(["--kill-after=1s", "6s", program])
        .args(arguments)
        .output()
        .map_err(|error| format!("{program}：{error}"))?;
    if matches!(output.status.code(), Some(124 | 137)) {
        return Err(format!("{program} 读取超时"));
    }
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        let message = message.trim();
        return Err(if message.is_empty() {
            format!("{program} 未成功执行")
        } else {
            message.into()
        });
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

#[derive(Deserialize)]
struct KScreen {
    outputs: Vec<KScreenOutput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KScreenOutput {
    name: String,
    enabled: bool,
    connected: bool,
    current_mode_id: String,
    modes: Vec<Mode>,
    brightness: Option<f64>,
    #[serde(default)]
    hdr: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Mode {
    id: String,
    name: String,
    refresh_rate: f64,
    size: Dimensions,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
struct Dimensions {
    width: u32,
    height: u32,
}

#[derive(Serialize)]
struct Display {
    name: String,
    current_mode: Mode,
    modes: Vec<Mode>,
    brightness: Option<u8>,
    hdr: bool,
}

fn internal_display() -> Result<Display, String> {
    let screen: KScreen = serde_json::from_str(&command("kscreen-doctor", &["-j"])?)
        .map_err(|error| error.to_string())?;
    let output = screen
        .outputs
        .into_iter()
        .find(|output| {
            output.connected
                && output.enabled
                && ["eDP-", "LVDS-", "DSI-"]
                    .iter()
                    .any(|prefix| output.name.starts_with(prefix))
        })
        .ok_or("KScreen 未检测到启用的笔记本内屏")?;
    let current_mode = output
        .modes
        .iter()
        .find(|mode| mode.id == output.current_mode_id)
        .cloned()
        .ok_or("KScreen 未返回内屏当前显示模式")?;
    // 只列出当前分辨率下的刷新率，切换时保留分辨率与缩放。
    let mut modes: Vec<_> = output
        .modes
        .into_iter()
        .filter(|mode| mode.size == current_mode.size)
        .collect();
    modes.sort_by(|left, right| left.refresh_rate.total_cmp(&right.refresh_rate));
    let brightness = output
        .brightness
        .filter(|value| (0.0..=1.0).contains(value))
        .map(|value| (value * 100.0).round() as u8);
    Ok(Display {
        name: output.name,
        current_mode,
        modes,
        brightness,
        hdr: output.hdr,
    })
}

enum DisplaySetting {
    Brightness(u8),
    Rate(String),
}

fn set_display(setting: DisplaySetting) -> Result<(), String> {
    let current = internal_display()?;
    let option = match &setting {
        DisplaySetting::Brightness(value) => {
            if !(1..=100).contains(value) || current.brightness.is_none() || current.hdr {
                return Err("当前内屏不支持该 SDR 亮度设置".into());
            }
            format!("output.{}.brightness.{value}", current.name)
        }
        DisplaySetting::Rate(id) => {
            if !current.modes.iter().any(|mode| &mode.id == id) {
                return Err("当前分辨率不提供所选刷新率".into());
            }
            format!("output.{}.mode.{id}", current.name)
        }
    };
    command("kscreen-doctor", &[&option])?;
    for _ in 0..3 {
        let readback = internal_display()?;
        let matches = match &setting {
            DisplaySetting::Brightness(value) => readback.brightness == Some(*value),
            DisplaySetting::Rate(id) => &readback.current_mode.id == id,
        };
        if matches {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(200));
    }
    Err("内屏未读回目标设置".into())
}

#[derive(Serialize)]
struct Touchpad {
    path: String,
    enabled: bool,
}

fn touchpad(connection: &Connection) -> Result<Touchpad, String> {
    for node in entries("/sys/class/input") {
        let event = name(&node);
        if !event.starts_with("event")
            || !text(node.join("device/name"))
                .is_some_and(|name| name.starts_with("UNIW0001:") && name.ends_with("Touchpad"))
        {
            continue;
        }
        let path = format!("/org/kde/KWin/InputDevice/{event}");
        let proxy = connection.with_proxy(KWIN, path.as_str(), DBUS_TIMEOUT);
        let is_touchpad: bool = proxy
            .get(INPUT, "touchpad")
            .map_err(|error| error.to_string())?;
        let can_disable: bool = proxy
            .get(INPUT, "supportsDisableEvents")
            .map_err(|error| error.to_string())?;
        if is_touchpad && can_disable {
            let enabled = proxy
                .get(INPUT, "enabled")
                .map_err(|error| error.to_string())?;
            return Ok(Touchpad { path, enabled });
        }
    }
    Err("KWin 未公开这台耀世的可切换触控板".into())
}

fn touchpad_status() -> Result<Touchpad, String> {
    let connection = Connection::new_session().map_err(|error| error.to_string())?;
    touchpad(&connection)
}

fn set_touchpad(enabled: bool) -> Result<(), String> {
    let connection = Connection::new_session().map_err(|error| error.to_string())?;
    let device = touchpad(&connection)?;
    connection
        .with_proxy(KWIN, device.path.as_str(), DBUS_TIMEOUT)
        .set(INPUT, "enabled", enabled)
        .map_err(|error| error.to_string())?;
    if touchpad(&connection)?.enabled != enabled {
        return Err("KWin 未读回所选触控板状态".into());
    }
    Ok(())
}

fn gpu_metrics() -> Result<Value, String> {
    // 只在按需读取时调用驱动工具，避免定时唤醒独显。
    const KEYS: &[&str] = &[
        "name",
        "temperature.gpu",
        "utilization.gpu",
        "memory.used",
        "memory.total",
        "power.draw",
    ];
    let query = format!("--query-gpu={}", KEYS.join(","));
    let output = command("nvidia-smi", &[&query, "--format=csv,noheader,nounits"])?;
    let gpus: Vec<serde_json::Map<String, Value>> = output
        .lines()
        .filter_map(|line| {
            // 从数值列开始分割，以保留型号字段中可能出现的逗号。
            let columns: Vec<_> = line.rsplitn(KEYS.len(), ',').map(str::trim).collect();
            (columns.len() == KEYS.len()).then(|| {
                KEYS.iter()
                    .zip(columns.into_iter().rev())
                    .map(|(key, value)| (key.to_string(), json!(value.trim_matches('"'))))
                    .collect()
            })
        })
        .collect();
    Ok(json!({"gpus": gpus}))
}

fn status<T: Serialize>(result: Result<T, String>) -> Value {
    match result {
        Ok(value) => json!(value),
        Err(error) => json!({"error": error}),
    }
}

pub fn run(request: Request) -> Result<Value, String> {
    match request {
        Request::Read => {}
        Request::Gpu => return gpu_metrics(),
        Request::Power { value } => set_power(value)?,
        Request::Brightness { value } => set_display(DisplaySetting::Brightness(value))?,
        Request::RefreshRate { value } => set_display(DisplaySetting::Rate(value))?,
        Request::Touchpad { enabled } => set_touchpad(enabled)?,
    }
    Ok(
        json!({"power": status(power_status()), "display": status(internal_display()), "touchpad": status(touchpad_status())}),
    )
}
