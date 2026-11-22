use crate::linux::{entries, name, text};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

fn number(path: impl AsRef<Path>) -> Option<i64> {
    text(path)?.parse().ok()
}

fn health(device: &Path) -> Option<f64> {
    // 电量与能量分别使用相同单位的一对额定值，不能交叉计算。
    for unit in ["charge", "energy"] {
        let full = number(device.join(format!("{unit}_full")));
        let design = number(device.join(format!("{unit}_full_design")));
        if let (Some(full), Some(design)) = (full, design) {
            if full > 0 && design > 0 {
                return Some((1000.0 * full as f64 / design as f64).round() / 10.0);
            }
        }
    }
    None
}

fn cpu_times(procfs: &Path) -> Option<[u64; 2]> {
    let stats = text(procfs.join("stat"))?;
    let mut row = stats.lines().next()?.split_whitespace();
    if row.next()? != "cpu" {
        return None;
    }
    let mut fields = [0u64; 8];
    for value in &mut fields {
        *value = row.next()?.parse().ok()?;
    }
    // guest 已包含在 user/nice 中，闲置时间包含 idle 和 iowait。
    Some([fields.iter().sum(), fields[3] + fields[4]])
}

#[derive(Clone, Copy, Ord, PartialOrd, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuRuntime {
    NotDetected,
    Suspended,
    Unknown,
    Transition,
    Active,
}

pub fn nvidia_runtime_status() -> GpuRuntime {
    nvidia_runtime(Path::new("/sys")).0
}

fn nvidia_runtime(sysfs: &Path) -> (GpuRuntime, Vec<PathBuf>) {
    let mut status = GpuRuntime::NotDetected;
    let mut devices = Vec::new();
    for device in entries(sysfs.join("bus/pci/devices")) {
        if text(device.join("vendor")).as_deref() != Some("0x10de")
            || !text(device.join("class")).is_some_and(|class| class.starts_with("0x03"))
        {
            continue;
        }
        // 只读取内核已记录的 PCI 运行状态，无需打开独显设备或调用 nvidia-smi。
        let current = match text(device.join("power/runtime_status")).as_deref() {
            Some("active") => GpuRuntime::Active,
            Some("suspended") => GpuRuntime::Suspended,
            Some("resuming" | "suspending") => GpuRuntime::Transition,
            _ => GpuRuntime::Unknown,
        };
        status = status.max(current);
        if let Ok(path) = device.canonicalize() {
            devices.push(path);
        }
    }
    (status, devices)
}

fn readings(classes: &Path, nvidia_devices: &[PathBuf]) -> (Vec<Value>, Vec<Value>) {
    let mut temperatures = Vec::new();
    let mut fans = Vec::new();
    for device in entries(classes.join("hwmon")) {
        let device_name = name(&device);
        if !device_name.starts_with("hwmon") {
            continue;
        }
        // 独显可能在状态读取后立刻休眠，后台跳过其传感器。详细指标由用户按需读取。
        if !nvidia_devices.is_empty()
            && device
                .canonicalize()
                .is_ok_and(|path| nvidia_devices.iter().any(|gpu| path.starts_with(gpu)))
        {
            continue;
        }
        let chip = text(device.join("name")).unwrap_or_else(|| device_name.to_owned());
        if matches!(chip.as_str(), "nvidia" | "nouveau") {
            continue;
        }
        for path in entries(&device) {
            let Some(channel) = name(&path).strip_suffix("_input") else {
                continue;
            };
            let Some(index) = channel
                .strip_prefix("temp")
                .or_else(|| channel.strip_prefix("fan"))
            else {
                continue;
            };
            if index.is_empty()
                || index.starts_with('0')
                || !index.bytes().all(|value| value.is_ascii_digit())
            {
                continue;
            }
            let Some(value) = number(&path) else { continue };
            let mut reading = json!({"device": device_name, "chip": chip, "channel": channel,
                "label": text(device.join(format!("{channel}_label")))});
            if channel.starts_with("temp") {
                reading["celsius"] = json!(value as f64 / 1000.0);
                temperatures.push(reading);
            } else {
                reading["rpm"] = json!(value);
                fans.push(reading);
            }
        }
    }
    (temperatures, fans)
}

