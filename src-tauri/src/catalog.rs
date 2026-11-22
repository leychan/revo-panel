use serde::Serialize;
use serde_json::{json, Value};

pub const BRIGHTNESS_MAX: u8 = 50;
pub const USER_MODE: u8 = 0x33;

#[derive(Serialize)]
pub struct Effect {
    pub id: &'static str,
    label: &'static str,
    pub code: u8,
    pub color: bool,
}

const fn effect(id: &'static str, label: &'static str, code: u8, color: bool) -> Effect {
    Effect {
        id,
        label,
        code,
        color,
    }
}

pub const KEYBOARD_EFFECTS: &[Effect] = &[
    effect("static", "单色常亮", USER_MODE, true),
    effect("breathe", "单色呼吸", 0x02, true),
    effect("wave", "波浪流光", 0x03, false),
    effect("reactive", "按键微光", 0x04, true),
    effect("rainbow", "彩虹循环", 0x05, false),
    effect("ripple", "涟漪扩散", 0x06, false),
    effect("marquee", "全彩跑马", 0x09, false),
    effect("rain", "随机雨滴", 0x0a, false),
    effect("aurora", "极光", 0x0e, false),
    effect("spark", "烟花", 0x11, false),
    effect("off", "关闭", 0, false),
];

pub const CHASSIS_EFFECTS: &[Effect] = &[
    effect("static", "单色常亮", 1, true),
    effect("breathe", "单色呼吸", 2, true),
    effect("rainbow-breathe", "彩虹呼吸", 2, false),
    effect("wave", "全彩波浪", 3, false),
    effect("collision", "动态对撞", 4, false),
    effect("meteor", "流星", 6, true),
    effect("off", "关闭", 255, false),
];

pub struct Zone {
    pub id: &'static str,
    label: &'static str,
    pub selector: u8,
    pub palette: u8,
    pub capabilities: &'static [u8],
}

// 0xA2 能力码使用 OpenRevo 的分区定义，前灯条的调色槽采用 Linux 驱动映射。
pub const ZONES: &[Zone] = &[
    Zone {
        id: "front",
        label: "前灯条",
        selector: 0x22,
        palette: 0,
        capabilities: &[1, 3, 4, 5, 7, 9, 11],
    },
    Zone {
        id: "logo",
        label: "A 面徽标",
        selector: 0x23,
        palette: 3,
        capabilities: &[5, 8, 9, 10, 11],
    },
    Zone {
        id: "rear",
        label: "出风口灯",
        selector: 0x21,
        palette: 1,
        capabilities: &[0, 2, 3, 4, 6, 7, 10, 11],
    },
];

#[derive(Serialize)]
pub struct Pump {
    pub id: &'static str,
    label: &'static str,
    pub duty: u8,
    #[serde(skip)]
    pub voltage: Option<u8>,
}

pub const PUMPS: &[Pump] = &[
    Pump {
        id: "7v",
        label: "7 V 静音",
        duty: 45,
        voltage: Some(0x02),
    },
    Pump {
        id: "8v",
        label: "8 V 均衡",
        duty: 60,
        voltage: Some(0x03),
    },
    Pump {
        id: "11v",
        label: "11 V 性能",
        duty: 80,
        voltage: Some(0x00),
    },
    Pump {
        id: "12v",
        label: "12 V 全速",
        duty: 100,
        voltage: Some(0x01),
    },
    Pump {
        id: "off",
        label: "停泵",
        duty: 0,
        voltage: None,
    },
];

#[derive(Serialize)]
pub struct CoolerLight {
    pub id: &'static str,
    label: &'static str,
    color: bool,
    #[serde(skip)]
    pub code: Option<u8>,
}

pub const COOLER_LIGHTS: &[CoolerLight] = &[
    CoolerLight {
        id: "static",
        label: "单色常亮",
        color: true,
        code: Some(0),
    },
    CoolerLight {
        id: "breathe",
        label: "单色呼吸",
        color: true,
        code: Some(1),
    },
    CoolerLight {
        id: "rainbow",
        label: "彩色流光",
        color: false,
        code: Some(2),
    },
    CoolerLight {
        id: "rainbow-breathe",
        label: "彩色呼吸",
        color: false,
        code: Some(3),
    },
    CoolerLight {
        id: "off",
        label: "关闭",
        color: false,
        code: None,
    },
];

pub fn snapshot() -> Value {
    let zones = ZONES
        .iter()
        .map(|zone| (zone.id, zone.label))
        .collect::<std::collections::BTreeMap<_, _>>();
    json!({"keyboard_max": BRIGHTNESS_MAX, "keyboard": KEYBOARD_EFFECTS,
        "chassis": CHASSIS_EFFECTS, "zones": zones, "pump": PUMPS, "cooler_lights": COOLER_LIGHTS})
}
