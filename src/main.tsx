import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { createRoot } from "react-dom/client";
import { Activity, Droplets, Gauge, Keyboard, Lightbulb, Settings2, Thermometer } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AppearanceSettings, useAppearance } from "./appearance";
import { bridge, native } from "./bridge";
import { previewHistory } from "./demo";
import { ChassisPage, CoolerPage, KeyboardPage } from "./device-pages";
import { HardwareController } from "./hardware";
import { cpuSensor, Overview, Sensors } from "./overview";
import type { Page } from "./overview";
import { SystemPage } from "./system-page";
import type { Telemetry } from "./types";
import { Status } from "./ui";
import type { Sample } from "./ui";
import "./styles.css";

const pages = {
  overview: { title: "硬件总览", label: "总览", icon: Gauge },
  sensors: { title: "温度传感器", label: "温度传感器", icon: Thermometer },
  keyboard: { title: "键盘灯光", label: "键盘灯光", icon: Keyboard },
  chassis: { title: "机身灯光", label: "机身灯光", icon: Lightbulb },
  cooler: { title: "外置水冷箱", label: "水冷箱", icon: Droplets },
  system: { title: "系统设置", label: "系统设置", icon: Settings2 },
};
const nav = ["overview", "sensors", "keyboard", "chassis", "cooler", "system"] as const;
const hardware = new HardwareController();

function App() {
  const { appearance, setAppearance, themeKey } = useAppearance();
  const [page, setPage] = useState<Page>("overview");
  const [telemetry, setTelemetry] = useState<Telemetry | null>(null);
  const [load, setLoad] = useState<number | null>(null);
  const [samples, setSamples] = useState<Sample[]>(() => native ? [] : previewHistory.map((value, index) => ({ time: Date.now() - (30 - index) * 2000, value })));
  const [dischargeSamples, setDischargeSamples] = useState<Sample[]>([]);
  const [updated, setUpdated] = useState(0);
  const [error, setError] = useState("");
  const state = useSyncExternalStore(hardware.subscribe, hardware.getSnapshot);
  const pane = useRef<HTMLElement>(null);

  useEffect(() => {
    hardware.start();
    let stopped = false;
    let reading = false;
    let previous: Telemetry["cpu_times"] = null;
    let previousPlugged: boolean | undefined;
    async function poll() {
      if (reading || stopped) return;
      reading = true;
      try {
        const result = await bridge.telemetry();
        if (stopped) return;
        const now = Date.now();
        const current = result.cpu_times;
        if (previous && current && current[0] > previous[0]) {
          const total = current[0] - previous[0];
          setLoad(Math.round(100 * Math.min(total, Math.max(0, total - current[1] + previous[1])) / total));
        } else setLoad(null);
        previous = current;
        const cpu = cpuSensor(result);
        if (cpu) setSamples((history) => [...history.filter((sample) => now - sample.time < 60_000), { time: now, value: cpu.celsius }]);
        else setSamples([]);
        const readings = result.batteries.filter(({ status }) => status === "Discharging")
          .map(({ discharge_watts }) => discharge_watts);
        const watts = readings.length && readings.every((value) => value != null)
          ? readings.reduce((sum, value) => sum + value, 0) : null;
        const plugged = result.power_sources.some(({ online }) => online);
        const sameSupply = plugged === previousPlugged;
        previousPlugged = plugged;
        // 切换供电来源时重新取样，避免把插电和拔电的数据混在一起。
        setDischargeSamples((history) => watts == null ? [] : [
          ...(sameSupply ? history.filter((sample) => now - sample.time < 180_000) : []),
          { time: now, value: watts },
        ]);
        setUpdated(now);
        setTelemetry(result);
        setError("");
        hardware.discoverLighting(result);
      } catch (failure) {
        if (!stopped) {
          setDischargeSamples([]);
          setError(String(failure));
        }
      } finally {
        reading = false;
      }
    }
    void poll();
    const timer = window.setInterval(() => void poll(), 2000);
    return () => { stopped = true; window.clearInterval(timer); };
  }, []);

  useEffect(() => {
    if (!native) return;
    let closing = false;
    const unlisten = listen("desktop-exit", async () => {
      if (closing) return;
      closing = true;
      await Promise.allSettled([hardware.close(), bridge.finishSystem()]);
      await invoke("finish_exit");
    });
    void unlisten.then(() => invoke("desktop_ready")).catch(console.error);
    return () => { void unlisten.then((stop) => stop()).catch(console.error); };
  }, []);

  function navigate(next: Page) {
    setPage(next);
    pane.current?.scrollTo({ top: 0 });
  }

  const chassis = telemetry?.model.board_name ?? "等待机型信息";
  const model = telemetry?.model.product_name ?? "本机设备";
  const lightingSupported = telemetry?.model.board_name === "YAOSHI Series-X6AR55xY" && !!telemetry?.lighting_usb.some(({ id }) => id === "048d:600b");
  return <div className="shell">
    <aside className="sidebar">
      <div className="brand"><span className="brand-mark"><Activity size={22} strokeWidth={2.3} /></span><div><strong>RevoPanel</strong><small>Linux 控制台</small></div></div>
      <nav aria-label="控制台导航">
        {nav.map((key) => {
          const { icon: Icon, label } = pages[key];
          return <button className="nav-item" key={key} aria-label={label} aria-current={page === key ? "page" : undefined} onClick={() => navigate(key)}>
            <Icon size={18} /><span>{label}</span>
          </button>;
        })}
      </nav>
      <div className="sidebar-device"><span className="live-indicator" /><div><strong>{model}</strong><span>{telemetry ? "本机设备" : "正在检测"}</span></div></div>
    </aside>
    <main className="workspace" ref={pane}>
      <header className="topbar"><span className="machine-path"><b>{telemetry?.model.sys_vendor ?? "Linux"}</b><i>/</i>{chassis}</span>
        <Status tone={error ? "warn" : native && updated ? "good" : "quiet"}>{native ? error ? "读取异常" : updated ? "实时监测" : "正在启动" : "界面预览"}</Status>
      </header>
      <div className="page-body">
        <div className="page-title"><h1>{pages[page].title}</h1>{page === "overview" && <span className="update-time">更新于 <b>{updated ? new Date(updated).toLocaleTimeString("zh-CN", { hour12: false }) : "—"}</b></span>}</div>
        {error && <p className="panel error" role="status">读取硬件状态失败 · {error}</p>}
        {page === "overview" && <Overview data={telemetry} load={load} samples={samples} dischargeSamples={dischargeSamples}
          themeKey={themeKey} hardware={state} navigate={navigate} />}
        {page === "sensors" && <Sensors telemetry={telemetry} />}
        {page === "keyboard" && <KeyboardPage state={state} controller={hardware} supported={lightingSupported} />}
        {page === "chassis" && <ChassisPage state={state} controller={hardware} />}
        {page === "cooler" && <CoolerPage state={state} controller={hardware} />}
        {page === "system" && <SystemPage appearancePanel={<AppearanceSettings appearance={appearance} change={setAppearance} />}
          turbo={state.turbo} turboRestricted={telemetry?.cpu_turbo_restricted ?? null}
          setTurboPolicy={hardware.setTurboPolicy} />}
        <footer className="system-footer"><span>Linux {telemetry?.kernel ?? "—"}</span><span>BIOS {telemetry?.model.bios_version ?? "—"}</span></footer>
      </div>
    </main>
  </div>;
}

createRoot(document.getElementById("root")!).render(<App />);
