use crate::{
    catalog::{COOLER_LIGHTS, PUMPS},
    linux::{emit, request_line, rgb},
};
use dbus::{
    arg::{AppendAll, PropMap, ReadAll, Variant},
    blocking::{
        stdintf::org_freedesktop_dbus::{ObjectManager, PropertiesPropertiesChanged},
        Connection,
    },
    channel::Token,
    message::MatchRule,
    Path,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    io,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

const BLUEZ: &str = "org.bluez";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const SERVICE: &str = "org.bluez.GattService1";
const CHARACTERISTIC: &str = "org.bluez.GattCharacteristic1";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const NUS: &str = "6e400001-b5a3-f393-e0a9-e50e24dcca9e";
const TX: &str = "6e400002-b5a3-f393-e0a9-e50e24dcca9e";
const RX: &str = "6e400003-b5a3-f393-e0a9-e50e24dcca9e";
const FLOW_CHANGED: u8 = 0x31;
const FLOW_QUERY: u8 = 0x32;
const SET_PUMP: u8 = 0x1c;
const SET_FAN: u8 = 0x1b;
const TANK_LED: u8 = 0x1e;
const RADIATOR_LED: u8 = 0x33;
const CALL_TIMEOUT: Duration = Duration::from_secs(4);
const WRITE_TIMEOUT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_secs(3);
const FLOW_EXPIRY: Duration = Duration::from_secs(8);
type Objects = HashMap<Path<'static>, HashMap<String, PropMap>>;
type Packet = [u8; 8];

#[derive(Serialize)]
struct Device {
    name: String,
    connected: bool,
    nearby: bool,
    known_model: bool,
}

#[derive(Default)]
struct Link {
    path: String,
    owns_device: bool,
    tx: Option<String>,
    rx: Option<String>,
    watches: Vec<Token>,
    meter: Option<(u8, Instant)>,
    next_query: Option<Instant>,
    radiator: bool,
}

enum Notice {
    Data(Vec<u8>),
    Disconnected,
}

struct Cooler {
    connection: Connection,
    devices: BTreeMap<String, Device>,
    link: Link,
    notify_tx: Sender<Notice>,
    notify_rx: Receiver<Notice>,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Request {
    List,
    Scan,
    Connect {
        path: String,
    },
    Disconnect,
    Pump {
        mode: String,
        duty: Option<u8>,
    },
    Fan {
        duty: u8,
    },
    Lighting {
        mode: String,
        color: String,
        zone: Option<String>,
    },
}

fn packet(command: u8, params: &[u8]) -> Packet {
    // UART 控制请求固定八字节，起止字节为 FE / EF。
    let mut frame = [0; 8];
    frame[0] = 0xfe;
    frame[1] = command;
    frame[2..2 + params.len()].copy_from_slice(params);
    frame[7] = 0xef;
    frame
}

fn flow(payload: &[u8]) -> Option<u8> {
    // 本机 LCT22002 回五字节，协议也允许八字节变体。
    if matches!(payload.len(), 5 | 8)
        && payload[0] == 0xfe
        && payload.last() == Some(&0xef)
        && matches!(payload[1], FLOW_CHANGED | FLOW_QUERY)
    {
        Some(payload[3])
    } else {
        None
    }
}

fn pump_packet(mode: &str, duty: Option<u8>) -> Result<(Packet, u8), String> {
    let pump = PUMPS
        .iter()
        .find(|pump| pump.id == mode)
        .ok_or("不支持该水泵档位")?;
    let duty = duty.unwrap_or(pump.duty);
    // 电压码 0 表示 11 V，停泵由独立使能位决定。
    let params = match pump.voltage {
        Some(code) if (1..=100).contains(&duty) => [1, duty, code],
        None if duty == 0 => [0; 3],
        _ => return Err("请选择有效的水泵占空比".into()),
    };
    Ok((packet(SET_PUMP, &params), duty))
}

fn property<'a>(props: &'a PropMap, key: &str) -> Option<&'a str> {
    props.get(key)?.0.as_str()
}

fn has_flag(props: &PropMap, flag: &str) -> bool {
    props
        .get("Flags")
        .and_then(|value| value.0.as_iter())
        .is_some_and(|mut flags| flags.any(|value| value.as_str() == Some(flag)))
}

fn boolean(props: &PropMap, key: &str) -> bool {
    props.get(key).and_then(|value| value.0.as_i64()) == Some(1)
}

