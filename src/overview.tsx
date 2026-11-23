import { useMemo, useState } from "react";
import { Activity, ArrowUpRight, BatteryCharging, Cpu, Droplets, Keyboard, Lightbulb, PlugZap, Search, Thermometer } from "lucide-react";
import type { HardwareController } from "./hardware";
import type { Translate } from "./i18n";
import type { GpuRuntime, Sensor, Telemetry, UsbCPort } from "./types";
import { Status, TemperatureChart } from "./ui";
import type { Sample } from "./ui";
import { useI18n } from "./i18n";

export type Hardware = ReturnType<HardwareController["getSnapshot"]>;
export type Page = "overview" | "sensors" | "keyboard" | "chassis" | "cooler" | "system" | "about";

const groups: Record<string, string> = {
  coretemp: "处理器", k10temp: "处理器", zenpower: "处理器", nvme: "固态硬盘", spd5118: "内存", acpitz: "主板温区",
  amdgpu: "显卡", nvidia: "显卡", nouveau: "显卡", iwlwifi_1: "无线网卡",
};

const batteryStatus: Record<string, string> = { Full: "已充满", Charging: "正在充电", Discharging: "使用电池", "Not charging": "未充电", Unknown: "状态未知" };
const gpuStatus: Record<GpuRuntime, string> = { active: "活动", suspended: "休眠", transition: "切换中", not_detected: "未检测到", unknown: "状态未知" };

export function cpuSensor(data: Telemetry | null) {
  const cpu = data?.temperatures.filter((sensor) => groups[sensor.chip] === "处理器") ?? [];
  for (const prefix of ["Package", "Tctl", "Tdie"]) {
    const matching = cpu.filter((sensor) => sensor.label?.startsWith(prefix));
    if (matching.length) return matching.reduce((warmest, sensor) => sensor.celsius > warmest.celsius ? sensor : warmest);
  }
  return null;
}

const measure = (value: number | null | undefined) => value == null ? "—" : Number(value.toFixed(1));
export const flowText = (meter: number | null) => meter === 1 || meter === 2 ? "水流正常" : meter === 0 ? "未检测到水流" : "等待水流回报";

function usbPowerText(port: UsbCPort, t: Translate) {
  if (!port.connected) return t("未连接");
  const state = port.input_online ? t("电源已接入") : port.power_role === "source" ? t("对外供电")
    : port.power_role === "sink" ? t("已连接 · 受电角色") : t("已连接");
  const mode = port.power_mode === "usb_power_delivery" ? "USB PD" : port.power_mode === "default" ? t("USB 默认") : port.power_mode;
  // 保留 3.25 A 等常见值的精度，电压与电流分别处理未上报的情况。
  const specs = [
    port.voltage_volts == null ? null : `${Number(port.voltage_volts.toFixed(2))} V`,
    port.current_amps == null ? null : `${Number(port.current_amps.toFixed(2))} A`,
  ].filter(Boolean).join(" / ");
  return [state, mode, port.input_online ? specs ? t("协商 {specs}", { specs }) : t("暂无协商读数") : null].filter(Boolean).join(" · ");
}

