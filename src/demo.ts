import type { Catalog, ChassisProbe, HardwareEvent, LightReading, Mode, Reply, Request, Service, SystemSettings, Telemetry } from "./types";

const modes = (rows: [string, string, number?, boolean?][]): Mode[] =>
  rows.map(([id, label, code, color]) => ({ id, label, code, color: color ?? false }));

const catalog: Catalog = {
  keyboard_max: 50,
  keyboard: modes([
    ["static", "单色常亮", 51, true], ["breathe", "单色呼吸", 2, true], ["wave", "波浪流光", 3],
    ["reactive", "按键微光", 4, true], ["rainbow", "彩虹循环", 5], ["ripple", "涟漪扩散", 6], ["off", "关闭", 0],
  ]),
  chassis: modes([["static", "单色常亮", 1, true], ["breathe", "单色呼吸", 2, true], ["wave", "全彩波浪", 3], ["off", "关闭", 255]]),
  zones: { front: "前灯条", logo: "A 面徽标", rear: "出风口灯" },
  pump: [
    { id: "7v", label: "7 V 静音", duty: 45 }, { id: "8v", label: "8 V 均衡", duty: 60 },
    { id: "11v", label: "11 V 性能", duty: 80 }, { id: "12v", label: "12 V 全速", duty: 100 }, { id: "off", label: "停泵", duty: 0 },
  ],
  cooler_lights: modes([["static", "单色常亮", 0, true], ["breathe", "单色呼吸", 1, true], ["rainbow", "彩色流光", 2], ["off", "关闭"]]),
};

const panelSize = { width: 2560, height: 1600 };
const displayModes = [{ id: "2", refreshRate: 60, size: panelSize }, { id: "1", refreshRate: 240, size: panelSize }];
const settings: SystemSettings = {
  power: { active: "balanced", options: ["power-saver", "balanced", "performance"].map((name) => ({ name })) },
  display: { name: "eDP-1", current_mode: displayModes[1], modes: displayModes, brightness: 54, hdr: false },
  touchpad: { enabled: true },
};

let readings = 0;
const light: LightReading = { brightness: 36, mode: 3, powered: true, speed: 7, direction: 2 };
const chassisProbe: ChassisProbe = { zones: ["front"], status: "confirmed", capability: 1 };
const devicePath = "/demo/lct22002";
const device = { name: "CoolingSystem LCT22002", connected: false, nearby: true, known_model: true };
let receive: ((event: HardwareEvent) => void) | undefined;

function emit(service: Service, data: Reply, delay = 45) {
  window.setTimeout(() => receive?.({ service, kind: "message", data }), delay);
}

// 浏览器预览提供独立数据，桌面窗口始终走本机 IPC。
export const previewHistory = [43, 45, 44, 46, 48, 49, 47, 46, 50, 52, 51, 49, 48, 50, 54, 52, 55, 58, 56, 55, 53, 51, 52, 54, 51, 50, 47, 46, 49, 50, 49];

export const demo = {
  catalog,
  async telemetry(): Promise<Telemetry> {
    const temp = previewHistory[30] + Math.round(Math.sin(readings / 2) * 3);
    readings += 1;
    return {
      kernel: "7.2.9-1-MANJARO",
      model: { sys_vendor: "MECHREVO", product_name: "YAOSHI Series", board_name: "YAOSHI Series-X6AR55xY", bios_version: "N.1.33MRO63" },
      temperatures: [
        { device: "hwmon8", chip: "coretemp", channel: "temp1", label: "Package id 0", celsius: temp },
        { device: "hwmon8", chip: "coretemp", channel: "temp2", label: "Core 0", celsius: temp - 4 },
        { device: "hwmon3", chip: "nvme", channel: "temp1", label: "Composite", celsius: 38.8 },
        { device: "hwmon4", chip: "nvme", channel: "temp1", label: "Composite", celsius: 32.6 },
        { device: "hwmon5", chip: "spd5118", channel: "temp1", label: null, celsius: 43.5 },
        { device: "hwmon1", chip: "acpitz", channel: "temp1", label: null, celsius: 46 },
      ],
      cpu_times: [readings * 1000, readings * 820],
      batteries: [{ name: "BAT0", percent: 86, status: "Discharging", health_percent: 100, cycle_count: 4, charge_end_threshold: null, charge_types: null,
        discharge_watts: 12 + Math.sin(readings / 2) }],
      power_sources: [{ name: "USB-C", online: true }],
      usb_c_ports: [
        { name: "port0", connected: true, power_role: "sink", input_online: true, power_mode: "usb_power_delivery", voltage_volts: 20, current_amps: 3.25 },
        { name: "port1", connected: false, power_role: "sink", input_online: false, power_mode: "default", voltage_volts: null, current_amps: null },
      ],
      cpu_turbo_restricted: false,
      nvidia_runtime: "suspended", fans: [], lighting_usb: [{ id: "048d:600b" }, { id: "048d:7001" }],
    };
  },
  async events(next: (event: HardwareEvent) => void) {
    receive = next;
    return () => { receive = undefined; };
  },
  async start(service: Service) {
    if (service === "lighting") emit(service, { state: { ...light }, chassis_probe: chassisProbe });
    if (service === "turbo") emit(service, { turbo: { enabled: true, restricted: false, power_source: "external" } });
  },
  async send(service: Service, request: Request) {
    if (service === "turbo") {
      emit(service, { turbo: { enabled: Boolean(request.enabled), restricted: false, power_source: "external" } });
      return;
    }
    if (service === "lighting") {
      if (request.action === "brightness") light.brightness = Number(request.level);
      if (request.action === "effect") {
        light.mode = catalog.keyboard.find((mode) => mode.id === request.name)?.code ?? 0;
        light.powered = light.mode !== 0;
        light.speed = Number(request.speed);
        light.direction = Number(request.direction);
      }
      if (request.action === "chassis") {
        const off = request.name === "off" || request.brightness === 0;
        emit(service, { chassis: { name: off ? "off" : String(request.name), brightness: off ? 0 : Number(request.brightness) } });
      } else {
        emit(service, { state: { ...light }, chassis_probe: request.action === "status" ? chassisProbe : undefined });
      }
      return;
    }
    if (request.action === "list" || request.action === "scan") {
      emit(service, { event: "devices", devices: { [devicePath]: { ...device } } }, request.action === "scan" ? 250 : 45);
    } else if (request.action === "connect") {
      device.connected = true;
      emit(service, { event: "connected", name: device.name, path: devicePath });
      emit(service, { event: "firmware", version: "2.0.0.4" }, 85);
      emit(service, { event: "water_flow", meter: 2 }, 100);
    } else if (request.action === "disconnect") {
      device.connected = false;
      emit(service, { event: "disconnected", reason: "已结束监测" });
    } else {
      emit(service, { event: "sent", component: request.action, value: String(request.mode ?? request.duty), duty: Number(request.duty), zone: String(request.zone) });
    }
  },
  async system(request: Request): Promise<SystemSettings> {
    if (request.action === "power") settings.power.active = String(request.value);
    if (request.action === "brightness") settings.display.brightness = Number(request.value);
    if (request.action === "refresh_rate") settings.display.current_mode = displayModes.find((mode) => mode.id === request.value);
    if (request.action === "touchpad") settings.touchpad.enabled = Boolean(request.enabled);
    return structuredClone(settings);
  },
  async gpu() {
    return { gpus: [{ name: "GeForce RTX 5070 Ti Laptop GPU", "temperature.gpu": "37", "utilization.gpu": "4", "memory.used": "280", "memory.total": "12227", "power.draw": "4.9" }] };
  },
};