fn discharge_watts(device: &Path) -> Option<f64> {
    if let Some(power) = number(device.join("power_now")) {
        return Some((power as f64).abs() / 1_000_000.0);
    }
    // 有些耀世只提供电流和电压。微安乘微伏后按 10^12 换算为瓦。
    let current = number(device.join("current_now"))?;
    let voltage = number(device.join("voltage_now")).filter(|&voltage| voltage > 0)?;
    Some((current as f64).abs() * voltage as f64 / 1_000_000_000_000.0)
}

fn supplies(classes: &Path) -> (Vec<Value>, Vec<Value>) {
    let mut batteries = Vec::new();
    let mut sources = Vec::new();
    for device in entries(classes.join("power_supply")) {
        let kind = text(device.join("type"));
        if kind.as_deref() == Some("Battery") && number(device.join("present")) != Some(0) {
            let status = text(device.join("status"));
            let watts = if status.as_deref() == Some("Discharging") {
                discharge_watts(&device)
            } else {
                None
            };
            batteries.push(
                json!({"name": name(&device), "percent": number(device.join("capacity")),
                "status": status, "health_percent": health(&device), "discharge_watts": watts,
                "cycle_count": number(device.join("cycle_count")),
                "charge_end_threshold": number(device.join("charge_control_end_threshold")),
                "charge_types": text(device.join("charge_types"))}),
            );
        }
        if let Some(online) = number(device.join("online")) {
            sources.push(json!({"name": name(&device), "type": kind, "online": online > 0}));
        }
    }
    (batteries, sources)
}

fn usb_c_ports(classes: &Path) -> Vec<Value> {
    let typec = classes.join("typec");
    let mut ports: Vec<_> = entries(&typec)
        .into_iter()
        .filter_map(|port| {
            let index = name(&port).strip_prefix("port")?.parse::<usize>().ok()?;
            let controller = port.join("device").canonicalize().ok();
            Some((index, port, controller))
        })
        .collect();
    ports.sort_unstable_by_key(|(index, _, _)| *index);
    if ports.is_empty() {
        return Vec::new();
    }

    let mut supplies: BTreeMap<PathBuf, BTreeMap<usize, PathBuf>> = BTreeMap::new();
    for supply in entries(classes.join("power_supply")) {
        if !name(&supply).starts_with("ucsi-source-psy-") {
            continue;
        }
        let Ok(controller) = supply.join("device").canonicalize() else {
            continue;
        };
        let prefix = format!("ucsi-source-psy-{}", name(&controller));
        if let Some(number) = name(&supply)
            .strip_prefix(&prefix)
            .and_then(|value| value.parse().ok())
        {
            supplies
                .entry(controller)
                .or_default()
                .insert(number, supply);
        }
    }

    let mut port_supplies = BTreeMap::new();
    for (controller, sources) in supplies {
        let siblings: Vec<_> = ports
            .iter()
            .filter(|(_, _, parent)| parent.as_ref() == Some(&controller))
            .collect();
        // UCSI 按控制器顺序注册端口，供电节点后缀从 1 开始。缺少对应节点时省略协商值。
        // Type-C 的 portN 是全局编号，不能直接当作控制器的端口编号。
        if siblings.len() == sources.len() && sources.keys().copied().eq(1..=siblings.len()) {
            for ((_, port, _), supply) in siblings.into_iter().zip(sources.into_values()) {
                port_supplies.insert(port.clone(), supply);
            }
        }
    }

    ports.into_iter().map(|(_, port, _)| {
        let port_name = name(&port);
        let supply = port_supplies.get(&port);
        let online = supply.and_then(|path| number(path.join("online"))).map(|value| value > 0);
        let connected = online == Some(true) || typec.join(format!("{port_name}-partner")).is_dir()
            || port.join(format!("{port_name}-partner")).is_dir();
        let role = text(port.join("power_role"));
        let role = role.as_deref().and_then(|value| match value {
            "sink" | "source" => Some(value),
            _ => value.split_whitespace().find_map(|word| word.strip_prefix('[')?.strip_suffix(']')),
        });
        // UCSI 报告的电压、电流来自供电规格。仅在输入在线时读取，忽略未上报的零值。
        let negotiated = |property| {
            supply.filter(|_| online == Some(true))
                .and_then(|path| number(path.join(property)))
                .filter(|&value| value > 0)
                .map(|value| value as f64 / 1_000_000.0)
        };
        json!({"name": port_name, "connected": connected, "power_role": role, "input_online": online,
            "power_mode": text(port.join("power_operation_mode")),
            "voltage_volts": negotiated("voltage_now"), "current_amps": negotiated("current_now")})
    }).collect()
}