impl Cooler {
    fn new() -> Result<Self, String> {
        let connection =
            Connection::new_system().map_err(|error| format!("BlueZ 不可用：{error}"))?;
        let (notify_tx, notify_rx) = mpsc::channel();
        Ok(Self {
            connection,
            devices: BTreeMap::new(),
            link: Link::default(),
            notify_tx,
            notify_rx,
        })
    }

    fn call<R: ReadAll, A: AppendAll>(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        args: A,
        wait: Duration,
    ) -> Result<R, String> {
        self.connection
            .with_proxy(BLUEZ, path, wait)
            .method_call(interface, method, args)
            .map_err(|error| error.to_string())
    }

    fn objects(&self) -> Result<Objects, String> {
        self.connection
            .with_proxy(BLUEZ, "/", CALL_TIMEOUT)
            .get_managed_objects()
            .map_err(|error| error.to_string())
    }

    fn list(&mut self, scan: bool) -> Result<Value, String> {
        let mut objects = self.objects()?;
        if scan {
            let adapter = objects
                .iter()
                .filter(|(_, data)| {
                    data.get(ADAPTER)
                        .is_some_and(|props| boolean(props, "Powered"))
                })
                .map(|(path, _)| path.to_string())
                .min()
                .ok_or("未发现已启用的蓝牙适配器")?;
            self.call::<(), _>(&adapter, ADAPTER, "StartDiscovery", (), CALL_TIMEOUT)?;
            thread::sleep(Duration::from_secs(5));
            let result = self.objects();
            let stopped = self.call::<(), _>(&adapter, ADAPTER, "StopDiscovery", (), CALL_TIMEOUT);
            objects = result?;
            stopped?;
        }
        self.devices = objects
            .into_iter()
            .filter_map(|(path, interfaces)| {
                let device = interfaces.get(DEVICE)?;
                let name = property(device, "Name")?;
                let upper = name.to_ascii_uppercase();
                if !["COOLING", "LCT", "CS_"]
                    .iter()
                    .any(|prefix| upper.starts_with(prefix))
                {
                    return None;
                }
                // BlueZ 会保留历史设备，只有已连接或扫描中收到广播的设备用于自动连接。
                Some((
                    path.to_string(),
                    Device {
                        name: name.into(),
                        connected: boolean(device, "Connected"),
                        nearby: scan && device.contains_key("RSSI"),
                        known_model: upper.contains("LCT21001") || upper.contains("LCT22002"),
                    },
                ))
            })
            .collect();
        Ok(json!({"event": "devices", "devices": self.devices}))
    }

    fn watch(&mut self, path: &str, characteristic: bool) -> Result<(), String> {
        let path = Path::new(path.to_owned()).map_err(|error| error.to_string())?;
        let rule = MatchRule::new_signal(PROPERTIES, "PropertiesChanged")
            .with_sender(BLUEZ)
            .with_path(path);
        let notices = self.notify_tx.clone();
        let token = self
            .connection
            .add_match(rule, move |signal: PropertiesPropertiesChanged, _, _| {
                if characteristic && signal.interface_name == CHARACTERISTIC {
                    if let Some(value) = signal.changed_properties.get("Value") {
                        if let Some(payload) = value.0.as_iter() {
                            let bytes: Option<Vec<_>> = payload
                                .map(|byte| u8::try_from(byte.as_u64()?).ok())
                                .collect();
                            if let Some(bytes) = bytes {
                                let _ = notices.send(Notice::Data(bytes));
                            }
                        }
                    }
                } else if !characteristic
                    && signal.interface_name == DEVICE
                    && signal.changed_properties.contains_key("Connected")
                    && !boolean(&signal.changed_properties, "Connected")
                {
                    let _ = notices.send(Notice::Disconnected);
                }
                true
            })
            .map_err(|error| error.to_string())?;
        self.link.watches.push(token);
        Ok(())
    }

