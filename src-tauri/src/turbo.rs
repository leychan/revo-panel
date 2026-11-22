use crate::linux::{drop_pkexec_privileges, emit, entries, request_line, text};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    sync::mpsc,
    thread,
    time::Duration,
};

const TURBO: &str = "devices/system/cpu/intel_pstate/no_turbo";
const SUPPLIES: &str = "/sys/class/power_supply";

pub fn restricted(sysfs: &Path) -> Option<bool> {
    match text(sysfs.join(TURBO)).as_deref() {
        Some("0") => Some(false),
        Some("1") => Some(true),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PowerSource {
    Battery,
    External,
    Unknown,
}

fn power_source(supplies: &Path) -> PowerSource {
    let mut discharging = false;
    let mut checked = false;
    let mut unknown = false;
    for device in entries(supplies) {
        match text(device.join("type")).as_deref() {
            Some("Battery")
                if text(device.join("present")).as_deref() != Some("0")
                    && text(device.join("scope")).as_deref() != Some("Device") =>
            {
                match text(device.join("status")).as_deref() {
                    Some("Charging") => return PowerSource::External,
                    Some("Discharging") => discharging = true,
                    _ => {}
                }
            }
            Some(kind)
                if (kind == "Mains" || kind == "Wireless" || kind.starts_with("USB"))
                    && text(device.join("scope")).as_deref() != Some("Device") =>
            {
                match text(device.join("online")).and_then(|value| value.parse::<u8>().ok()) {
                    Some(0) => checked = true,
                    Some(_) => return PowerSource::External,
                    None => unknown = true,
                }
            }
            _ => {}
        }
    }
    // 只有确认电池在放电且外部电源全离线，才接管睿频。
    if checked && !unknown && discharging {
        PowerSource::Battery
    } else {
        PowerSource::Unknown
    }
}

pub(crate) struct Session {
    file: File,
    restore_pending: bool,
}

impl Session {
    pub(crate) fn open() -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(Path::new("/sys").join(TURBO))?;
        Ok(Self {
            file,
            restore_pending: false,
        })
    }

    pub(crate) fn run(
        mut self,
        requests: mpsc::Receiver<bool>,
        report: impl Fn(&Value) -> io::Result<()>,
    ) -> io::Result<()> {
        let result = monitor(&mut self, requests, Path::new(SUPPLIES), report);
        self.restore()
            .map_err(|error| io::Error::other(format!("恢复睿频状态失败：{error}")))?;
        result
    }

    fn read(&mut self) -> io::Result<bool> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut byte = [0];
        self.file.read_exact(&mut byte)?;
        match byte[0] {
            b'0' => Ok(false),
            b'1' => Ok(true),
            _ => Err(io::Error::other("内核返回了未知的睿频状态")),
        }
    }

    fn write(&mut self, restricted: bool) -> io::Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        self.file
            .write_all(if restricted { b"1\n" } else { b"0\n" })?;
        if self.read()? != restricted {
            return Err(io::Error::other("内核未读回请求的睿频状态"));
        }
        Ok(())
    }

    fn restrict(&mut self) -> io::Result<()> {
        if !self.read()? {
            // 只恢复由本会话施加的限制，保留系统原本关闭睿频的状态。
            self.restore_pending = true;
            self.write(true)?;
        }
        Ok(())
    }

    fn restore(&mut self) -> io::Result<()> {
        if self.restore_pending {
            if self.read()? {
                self.write(false)?;
            }
            self.restore_pending = false;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum Request {
    TurboPolicy { enabled: bool },
}

#[derive(PartialEq, Serialize)]
struct Status {
    enabled: bool,
    restricted: bool,
    power_source: PowerSource,
}

fn monitor(
    session: &mut Session,
    requests: mpsc::Receiver<bool>,
    supplies: &Path,
    report: impl Fn(&Value) -> io::Result<()>,
) -> io::Result<()> {
    // 主程序接到就绪消息后才允许修改，关闭认证窗口时不会留下未恢复的设置。
    let Ok(mut enabled) = requests.recv() else {
        return Ok(());
    };
    let mut previous = None;
    let mut on_battery = false;
    loop {
        let source = if enabled {
            power_source(supplies)
        } else {
            PowerSource::Unknown
        };
        let should_limit = source == PowerSource::Battery;
        if should_limit && !on_battery {
            session.restrict()?;
        } else if !should_limit {
            session.restore()?;
        }
        on_battery = should_limit;
        let current = Status {
            enabled,
            restricted: session.read()?,
            power_source: source,
        };
        if previous.as_ref() != Some(&current) {
            report(&json!({"turbo": current}))?;
            previous = Some(current);
        }

        // 禁用时释放策略，保留授权句柄以便重新启用，期间等待命令而不轮询。
        let result = if enabled {
            requests.recv_timeout(Duration::from_secs(2))
        } else {
            requests
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        match result {
            Ok(value) => enabled = value,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

pub fn run() -> io::Result<()> {
    let mut session = Session::open()?;
    drop_pkexec_privileges()?;
    session.read()?;
    emit(&json!({"ready": true}))?;
    let (send, requests) = mpsc::channel();
    thread::spawn(move || {
        let mut input = io::stdin().lock();
        while let Ok(Some(line)) = request_line(&mut input) {
            let Ok(Request::TurboPolicy { enabled }) = serde_json::from_slice(&line) else {
                break;
            };
            if send.send(enabled).is_err() {
                break;
            }
        }
    });
    session.run(requests, emit)
}