fn profiles(sysfs: &Path, classes: &Path) -> Vec<Value> {
    let choices = |path| {
        text(path)
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let mut profiles: Vec<_> = entries(classes.join("platform-profile"))
        .iter()
        .filter_map(|device| {
            let current = text(device.join("profile"))?;
            Some(
                json!({"name": text(device.join("name")).unwrap_or_else(|| name(device).to_owned()),
            "current": current, "choices": choices(device.join("choices"))}),
            )
        })
        .collect();
    let acpi = sysfs.join("firmware/acpi");
    if profiles.is_empty() {
        if let Some(current) = text(acpi.join("platform_profile")) {
            profiles.push(json!({"name": "ACPI", "current": current,
                "choices": choices(acpi.join("platform_profile_choices"))}));
        }
    }
    profiles
}

pub fn snapshot() -> Value {
    read_snapshot(Path::new("/sys"), Path::new("/proc"))
}

fn read_snapshot(sysfs: &Path, procfs: &Path) -> Value {
    let classes = sysfs.join("class");
    let dmi = classes.join("dmi/id");
    let model = ["sys_vendor", "product_name", "board_name", "bios_version"]
        .into_iter()
        .map(|field| (field, text(dmi.join(field))))
        .collect::<std::collections::BTreeMap<_, _>>();
    let (gpu_runtime, nvidia_devices) = nvidia_runtime(sysfs);
    let (temperatures, fans) = readings(&classes, &nvidia_devices);
    let (batteries, power_sources) = supplies(&classes);
    let lighting_usb: Vec<_> = entries(sysfs.join("bus/usb/devices"))
        .iter()
        .filter_map(|device| {
            let vendor = text(device.join("idVendor"))?;
            let product = text(device.join("idProduct"))?;
            let id = format!("{vendor}:{product}").to_ascii_lowercase();
            let chip = match id.as_str() {
                "048d:600b" => "ITE 8291",
                "048d:7001" => "ITE 8233",
                _ => return None,
            };
            Some(json!({"port": name(device), "id": id, "chip": chip}))
        })
        .collect();
    let leds = entries(classes.join("leds"));
    let keyboard_backlights: Vec<_> = leds
        .iter()
        .filter(|device| {
            name(device).contains(":kbd_backlight") && device.join("brightness").is_file()
        })
        .map(|device| name(device))
        .collect();
    json!({"kernel": text(procfs.join("sys/kernel/osrelease")).unwrap_or_default(), "model": model,
        "cpu_times": cpu_times(procfs), "temperatures": temperatures, "fans": fans, "nvidia_runtime": gpu_runtime,
        "batteries": batteries, "power_sources": power_sources, "usb_c_ports": usb_c_ports(&classes),
        "platform_profiles": profiles(sysfs, &classes), "cpu_turbo_restricted": crate::turbo::restricted(sysfs),
        "keyboard_backlights": keyboard_backlights, "lighting_usb": lighting_usb})
}
