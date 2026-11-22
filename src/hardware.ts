import { bridge } from "./bridge";
import { message } from "./i18n";
import type { Localized } from "./i18n";
import type { Catalog, ChassisProbe, CoolerDevice, Effect, HardwareEvent, LightReading, Reply, Request, Service, Telemetry, TurboStatus } from "./types";

class CommandQueue {
  active: Request | null = null;
  private staged = new Map<string, Request>();
  private timer = 0;

  constructor(private transport: (request: Request) => void) {}

  get idle() { return !this.active && this.staged.size === 0; }
  contains(key: string) { return this.staged.has(key); }

  schedule(key: string, request: Request) {
    this.staged.set(key, request);
    window.clearTimeout(this.timer);
    this.timer = window.setTimeout(() => this.flush(), 180);
  }

  send(request: Request) {
    if (!this.active) {
      this.active = request;
      this.transport(request);
    }
  }

  drain() {
    if (this.active || this.timer) return;
    const next = this.staged.entries().next().value;
    if (!next) return;
    this.staged.delete(next[0]);
    this.send(next[1]);
  }

  finish() { this.active = null; }

  flush() {
    window.clearTimeout(this.timer);
    this.timer = 0;
    this.drain();
  }

  discard() {
    window.clearTimeout(this.timer);
    this.timer = 0;
    this.staged.clear();
  }

  clear() { this.discard(); this.finish(); }
}

const effect = (brightness: number): Effect => ({ brightness, color: "#32D4FF", direction: 1, speed: 5, name: "static" });
const turboPreferenceKey = "revo-panel.turbo-on-battery.v1";
const controlAuthorization = "请在系统窗口中授权本次运行的灯控和睿频控制";

function savedTurboPolicy() {
  try { return localStorage.getItem(turboPreferenceKey) === "true"; }
  catch { return false; }
}

interface Lighting {
  stage: "idle" | "starting" | "authorizing" | "ready";
  feedback: Localized;
  busy: boolean;
  actual: LightReading | null;
  keyboard: Effect;
  probe: ChassisProbe;
  chassis: Record<string, Effect>;
  zoneFeedback: Record<string, Localized>;
}

interface Cooler {
  connected: boolean;
  automatic: boolean;
  pending: string | null;
  feedback: Localized;
  devices: Record<string, CoolerDevice>;
  selected: string;
  connectedName: string;
  firmware: string;
  meter: number | null;
  pump: string;
  pumpDuty: number;
  fanDuty: number;
  lights: Record<string, { mode: string; color: string }>;
  coolingSent: Localized;
  lightsSent: Localized;
}

export interface TurboState {
  stage: "idle" | "starting" | "authorizing" | "ready";
  busy: boolean;
  actual: TurboStatus | null;
  preferred: boolean;
  feedback: string;
}

type State = { catalog: Catalog | null; lighting: Lighting; cooler: Cooler; turbo: TurboState };