    fn connect_channels(&mut self, path: &str) -> Result<(), String> {
        let address = Path::new(path.to_owned()).map_err(|error| error.to_string())?;
        let mut objects = self.objects()?;
        let device = objects
            .get(&address)
            .and_then(|data| data.get(DEVICE))
            .ok_or("水冷箱已离线，请重新扫描")?;
        if !boolean(device, "Connected") {
            self.call::<(), _>(path, DEVICE, "Connect", (), Duration::from_secs(9))?;
            self.link.owns_device = true;
        }
        for _ in 0..16 {
            objects = self.objects()?;
            if objects
                .get(&address)
                .and_then(|data| data.get(DEVICE))
                .is_some_and(|device| boolean(device, "ServicesResolved"))
            {
                break;
            }
            thread::sleep(Duration::from_millis(250));
        }
        let uart = objects
            .iter()
            .find(|(_, data)| {
                data.get(SERVICE).is_some_and(|props| {
                    property(props, "UUID") == Some(NUS) && property(props, "Device") == Some(path)
                })
            })
            .map(|(path, _)| path.to_string())
            .ok_or("设备没有匹配的水冷箱 UART 服务")?;
        let characteristic = |uuid, flag| {
            objects
                .iter()
                .find(|(_, data)| {
                    data.get(CHARACTERISTIC).is_some_and(|props| {
                        property(props, "Service") == Some(uart.as_str())
                            && property(props, "UUID") == Some(uuid)
                            && has_flag(props, flag)
                    })
                })
                .map(|(path, _)| path.to_string())
                .ok_or("设备没有匹配的水冷箱写入和通知通道")
        };
        let tx = characteristic(TX, "write-without-response")?;
        let rx = characteristic(RX, "notify")?;
        self.watch(&rx, true)?;
        self.watch(path, false)?;
        self.call::<(), _>(&rx, CHARACTERISTIC, "StartNotify", (), CALL_TIMEOUT)?;
        self.link.tx = Some(tx);
        self.link.rx = Some(rx);
        // 建立监测会话时只有握手和水流查询，不改动水泵、风扇或灯光。
        self.write(b"sw")?;
        thread::sleep(Duration::from_millis(350));
        self.query_flow()?;
        Ok(())
    }

    fn connect(&mut self, path: &str) -> Result<Value, String> {
        let name = self
            .devices
            .get(path)
            .ok_or("请先扫描并选择水冷箱")?
            .name
            .clone();
        self.disconnect();
        self.link.path = path.into();
        self.link.radiator = name.to_ascii_uppercase().contains("LCT22002");
        if let Err(error) = self.connect_channels(path) {
            self.disconnect();
            return Err(error);
        }
        Ok(json!({"event": "connected", "name": name, "path": path}))
    }

    fn disconnect(&mut self) {
        let old = std::mem::take(&mut self.link);
        for token in old.watches {
            let _ = self.connection.remove_match(token);
        }
        if let Some(rx) = old.rx {
            let _ = self.call::<(), _>(&rx, CHARACTERISTIC, "StopNotify", (), WRITE_TIMEOUT);
        }
        // 只断开由这个会话主动建立的蓝牙连接，保留外部连接。
        if old.owns_device {
            let _ = self.call::<(), _>(&old.path, DEVICE, "Disconnect", (), WRITE_TIMEOUT);
        }
        while self.notify_rx.try_recv().is_ok() {}
    }

    fn write(&self, bytes: &[u8]) -> Result<(), String> {
        let tx = self.link.tx.as_deref().ok_or("尚未连接水冷箱")?;
        let mut options = PropMap::new();
        options.insert("type".into(), Variant(Box::new("command".to_owned())));
        self.call(
            tx,
            CHARACTERISTIC,
            "WriteValue",
            (bytes.to_vec(), options),
            WRITE_TIMEOUT,
        )
    }

    fn query_flow(&mut self) -> Result<(), String> {
        self.write(&packet(FLOW_QUERY, &[]))?;
        self.link.next_query = Some(Instant::now() + POLL_INTERVAL);
        Ok(())
    }

    fn latest_flow(&self) -> Option<u8> {
        self.link
            .meter
            .filter(|(_, timestamp)| timestamp.elapsed() <= FLOW_EXPIRY)
            .map(|(meter, _)| meter)
    }

    fn set_lighting(&self, mode: &str, zone: &str, color: &str) -> Result<(), String> {
        let channels: &[u8] = match (zone, self.link.radiator) {
            ("all", true) => &[TANK_LED, RADIATOR_LED],
            ("all" | "tank", _) => &[TANK_LED],
            ("radiator", true) => &[RADIATOR_LED],
            _ => return Err("当前水冷箱未提供所选灯光通道".into()),
        };
        let mode = COOLER_LIGHTS
            .iter()
            .find(|effect| effect.id == mode)
            .ok_or("水冷箱灯光参数无效")?;
        let [red, green, blue] = rgb(color)?;
        let params = match mode.code {
            Some(code) if red != 0 || green != 0 || blue != 0 => [1, red, green, blue, code],
            None => [0; 5],
            _ => return Err("请选择非黑色，或使用关闭灯光".into()),
        };
        for (index, channel) in channels.iter().copied().enumerate() {
            if index != 0 {
                thread::sleep(Duration::from_millis(50));
            }
            self.write(&packet(channel, &params))?;
        }
        Ok(())
    }

