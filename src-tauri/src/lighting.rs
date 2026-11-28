use crate::{
    catalog::{Effect, Zone, BRIGHTNESS_MAX, CHASSIS_EFFECTS, KEYBOARD_EFFECTS, USER_MODE, ZONES},
    linux::{drop_pkexec_privileges, emit, entries, name, request_line, rgb, text},
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{FileTypeExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

const USB: u32 = 3;
const ITE: u16 = 0x048d;
const KEYBOARD: u16 = 0x600b;
const LIGHTBAR: u16 = 0x7001;
const REPORT: usize = 9;
const SET_FEATURE: u8 = 0x06;
const GET_FEATURE: u8 = 0x07;
const RAW_INFO: u8 = 0x03;
const QUERY_STATE: u8 = 0x88;
const QUERY_ZONES: u8 = 0xa2;
const SET_EFFECT: u8 = 0x08;
const SET_BRIGHTNESS: u8 = 0x09;
const ROW_TRANSFER: u8 = 0x16;
const SET_PALETTE: u8 = 0x14;
const POWER_OFF: u8 = 0x01;
const POWER_ON: u8 = 0x02;
const EFFECT_DELAY: Duration = Duration::from_millis(250);
const KEYBOARD_ROWS: u8 = 6;
const KEYBOARD_COLUMNS: usize = 21;
const OUTPUT_REPORT: usize = 2 + 3 * KEYBOARD_COLUMNS;
const COLORS: [[u8; 3]; 8] = [
    [255, 0, 0],
    [255, 128, 0],
    [255, 255, 0],
    [0, 255, 0],
    [0, 255, 255],
    [0, 80, 255],
    [128, 0, 255],
    [255, 0, 255],
];

type EffectState = [u8; 8];

pub enum Operation {
    Session,
    KeyboardStatus,
    ChassisStatus,
    VerifyBrightness,
}

fn error(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

fn milliseconds(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

fn ioctl_number(direction: u8, operation: u8, size: usize) -> libc::c_ulong {
    // x86_64 Linux 的 _IOC 位布局，与 linux/hidraw.h 一致。
    ((u64::from(direction) << 30)
        | ((size as u64) << 16)
        | (u64::from(b'H') << 8)
        | u64::from(operation)) as libc::c_ulong
}

#[repr(C)]
struct Identity {
    bus: u32,
    vendor: u16,
    product: u16,
}

fn find_node(product: u16, usage: u8, description: &str) -> io::Result<PathBuf> {
    if !cfg!(target_arch = "x86_64") {
        return Err(error("此灯控仅支持 x86_64 Linux"));
    }
    let dmi = Path::new("/sys/class/dmi/id");
    if text(dmi.join("sys_vendor")).as_deref() != Some("MECHREVO")
        || text(dmi.join("board_name")).as_deref() != Some("YAOSHI Series-X6AR55xY")
    {
        return Err(error("当前主板与耀世的验证机型不匹配"));
    }
    let expected = format!("HID_ID={USB:04X}:{ITE:08X}:{product:08X}");
    let mut devices = Vec::new();
    for entry in entries("/sys/class/hidraw") {
        if !name(&entry).starts_with("hidraw") {
            continue;
        }
        let device = entry.join("device");
        let Some(attributes) = text(device.join("uevent")) else {
            continue;
        };
        if !attributes.lines().any(|line| line == expected) {
            continue;
        }
        if !attributes.lines().any(|line| line == "DRIVER=hid-generic") {
            return Err(error(format!("{description}已由专用驱动接管")));
        }
        let descriptor = fs::read(device.join("report_descriptor"))?;
        // 锁定厂商 Usage 和八字节 Feature Report，排除同 VID/PID 的其他接口。
        if descriptor.starts_with(&[0x06, 0x03, 0xff, 0x09, usage])
            && descriptor.ends_with(&[0x09, 0x22, 0x95, 0x08, 0xb1, 0x02, 0xc0])
        {
            devices.push(Path::new("/dev").join(name(&entry)));
        }
    }
    if devices.len() != 1 {
        return Err(error(format!(
            "预期找到一个{description}灯控接口，实际找到 {} 个",
            devices.len()
        )));
    }
    Ok(devices.remove(0))
}

struct Hid(File);

impl Hid {
    fn open(product: u16, usage: u8, description: &str) -> io::Result<Self> {
        let node = find_node(product, usage, description)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&node)
            .map_err(|failure| {
                io::Error::new(
                    failure.kind(),
                    format!("访问 {description} {}：{failure}", node.display()),
                )
            })?;
        if !file.metadata()?.file_type().is_char_device() {
            return Err(error("灯控节点不是字符设备"));
        }
        let mut identity = Identity {
            bus: 0,
            vendor: 0,
            product: 0,
        };
        // ioctl 使用与内核 hidraw_devinfo 相同的 C 布局，并再次校验已打开句柄的身份。
        let result = unsafe {
            libc::ioctl(
                file.as_raw_fd(),
                ioctl_number(2, RAW_INFO, size_of::<Identity>()),
                &mut identity,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        if (identity.bus, identity.vendor, identity.product) != (USB, ITE, product) {
            return Err(error("打开的设备与预期 ITE 灯控标识不符"));
        }
        Ok(Self(file))
    }

    fn with_lock<T>(&self, operation: impl FnOnce(&Self) -> io::Result<T>) -> io::Result<T> {
        let fd = self.0.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let result = operation(self);
        unsafe { libc::flock(fd, libc::LOCK_UN) };
        result
    }

    fn feature(&self, operation: u8, report: &mut [u8; REPORT]) -> io::Result<usize> {
        // 未编号报告的第一个字节为 0，内核会从其后的八字节读取或填写数据。
        let count = unsafe {
            libc::ioctl(
                self.0.as_raw_fd(),
                ioctl_number(3, operation, REPORT),
                report.as_mut_ptr(),
            )
        };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(count as usize)
    }

    fn send(&self, payload: &[u8]) -> io::Result<()> {
        let mut report = feature_report(payload)?;
        let count = self.feature(SET_FEATURE, &mut report)?;
        if count != REPORT {
            return Err(error(format!("灯控设备返回异常的 HID 写入长度 {count}")));
        }
        Ok(())
    }

    fn query(&self, command: u8) -> io::Result<[u8; REPORT]> {
        self.send(&[command])?;
        let mut reply = [0; REPORT];
        let count = self.feature(GET_FEATURE, &mut reply)?;
        if count != REPORT || reply[0] != 0 || reply[1] != command {
            return Err(error(format!(
                "灯控设备未返回预期的状态报告（{}）",
                hex(&reply[..count.min(REPORT)])
            )));
        }
        Ok(reply)
    }

    fn effect(&self) -> io::Result<EffectState> {
        let reply = self.query(QUERY_STATE)?;
        if reply[5] > BRIGHTNESS_MAX {
            return Err(error(format!("键盘返回了超出范围的亮度 {}", reply[5])));
        }
        let mut effect = [0; 8];
        effect.copy_from_slice(&reply[1..]);
        effect[0] = SET_EFFECT;
        Ok(effect)
    }

    fn set_brightness(&self, level: u8) -> io::Result<EffectState> {
        if level > BRIGHTNESS_MAX {
            return Err(error(format!("亮度应为 0–{BRIGHTNESS_MAX} 的整数")));
        }
        self.send(&[SET_BRIGHTNESS, 0x02, level])?;
        thread::sleep(EFFECT_DELAY);
        let effect = self.effect()?;
        if effect[4] != level {
            return Err(error(format!(
                "亮度读回 {}/{BRIGHTNESS_MAX}，预期 {level}/{BRIGHTNESS_MAX}",
                effect[4]
            )));
        }
        Ok(effect)
    }

    fn fill_keyboard(&self, color: [u8; 3], brightness: u8) -> io::Result<()> {
        self.send(&[SET_EFFECT, POWER_ON, USER_MODE, 0, brightness, 0, 0, 0])?;
        milliseconds(20);
        let report = keyboard_row(color);
        let mut file = &self.0;
        // 使用 ITE 8291 的逐行写入协议，每行先发编号，再发该行的颜色。
        for row in 0..KEYBOARD_ROWS {
            self.send(&[ROW_TRANSFER, 0, row])?;
            // 一次 write 对应一个完整报告，短写不能继续发送剩余字节。
            if file.write(&report)? != report.len() {
                return Err(error(format!(
                    "键盘未接收第 {} 行的完整 RGB 数据包",
                    row + 1
                )));
            }
        }
        Ok(())
    }

    fn animation(
        &self,
        effect: &Effect,
        color: [u8; 3],
        speed: u8,
        direction: u8,
        brightness: u8,
    ) -> io::Result<()> {
        self.send(&[SET_EFFECT, POWER_OFF, 0, 0, 0, 0, 0, 0])?;
        milliseconds(20);
        if effect.id == "off" {
            return Ok(());
        }
        self.send(&[SET_EFFECT, POWER_ON, 0, 0, brightness, 0, 0, 0])?;
        milliseconds(15);
        let colors: &[[u8; 3]] = if effect.color {
            std::slice::from_ref(&color)
        } else if effect.id == "rainbow" {
            &[]
        } else {
            &COLORS
        };
        for (slot, [red, green, blue]) in colors.iter().copied().enumerate() {
            self.send(&[SET_PALETTE, 0, slot as u8 + 1, red, green, blue, 0, 0])?;
        }
        milliseconds(15);
        let slots = if effect.color { 1 } else { COLORS.len() as u8 };
        self.send(&[
            SET_EFFECT,
            POWER_ON,
            effect.code,
            speed,
            brightness,
            slots,
            direction,
            0,
        ])
    }

    fn restore(&self, original: EffectState) -> io::Result<()> {
        let mut command = original;
        command[7] = 0; // 只恢复运行状态，不保存到芯片闪存。
        self.send(&command)?;
        thread::sleep(EFFECT_DELAY);
        let state = self.effect()?;
        if state[1..7] != original[1..7] {
            return Err(error(format!(
                "灯效恢复读回不一致，原值 {}，读回 {}",
                hex(&original),
                hex(&state)
            )));
        }
        Ok(())
    }

    fn set_effect(
        &self,
        effect: &Effect,
        color: [u8; 3],
        speed: u8,
        direction: u8,
    ) -> io::Result<EffectState> {
        let original = self.effect()?;
        let brightness = if original[4] == 0 { 20 } else { original[4] };
        let operation = || {
            if effect.id == "static" {
                self.fill_keyboard(color, brightness)?;
            } else {
                self.animation(effect, color, speed, direction, brightness)?;
            }
            if effect.id != "off" {
                thread::sleep(EFFECT_DELAY);
            }
            let actual = self.effect()?;
            if effect.id == "off" {
                if !matches!(actual[1], POWER_OFF | 0x05) {
                    return Err(error("键盘未读回关闭状态"));
                }
            } else if (actual[1], actual[2], actual[4]) != (POWER_ON, effect.code, brightness) {
                return Err(error("键盘未读回预期的灯效模式或亮度"));
            }
            Ok(actual)
        };
        match operation() {
            Ok(state) => Ok(state),
            Err(failure) => match self.restore(original) {
                Err(restore) => Err(error(format!("{failure}。{restore}"))),
                Ok(()) if original[2] == USER_MODE => Err(error(format!(
                    "{failure}。已恢复模式和亮度，原自定义配色无法读回"
                ))),
                Ok(()) => Err(failure),
            },
        }
    }
}

fn feature_report(payload: &[u8]) -> io::Result<[u8; REPORT]> {
    if payload.len() >= REPORT {
        return Err(error("HID Feature Report 长度无效"));
    }
    let mut report = [0; REPORT];
    report[1..1 + payload.len()].copy_from_slice(payload);
    Ok(report)
}

fn keyboard_row(color: [u8; 3]) -> [u8; OUTPUT_REPORT] {
    // 第一个 0 是 HID 报告编号，第二个 0 是行数据的填充。之后按蓝、绿、红各写 21 字节。
    let mut report = [0; OUTPUT_REPORT];
    for (plane, channel) in report[2..]
        .chunks_exact_mut(KEYBOARD_COLUMNS)
        .zip(color.into_iter().rev())
    {
        plane.fill(channel);
    }
    report
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn state(effect: EffectState) -> Value {
    json!({
        "brightness": effect[4], "mode": effect[2], "powered": effect[1] == POWER_ON,
        "speed": effect[3], "direction": effect[6], "parameters": hex(&effect)
    })
}

fn chassis_probe(lightbar: Option<&Hid>) -> (Vec<&'static str>, Value) {
    let Some(lightbar) = lightbar else {
        return (vec![], json!({"zones": [], "status": "unavailable"}));
    };
    let reply = match lightbar.with_lock(|device| device.query(QUERY_ZONES)) {
        Ok(report) => report,
        Err(failure) => {
            return (
                vec!["front"],
                json!({"zones": ["front"], "status": "unconfirmed", "detail": failure.to_string()}),
            )
        }
    };
    let capability = reply[2];
    let (zones, status) = if capability > 11 {
        (vec!["front"], "unconfirmed")
    } else {
        (
            ZONES
                .iter()
                .filter(|zone| zone.capabilities.contains(&capability))
                .map(|zone| zone.id)
                .collect(),
            "confirmed",
        )
    };
    let probe =
        json!({"zones": zones, "status": status, "capability": capability, "report": hex(&reply)});
    (zones, probe)
}

fn set_chassis(
    hid: &Hid,
    zone: &Zone,
    effect: &Effect,
    brightness: u8,
    speed: u8,
    color: [u8; 3],
) -> io::Result<Value> {
    hid.with_lock(|device| {
        if effect.id == "off" || brightness == 0 {
            device.send(&[SET_EFFECT, zone.selector, 255, 1, 50, 0, 0, 0])?;
            return Ok(json!({"name": "off", "brightness": 0}));
        }
        if effect.color {
            let [red, green, blue] = color;
            device.send(&[SET_PALETTE, zone.palette, 1, red, green, blue, 0, 0])?;
            milliseconds(20);
        }
        // 最后一个字节为 0，分区灯效只写运行状态。
        let direction = u8::from(matches!(effect.id, "wave" | "meteor"));
        device.send(&[
            SET_EFFECT,
            zone.selector,
            effect.code,
            speed,
            brightness,
            if effect.color { 1 } else { 8 },
            direction,
            0,
        ])?;
        Ok(json!({"name": effect.id, "brightness": brightness}))
    })
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Request {
    Status,
    Brightness {
        level: u8,
    },
    Effect {
        name: String,
        color: String,
        speed: u8,
        direction: u8,
    },
    Chassis {
        zone: String,
        name: String,
        brightness: u8,
        speed: u8,
        color: String,
    },
}

fn apply(
    request: Request,
    keyboard: &Hid,
    lightbar: Option<&Hid>,
    zones: &[&str],
) -> io::Result<Value> {
    if let Request::Chassis {
        zone,
        name,
        brightness,
        speed,
        color,
    } = request
    {
        let hid = lightbar
            .filter(|_| zones.contains(&zone.as_str()))
            .ok_or_else(|| error("这个机身灯光通道不可用"))?;
        let zone = ZONES
            .iter()
            .find(|item| item.id == zone)
            .ok_or_else(|| error("未知的机身灯光通道"))?;
        let effect = CHASSIS_EFFECTS
            .iter()
            .find(|effect| effect.id == name && brightness <= 100 && (1..=10).contains(&speed))
            .ok_or_else(|| error("机身灯效、亮度或速度不正确"))?;
        return set_chassis(
            hid,
            zone,
            effect,
            brightness,
            speed,
            rgb(&color).map_err(error)?,
        )
        .map(|state| json!({"chassis": state}));
    }
    let actual = keyboard.with_lock(|device| match request {
        Request::Brightness { level } => device.set_brightness(level),
        Request::Effect {
            name,
            color,
            speed,
            direction,
        } => {
            let effect = KEYBOARD_EFFECTS
                .iter()
                .find(|effect| {
                    effect.id == name && (1..=10).contains(&speed) && (1..=4).contains(&direction)
                })
                .ok_or_else(|| error("无效的灯效、速度或方向"))?;
            device.set_effect(effect, rgb(&color).map_err(error)?, speed, direction)
        }
        Request::Status | Request::Chassis { .. } => unreachable!(),
    })?;
    Ok(json!({"state": state(actual)}))
}

pub(crate) struct Session {
    keyboard: Hid,
    lightbar: Option<Hid>,
    zones: Vec<&'static str>,
}

impl Session {
    pub(crate) fn open() -> io::Result<Self> {
        let keyboard = Hid::open(KEYBOARD, 1, "ITE 8291 键盘")?;
        let lightbar = match Hid::open(LIGHTBAR, 2, "ITE 8233 机身") {
            Ok(device) => Some(device),
            Err(error)
                if error.kind() == io::ErrorKind::PermissionDenied
                    && unsafe { libc::geteuid() } != 0 =>
            {
                return Err(error);
            }
            Err(_) => None,
        };
        Ok(Self {
            keyboard,
            lightbar,
            zones: Vec::new(),
        })
    }

    pub(crate) fn status(&mut self) -> io::Result<Value> {
        let initial = self.keyboard.with_lock(Hid::effect)?;
        let (zones, probe) = chassis_probe(self.lightbar.as_ref());
        self.zones = zones;
        Ok(json!({"state": state(initial), "chassis_probe": probe}))
    }

    pub(crate) fn request(&mut self, request: Value) -> io::Result<Value> {
        match serde_json::from_value(request)? {
            // 刷新与启动使用同一条读取路径，同时更新键盘读数和机身通道。
            Request::Status => self.status(),
            request => apply(request, &self.keyboard, self.lightbar.as_ref(), &self.zones),
        }
    }
}

pub fn run(operation: Operation) -> io::Result<()> {
    if matches!(operation, Operation::Session) {
        let mut session = Session::open()?;
        drop_pkexec_privileges()?;
        emit(&session.status()?)?;
        let mut input = io::stdin().lock();
        while let Some(line) = request_line(&mut input)? {
            let result = serde_json::from_slice(&line)
                .map_err(io::Error::from)
                .and_then(|request| session.request(request));
            emit(&result.unwrap_or_else(|error| json!({"error": error.to_string()})))?;
        }
        return Ok(());
    }
    if matches!(operation, Operation::ChassisStatus) {
        let lightbar = Hid::open(LIGHTBAR, 2, "ITE 8233 机身")?;
        drop_pkexec_privileges()?;
        return emit(&chassis_probe(Some(&lightbar)).1);
    }
    let keyboard = Hid::open(KEYBOARD, 1, "ITE 8291 键盘")?;
    drop_pkexec_privileges()?;
    keyboard.with_lock(|device| {
        let original = device.effect()?;
        if matches!(operation, Operation::VerifyBrightness) {
            let target = if original[4] >= 25 { 12 } else { 35 };
            let result = device.set_brightness(target);
            if result.is_ok() {
                thread::sleep(Duration::from_secs(1));
            }
            let restored = device.set_brightness(original[4])?;
            result?;
            emit(&json!({"verified": target, "restored": restored[4]}))
        } else {
            emit(&state(original))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unnumbered_reports_match_hidraw_abi() {
        assert_eq!(ioctl_number(3, SET_FEATURE, REPORT), 0xc0094806);
        assert_eq!(ioctl_number(3, GET_FEATURE, REPORT), 0xc0094807);
        assert_eq!(ioctl_number(2, RAW_INFO, size_of::<Identity>()), 0x80084803);
        assert_eq!(
            feature_report(&[SET_BRIGHTNESS, 2, 12]).unwrap(),
            [0, 9, 2, 12, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            feature_report(&[ROW_TRANSFER, 0, 5]).unwrap(),
            [0, 0x16, 0, 5, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn keyboard_rows_encode_each_selected_color() {
        for [red, green, blue] in [[0x32, 0xd4, 0xff], [0xff, 0x8d, 0xa0], [0, 0, 0]] {
            let report = keyboard_row([red, green, blue]);
            assert_eq!(report[..2], [0, 0]);
            assert_eq!(report[2..23], [blue; 21]);
            assert_eq!(report[23..44], [green; 21]);
            assert_eq!(report[44..65], [red; 21]);
        }
    }
}