export class HardwareController {
  private listeners = new Set<() => void>();
  private listening: Promise<void> | null = null;
  private cancelListener?: () => void;
  private closing = false;
  private lightingAttempted = false;
  private turboRestoreAttempted = false;
  private lightingSupported = false;
  private preferredCooler = "";
  private retryTimer = 0;
  private lightQueue = new CommandQueue((request) => this.send("lighting", request));
  private coolerQueue = new CommandQueue((request) => this.send("cooler", request));
  private state: State = {
    catalog: null,
    turbo: { stage: "idle", busy: false, actual: null, preferred: savedTurboPolicy(), feedback: "" },
    lighting: {
      stage: "idle", feedback: "正在识别灯控设备", busy: false, actual: null,
      keyboard: effect(25), probe: { zones: [], status: "pending" },
      chassis: { front: effect(60), logo: effect(60), rear: effect(60) }, zoneFeedback: {},
    },
    cooler: {
      connected: false, automatic: true, pending: null, feedback: "正在查找水冷箱", devices: {}, selected: "",
      connectedName: "", firmware: "", meter: null, pump: "7v", pumpDuty: 45, fanDuty: 60,
      lights: { tank: { mode: "static", color: "#32D4FF" }, radiator: { mode: "static", color: "#32D4FF" } },
      coolingSent: "", lightsSent: "",
    },
  };

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => { this.listeners.delete(listener); };
  };

  getSnapshot = () => this.state;

  private publish(patch: Partial<State>) {
    this.state = { ...this.state, ...patch };
    this.listeners.forEach((listener) => listener());
  }

  private lighting(patch: Partial<Lighting>) { this.publish({ lighting: { ...this.state.lighting, ...patch } }); }
  private cooler(patch: Partial<Cooler>) { this.publish({ cooler: { ...this.state.cooler, ...patch } }); }
  private turbo(patch: Partial<TurboState>) { this.publish({ turbo: { ...this.state.turbo, ...patch } }); }

  private saveTurboPolicy(enabled: boolean) {
    try { localStorage.setItem(turboPreferenceKey, String(enabled)); } catch { /* 本次运行仍可使用。 */ }
  }

  setTurboPolicy = async (enabled: boolean) => {
    const { stage, busy } = this.state.turbo;
    if (this.closing || busy || (stage !== "ready" && stage !== "idle")) return;
    if (stage === "idle" && !enabled) {
      this.saveTurboPolicy(false);
      this.turbo({ preferred: false, feedback: "自动限制已关闭" });
      return;
    }
    try {
      if (stage === "ready") {
        this.turbo({ busy: true, feedback: "正在更新睿频限制" });
        await bridge.send("turbo", { action: "turbo_policy", enabled });
      } else {
        this.turbo({ stage: "starting", busy: true, feedback: "正在检查睿频控制权限" });
        await this.listening;
        if (this.closing) this.turbo({ stage: "idle", busy: false });
        else await bridge.start("turbo");
      }
    } catch (error) {
      this.turbo({ ...(stage === "idle" ? { stage } : {}), busy: false, feedback: String(error) });
    }
  };

  start() {
    if (!this.listening) {
      // 先订阅再启动助手，避免丢失开机时立即送出的状态。
      this.listening = bridge.events((event) => this.receive(event)).then((cancel) => {
        this.cancelListener = cancel;
        if (!this.closing) void this.findCooler();
      }).catch((error: unknown) => {
        this.lighting({ feedback: String(error) });
        this.cooler({ feedback: String(error) });
      });
      void bridge.catalog().then((catalog) => {
        if (!this.closing) {
          this.publish({ catalog });
          const actual = this.state.lighting.actual;
          if (actual && this.lightQueue.idle) this.syncLight(actual);
        }
      }).catch((error: unknown) => this.lighting({ feedback: String(error) }));
    }
  }

  discoverControls(telemetry: Telemetry) {
    if (this.closing) return;
    if (!this.turboRestoreAttempted && telemetry.cpu_turbo_restricted != null) {
      this.turboRestoreAttempted = true;
      if (this.state.turbo.preferred) void this.setTurboPolicy(true);
    }
    if (this.lightingAttempted) return;
    const { model, lighting_usb } = telemetry;
    this.lightingSupported = model.sys_vendor === "MECHREVO" && model.board_name === "YAOSHI Series-X6AR55xY"
      && lighting_usb.some(({ id }) => id === "048d:600b");
    if (this.lightingSupported) {
      this.lightingAttempted = true;
      void this.connectLighting();
    } else {
      this.lighting({ feedback: "当前未识别到受支持的 ITE 键盘灯控" });
    }
  }

  connectLighting = async () => {
    if (!this.lightingSupported || this.closing || this.state.lighting.stage !== "idle") return;
    this.lighting({ stage: "starting", feedback: "正在连接 ITE 灯控" });
    try {
      await this.listening;
      if (!this.closing) await bridge.start("lighting");
    } catch (error) {
      this.lighting({ stage: "idle", feedback: String(error) });
    }
  };

  readLighting = () => {
    if (this.state.lighting.stage === "ready" && this.lightQueue.idle) this.lightQueue.send({ action: "status" });
  };

  setKeyboard = (changes: Partial<Effect>) => {
    if (this.state.lighting.stage !== "ready" || this.closing) return;
    const keyboard = { ...this.state.lighting.keyboard, ...changes };
    this.lighting({ keyboard, feedback: "等待发送设置" });
    if (changes.brightness !== undefined) {
      this.lightQueue.schedule("brightness", { action: "brightness", level: keyboard.brightness });
    } else {
      const { name, speed, direction, color } = keyboard;
      this.lightQueue.schedule("effect", { action: "effect", name, speed, direction, color });
    }
  };

  setChassis = (zone: string, changes: Partial<Effect>) => {
    const { stage, chassis, probe, zoneFeedback } = this.state.lighting;
    if (stage !== "ready" || !probe.zones.includes(zone) || this.closing) return;
    const next = { ...chassis[zone], ...changes };
    this.lighting({ chassis: { ...chassis, [zone]: next }, zoneFeedback: { ...zoneFeedback, [zone]: "等待发送" } });
    this.lightQueue.schedule(zone, { action: "chassis", zone, ...next });
  };

  selectCooler = (path: string) => this.cooler({ selected: path });

  findCooler = async (scan = false) => {
    if (this.closing || this.state.cooler.pending || this.state.cooler.connected) return;
    const action = scan ? "scan" : "list";
    this.cooler({ pending: action, feedback: scan ? "正在扫描附近的水冷箱" : "正在查找水冷箱" });
    try {
      await bridge.start("cooler");
      if (!this.closing) this.coolerQueue.send({ action });
      else this.cooler({ pending: null });
    } catch (error) {
      this.cooler({ pending: null });
      this.retry(String(error));
    }
  };

  toggleCooler = () => {
    if (this.closing || this.state.cooler.pending) return;
    const { connected, selected } = this.state.cooler;
    if (connected) {
      this.pauseCooler();
      this.coolerQueue.discard();
      this.coolerQueue.send({ action: "disconnect" });
    } else {
      this.cooler({ automatic: true });
      if (selected) this.coolerQueue.send({ action: "connect", path: selected });
      else void this.findCooler();
    }
  };

  private pauseCooler() {
    window.clearTimeout(this.retryTimer);
    this.cooler({ automatic: false });
  }

  private retry(detail: string, delay = 30_000) {
    window.clearTimeout(this.retryTimer);
    const retry = this.state.cooler.automatic && !this.state.cooler.connected && !this.closing;
    this.cooler({ feedback: retry ? message("{message} · 将自动重试", { message: message(detail) }) : detail });
    if (retry) this.retryTimer = window.setTimeout(() => void this.findCooler(), delay);
  }

  setPump = (mode: string, duty: number) => {
    if (!this.state.cooler.connected || this.state.cooler.meter === null || this.closing) return;
    this.cooler({ pump: mode, pumpDuty: duty });
    this.coolerQueue.schedule("pump", { action: "pump", mode, duty });
  };

  setFan = (duty: number) => {
    const { connected, meter } = this.state.cooler;
    if (!connected || (meter !== 1 && meter !== 2) || this.closing) return;
    this.cooler({ fanDuty: duty });
    this.coolerQueue.schedule("fan", { action: "fan", duty });
  };

  setCoolerLight = (zone: string, changes: { mode?: string; color?: string }) => {
    const { connected, connectedName, lights } = this.state.cooler;
    if (!connected || this.closing || (zone === "radiator" && !connectedName.toUpperCase().includes("LCT22002"))) return;
    const next = { ...lights[zone], ...changes };
    this.cooler({ lights: { ...lights, [zone]: next } });
    this.coolerQueue.schedule(zone, { action: "lighting", zone, ...next });
  };

  private send(service: Exclude<Service, "turbo">, request: Request) {
    if (service === "lighting") this.lighting({ busy: true });
    else {
      window.clearTimeout(this.retryTimer);
      const feedback = { list: "正在查找水冷箱", scan: "正在扫描附近的水冷箱", connect: "正在连接水冷箱" }[request.action];
      this.cooler({ pending: request.action, feedback: feedback ?? "正在发送水冷箱请求" });
    }
    void bridge.send(service, request).catch((error: unknown) => {
      const queue = service === "lighting" ? this.lightQueue : this.coolerQueue;
      if (queue.active !== request) return;
      if (service === "lighting") this.lightReply({ error: String(error) });
      else this.coolerReply({ event: "error", message: String(error) });
    });
  }

  private receive({ service, kind, data }: HardwareEvent) {
    if (service === "turbo") {
      this.turboReply(kind, data);
      return;
    }
    if (kind === "message") {
      if (service === "lighting") this.lightReply(data);
      else this.coolerReply(data);
      return;
    }
    if (service === "cooler") {
      this.resetCooler();
      this.cooler({ devices: {}, selected: "" });
      this.retry(data.detail || "水冷箱助手已退出");
    } else if (data.code === 77 && this.state.lighting.stage === "starting" && !this.closing) {
      this.lighting({ stage: "authorizing", feedback: controlAuthorization });
      void bridge.start("lighting", true).catch((error: unknown) => this.lighting({ stage: "idle", feedback: String(error) }));
    } else {
      this.lightQueue.clear();
      this.lighting({ stage: "idle", busy: false, probe: { zones: [], status: "disconnected" },
        feedback: data.code === 126 ? "已取消灯控授权" : data.detail || "灯控连接已断开" });
    }
  }

  private turboReply(kind: HardwareEvent["kind"], data: Reply) {
    if (kind === "message") {
      if (!data.turbo) return;
      const { enabled, restricted, power_source } = data.turbo;
      const feedback = !enabled ? "自动限制已关闭" : power_source === "external" ? "外接电源，使用系统睿频设置"
        : power_source === "battery" ? (restricted ? "电池供电，睿频已限制" : "电池供电，当前由系统控制睿频")
        : "供电来源未知，使用系统睿频设置";
      if (enabled !== this.state.turbo.preferred) this.saveTurboPolicy(enabled);
      this.turbo({ stage: "ready", busy: false, actual: data.turbo, preferred: enabled, feedback });
    } else if (data.code === 77 && this.state.turbo.stage === "starting" && !this.closing) {
      this.turbo({ stage: "authorizing", feedback: controlAuthorization });
      void bridge.start("turbo", true).catch((error: unknown) =>
        this.turbo({ stage: "idle", busy: false, feedback: String(error) }));
    } else {
      this.turbo({ stage: "idle", busy: false, actual: null,
        feedback: data.code === 126 ? "已取消睿频授权" : data.detail || "睿频控制会话已结束" });
    }
  }

  private syncLight(actual: LightReading) {
    const known = actual.powered ? this.state.catalog?.keyboard.find(({ code }) => code === actual.mode)?.id : "off";
    this.lighting({ keyboard: { ...this.state.lighting.keyboard, brightness: actual.brightness,
      name: known ?? this.state.lighting.keyboard.name } });
  }

  private lightReply(reply: Reply) {
    const request = this.lightQueue.active;
    const initial = this.state.lighting.stage !== "ready";
    this.lightQueue.finish();
    if (request?.action === "chassis") {
      const actual = reply.chassis;
      const mode = this.state.catalog?.chassis.find(({ id }) => id === actual?.name)?.label;
      const feedback = reply.error || (actual ? message("最近发送 · {mode} · {brightness}%",
        { mode: message(mode ?? actual.name), brightness: actual.brightness }) : "机身灯控未回复");
      this.lighting({ zoneFeedback: { ...this.state.lighting.zoneFeedback, [String(request.zone)]: feedback }, busy: false });
    } else if (reply.state) {
      this.lighting({ stage: "ready", actual: reply.state, busy: false,
        probe: reply.chassis_probe ?? this.state.lighting.probe,
        feedback: initial ? "已连接，改动自动发送" : "设备已读回设置" });
      if (initial || (request?.action === "status" && this.lightQueue.idle)) this.syncLight(reply.state);
      if (request?.action === "effect" && !this.lightQueue.contains("brightness")) {
        this.lighting({ keyboard: { ...this.state.lighting.keyboard, brightness: reply.state.brightness } });
      }
    } else {
      this.lighting({ busy: false, feedback: reply.error ?? "灯控助手未返回状态" });
    }
    this.lightQueue.drain();
  }

  private resetCooler() {
    this.coolerQueue.clear();
    this.cooler({ connected: false, connectedName: "", firmware: "", meter: null, pending: null, coolingSent: "", lightsSent: "" });
  }

  private coolerReply(reply: Reply) {
    if (reply.event === "water_flow") {
      this.cooler({ meter: reply.meter ?? null });
      return;
    }
    if (reply.event === "firmware") {
      this.cooler({ firmware: reply.version ?? "" });
      return;
    }
    const action = this.coolerQueue.active?.action;
    this.coolerQueue.finish();
    this.cooler({ pending: null });
    if (reply.event === "devices") {
      const devices = reply.devices ?? {};
      const old = this.state.cooler.selected;
      const selected = old in devices ? old : Object.keys(devices)[0] ?? "";
      this.cooler({ devices, selected, feedback: selected ? message("已发现 {count} 台水冷箱", { count: Object.keys(devices).length }) : "附近未发现水冷箱" });
      if (this.state.cooler.automatic && !this.closing) this.connectDiscovered(devices, action === "scan");
    } else if (reply.event === "connected") {
      this.preferredCooler = reply.path ?? this.state.cooler.selected;
      this.cooler({ connected: true, connectedName: reply.name ?? "", selected: this.preferredCooler,
        feedback: message("正在监测 {device}", { device: reply.name ?? message("水冷箱") }) });
    } else if (reply.event === "disconnected") {
      this.resetCooler();
      this.retry(reply.reason ?? "蓝牙连接已断开", 1000);
    } else if (reply.event === "sent") {
      const { catalog } = this.state;
      if (reply.component === "pump") {
        const label = catalog?.pump.find(({ id }) => id === reply.value)?.label;
        this.cooler({ coolingSent: message("已发送水泵 {mode} · {duty}%", {
          mode: label ? message(label) : reply.value, duty: reply.duty }) });
      } else if (reply.component === "fan") {
        this.cooler({ coolingSent: message("已发送箱体风扇 {duty}%", { duty: reply.value }) });
      } else if (reply.component === "lighting") {
        const label = catalog?.cooler_lights.find(({ id }) => id === reply.value)?.label;
        this.cooler({ lightsSent: message("已发送{zone} · {mode}", {
          zone: message(reply.zone === "tank" ? "水箱灯" : "冷排灯"), mode: label ? message(label) : reply.value }) });
      }
      this.cooler({ feedback: "设备已接收请求" });
    } else {
      const error = reply.message ?? reply.error ?? "水冷箱操作失败";
      if (action === "list" || action === "scan" || action === "connect") this.retry(error);
      else this.cooler({ feedback: error });
    }
    if (this.state.cooler.connected) this.coolerQueue.drain();
  }

  private connectDiscovered(devices: Record<string, CoolerDevice>, scanned: boolean) {
    const candidates = Object.entries(devices).filter(([, item]) => item.known_model && (item.connected || (scanned && item.nearby)));
    const selected = this.preferredCooler || (candidates.length === 1 ? candidates[0][0] : "");
    if (candidates.some(([path]) => path === selected)) {
      this.cooler({ selected });
      this.coolerQueue.send({ action: "connect", path: selected });
    } else if (!scanned) {
      void this.findCooler(true);
    } else if (!this.preferredCooler && candidates.length > 1) {
      this.cooler({ feedback: "发现多台水冷箱，请选择设备" });
    } else {
      this.retry(this.preferredCooler ? "等待上次连接的水冷箱" : "附近未发现水冷箱");
    }
  }

  async close() {
    if (this.closing) return;
    this.closing = true;
    this.pauseCooler();
    this.lightQueue.flush();
    this.coolerQueue.flush();
    // 关闭窗口前交付已经排队的设置。蓝牙搜索等操作有自己的超时。
    const deadline = Date.now() + 12_000;
    while ((!this.lightQueue.idle || !this.coolerQueue.idle) && Date.now() < deadline) {
      await new Promise((resolve) => window.setTimeout(resolve, 80));
    }
    await Promise.allSettled([bridge.stop()]);
    const restoreDeadline = Date.now() + 3000;
    while (this.state.turbo.stage !== "idle" && Date.now() < restoreDeadline) {
      await new Promise((resolve) => window.setTimeout(resolve, 80));
    }
    this.cancelListener?.();
  }
}