    fn apply(&mut self, request: Request) -> Result<Value, String> {
        match request {
            Request::List => self.list(false),
            Request::Scan => self.list(true),
            Request::Connect { path } => self.connect(&path),
            Request::Disconnect => {
                self.disconnect();
                Ok(json!({"event": "disconnected", "reason": "已结束监测"}))
            }
            Request::Pump { mode, duty } => {
                let (frame, duty) = pump_packet(&mode, duty)?;
                if self.latest_flow().is_none() {
                    return Err("请先等待读到水流状态".into());
                }
                self.write(&frame)?;
                Ok(json!({"event": "sent", "component": "pump", "value": mode, "duty": duty}))
            }
            Request::Fan { duty } => {
                if duty > 100 {
                    return Err("风扇占空比应为 0–100 的整数".into());
                }
                if !matches!(self.latest_flow(), Some(1 | 2)) {
                    return Err("水流正常时可调节箱体风扇".into());
                }
                self.write(&packet(SET_FAN, &[u8::from(duty > 0), duty]))?;
                Ok(json!({"event": "sent", "component": "fan", "value": duty}))
            }
            Request::Lighting { mode, color, zone } => {
                let zone = zone.as_deref().unwrap_or("all");
                self.set_lighting(&mode, zone, &color)?;
                Ok(json!({"event": "sent", "component": "lighting", "value": mode, "zone": zone}))
            }
        }
    }

    fn process_notices(&mut self) -> io::Result<()> {
        while let Ok(notice) = self.notify_rx.try_recv() {
            match notice {
                Notice::Disconnected => {
                    self.disconnect();
                    emit(&json!({"event": "disconnected", "reason": "蓝牙连接已断开"}))?;
                }
                Notice::Data(payload) => {
                    if let Some(meter) = flow(&payload) {
                        self.link.meter = Some((meter, Instant::now()));
                        emit(&json!({"event": "water_flow", "meter": meter}))?;
                    } else if let Some(version) = payload.strip_prefix(b"MCU F/W Version: ") {
                        emit(
                            &json!({"event": "firmware", "version": String::from_utf8_lossy(version).trim()}),
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    fn poll(&mut self) -> io::Result<()> {
        if self.link.meter.is_some() && self.latest_flow().is_none() {
            self.link.meter = None;
            emit(&json!({"event": "water_flow", "meter": null}))?;
        }
        if self
            .link
            .next_query
            .is_some_and(|due| due <= Instant::now())
        {
            if let Err(error) = self.query_flow() {
                self.disconnect();
                emit(&json!({"event": "disconnected", "reason": error}))?;
            }
        }
        Ok(())
    }
}

impl Drop for Cooler {
    fn drop(&mut self) {
        self.disconnect();
    }
}

pub fn run(list_only: bool) -> Result<(), String> {
    let mut cooler = Cooler::new()?;
    if list_only {
        return emit(&cooler.list(false)?).map_err(|error| error.to_string());
    }

    let (send, receive) = mpsc::sync_channel(16);
    // stdin 与 BlueZ 通知独立到达。独立读取管道，D-Bus 连接留在会话线程。
    thread::spawn(move || {
        let mut input = io::stdin().lock();
        loop {
            match request_line(&mut input) {
                Ok(Some(line)) => {
                    let request = serde_json::from_slice(&line).map_err(|error| error.to_string());
                    if send.send(request).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = send.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    loop {
        match receive.try_recv() {
            Ok(request) => {
                let reply = request
                    .and_then(|request| cooler.apply(request))
                    .unwrap_or_else(|error| json!({"event": "error", "message": error}));
                emit(&reply).map_err(|error| error.to_string())?;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(()),
        }
        cooler
            .connection
            .process(Duration::from_millis(80))
            .map_err(|error| error.to_string())?;
        cooler
            .process_notices()
            .map_err(|error| error.to_string())?;
        cooler.poll().map_err(|error| error.to_string())?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lct_voltage_and_meter_packets_keep_wire_format() {
        assert_eq!(
            pump_packet("11v", None).unwrap().0,
            [0xfe, 0x1c, 1, 80, 0, 0, 0, 0xef]
        );
        assert_eq!(
            pump_packet("off", None).unwrap().0,
            [0xfe, 0x1c, 0, 0, 0, 0, 0, 0xef]
        );
        assert!(pump_packet("7v", Some(0)).is_err());
        assert_eq!(flow(&[0xfe, 0x32, 0, 2, 0xef]), Some(2));
        assert_eq!(flow(&[0xfe, 0x31, 0, 1, 0, 0, 0, 0xef]), Some(1));
        assert_eq!(flow(&[0xfe, 0x32, 0, 2, 0]), None);
    }
}