export function Overview({ data, load, samples, dischargeSamples, themeKey, hardware, navigate }: {
  data: Telemetry | null; load: number | null; samples: Sample[]; dischargeSamples: Sample[];
  themeKey: string; hardware: Hardware; navigate: (page: Page) => void;
}) {
  const { t } = useI18n();
  const cpu = cpuSensor(data);
  const battery = data?.batteries[0];
  const plugged = data?.power_sources.some(({ online }) => online) ?? false;
  const powerText = data?.power_sources.length ? (plugged ? t("外接电源") : t("电池供电")) : t("供电状态未知");
  const discharge = dischargeSamples.at(-1)?.value;
  const average = dischargeSamples.length > 1
    ? dischargeSamples.reduce((sum, sample) => sum + sample.value, 0) / dischargeSamples.length : null;
  const anyDischarging = data?.batteries.some(({ status }) => status === "Discharging");
  const hasBatteryStatus = battery?.status && battery.status !== "Unknown";
  const cooler = hardware.cooler;
  const flowOk = cooler.meter === 1 || cooler.meter === 2;
  const summaries = [
    { icon: Keyboard, page: "keyboard" as const, title: t("键盘灯光"), value: hardware.lighting.stage === "ready" ? t("亮度 {value} / 50", { value: hardware.lighting.actual?.brightness }) : t("打开灯控状态") },
    { icon: Lightbulb, page: "chassis" as const, title: t("机身灯光"), value: hardware.lighting.probe.zones.length ? t("{count} 个可用通道", { count: hardware.lighting.probe.zones.length }) : t("查看机身分区") },
    { icon: Droplets, page: "cooler" as const, title: t("外置水冷箱"), value: cooler.connected ? (flowOk ? t("已连接 · 水流正常") : t("已连接 · 查看水流")) : t("扫描和冷却设置") },
  ];
  return <div className="overview">
    <div className="metric-grid">
      <section className="panel metric metric--cpu">
        <div className="metric-heading"><span className="metric-icon"><Cpu size={18} /></span><h2>{t("CPU 封装温度")}</h2></div>
        <div className="reading">{measure(cpu?.celsius)}<small>°C</small></div>
        <div className="metric-detail"><span>{cpu?.chip ?? t("等待传感器")}</span><strong>{t("负载 {value}", { value: load === null ? null : `${load}%` })}</strong></div>
      </section>
      <section className="panel metric metric--battery">
        <div className="metric-heading"><span className="metric-icon"><BatteryCharging size={18} /></span><h2>{t("电池电量")}</h2></div>
        <div className="reading">{battery?.percent ?? "—"}<small>%</small></div>
        <div className="metric-detail"><span>{battery ? t(batteryStatus[battery.status ?? "Unknown"] ?? battery.status) : t("暂无电池数据")}</span>
          <strong>{discharge == null ? powerText : `${measure(discharge)} W · ${powerText}`}</strong></div>
      </section>
      <section className="panel metric metric--cooler">
        <div className="metric-heading"><span className="metric-icon"><Droplets size={18} /></span><h2>{t("外置水冷箱")}</h2></div>
        <div className="reading reading--text">{cooler.connected ? t(flowText(cooler.meter)) : t("未连接")}</div>
        <div className="metric-detail"><span>{cooler.connectedName || (cooler.automatic ? t("自动查找已启用") : t("监测已暂停"))}</span></div>
      </section>
    </div>

    <div className="overview-middle">
      <section className="panel thermal-panel">
        <div className="section-heading"><div><h2><Activity size={18} /> {t("CPU 温度趋势")}</h2><p>{t("最近 60 秒 · 封装传感器")}</p></div><Status tone={cpu ? "good" : "quiet"}>{cpu ? t("采集中") : t("等待数据")}</Status></div>
        <TemperatureChart samples={samples} themeKey={themeKey} />
      </section>
      <section className="panel shortcuts-panel">
        <div className="section-heading"><div><h2>{t("设备快捷入口")}</h2><p>{t("灯光与外置散热")}</p></div></div>
        {summaries.map(({ page, icon: Icon, title, value }) => <button className="shortcut" key={page} onClick={() => navigate(page)}>
          <span className="shortcut-icon"><Icon size={17} /></span><span><strong>{title}</strong><small>{value}</small></span><ArrowUpRight size={15} className="shortcut-arrow" />
        </button>)}
      </section>
    </div>

    <div className="overview-bottom">
      <section className="panel battery-detail">
        <div className="section-heading"><div><h2><PlugZap size={18} /> {t("电池与供电")}</h2><p>{battery?.name ?? "—"} · {powerText}</p></div>
          {battery?.percent != null && <meter min={0} max={100} value={battery.percent} aria-label={t("电池电量")} />}</div>
        <dl className="details-grid">
          <div><dt>{t("容量健康度")}</dt><dd>{measure(battery?.health_percent)}<small>%</small></dd></div>
          <div><dt>{t("充电循环")}</dt><dd>{measure(battery?.cycle_count)}<small>{t("次")}</small></dd></div>
          <div><dt>{t("充电上限")}</dt><dd className="small-dd">{battery?.charge_end_threshold == null ? battery?.charge_types || t("暂无接口") : `${battery.charge_end_threshold}%`}</dd></div>
        </dl>
        <dl className="hardware-rows battery-power">
          <div><dt>{t("电池放电功率")}</dt><dd>{discharge == null ? (hasBatteryStatus && !anyDischarging ? t("未放电") : t("暂无读数")) : `${measure(discharge)} W`}</dd></div>
          <div><dt>{t("近 3 分钟平均")}</dt><dd>{average == null ? "—" : `${measure(average)} W`}</dd></div>
        </dl>
        {!!data?.usb_c_ports.length && <div className="battery-power usb-power">
          <h3>{t("USB-C 供电")}</h3>
          <dl className="hardware-rows">
            {data.usb_c_ports.map((port) => <div key={port.name}><dt>{port.name}</dt><dd>{usbPowerText(port, t)}</dd></div>)}
          </dl>
        </div>}
      </section>
      <section className="panel hardware-detail">
        <div className="section-heading"><div><h2><Thermometer size={18} /> {t("硬件读数")}</h2><p>{t("Linux 内核传感器")}</p></div><button className="button text" onClick={() => navigate("sensors")}>{t("查看全部")} <ArrowUpRight size={14} /></button></div>
        <dl className="hardware-rows">
          <div><dt>{t("温度通道")}</dt><dd>{data?.temperatures.length ?? "—"}</dd></div>
          <div><dt>{t("风扇转速")}</dt><dd>{data?.fans.length ? data.fans.map((fan) => `${fan.label ?? fan.channel} ${fan.rpm} RPM`).join(" · ") : t("未公开 RPM")}</dd></div>
          <div><dt>{t("NVIDIA 独显")}</dt><dd>{data ? t(gpuStatus[data.nvidia_runtime]) : t("等待读取")}</dd></div>
        </dl>
      </section>
    </div>
  </div>;
}

export function Sensors({ telemetry }: { telemetry: Telemetry | null }) {
  const { t } = useI18n();
  const [search, setSearch] = useState("");
  const shown = useMemo(() => (telemetry?.temperatures ?? []).filter((sensor) => {
    const query = search.toLowerCase().trim();
    return !query || `${t(groups[sensor.chip])} ${groups[sensor.chip] ?? ""} ${sensor.chip} ${sensor.label ?? ""} ${sensor.device}/${sensor.channel}`.toLowerCase().includes(query);
  }), [search, telemetry, t]);
  return <section className="panel sensor-panel">
    <div className="section-heading"><div><h2>{t("温度读数")}</h2><p>{t("显示 {count} 个通道", { count: shown.length })}</p></div><label className="search"><Search size={16} /><input value={search} aria-label={t("查找传感器")} placeholder={t("查找传感器")} onChange={(event) => setSearch(event.currentTarget.value)} /></label></div>
    <div className="table-scroll"><table><thead><tr><th>{t("设备")}</th><th>{t("传感器")}</th><th>{t("内核通道")}</th><th>{t("温度")}</th></tr></thead><tbody>
      {shown.map((sensor: Sensor) => <tr key={`${sensor.device}/${sensor.channel}`}><th scope="row">{t(groups[sensor.chip] ?? sensor.chip)}</th><td>{sensor.label ?? sensor.channel}</td><td className="mono muted">{sensor.device}/{sensor.channel}</td><td className="mono temperature-cell">{measure(sensor.celsius)} °C</td></tr>)}
    </tbody></table></div>
    {!shown.length && <p className="empty">{search ? t("没有匹配的传感器") : t("暂无可读的温度数据")}</p>}
  </section>;
}
