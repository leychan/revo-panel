import { Bluetooth, Droplets, Fan, Keyboard, Lightbulb, Power, RefreshCw, ScanLine, Waves, Zap } from "lucide-react";
import type { HardwareController } from "./hardware";
import { flowText } from "./overview";
import type { Hardware } from "./overview";
import type { Effect, Mode } from "./types";
import { Choices, ColorField, Slider, Status } from "./ui";
import { useI18n } from "./i18n";

const speeds = [{ id: 8, label: "慢速" }, { id: 5, label: "中速" }, { id: 2, label: "快速" }];
const directions = [{ id: 1, label: "左 → 右" }, { id: 2, label: "右 → 左" }, { id: 3, label: "下 → 上" }, { id: 4, label: "上 → 下" }];

function EffectOptions({ value, modes, disabled, keyboard, onChange }: {
  value: Effect; modes: Mode[]; disabled: boolean; keyboard?: boolean; onChange: (changes: Partial<Effect>) => void;
}) {
  const { t } = useI18n();
  const mode = modes.find(({ id }) => id === value.name);
  return <div className="effect-options">
    <Choices label="灯效模式" options={modes} value={value.name} disabled={disabled} onChange={(name) => onChange({ name })} />
    {value.name !== "off" && value.name !== "static" && <div className="option-row">
      <span>{t("动画速度")}</span><Choices label="动画速度" options={speeds} value={value.speed} disabled={disabled} onChange={(speed) => onChange({ speed })} compact />
    </div>}
    {keyboard && value.name === "wave" && <div className="option-row">
      <span>{t("流光方向")}</span><Choices label="流光方向" options={directions} value={value.direction} disabled={disabled} onChange={(direction) => onChange({ direction })} compact />
    </div>}
    {mode?.color && <div className="option-row"><span>{t("颜色")}</span><ColorField title="灯光颜色" value={value.color} disabled={disabled} onChange={(color) => onChange({ color })} /></div>}
  </div>;
}

export function KeyboardPage({ state, controller, supported }: { state: Hardware; controller: HardwareController; supported: boolean }) {
  const { t } = useI18n();
  const { lighting, catalog } = state;
  const ready = lighting.stage === "ready";
  const actual = lighting.actual;
  const activeName = !actual ? t("等待设备读数") : !actual.powered ? t("已关闭")
    : t(catalog?.keyboard.find(({ code }) => code === actual.mode)?.label) || t("固件模式 {mode}", { mode: actual.mode });
  return <div className="page-stack">
    <section className="panel device-header">
      <span className="device-emblem"><Keyboard size={25} /></span>
      <div><h2>{t("ITE 8291 键盘")}</h2><p>{t(lighting.feedback)}</p></div>
      <div className="header-actions">
        <Status tone={ready ? "good" : "quiet"}>{ready ? t("已连接") : lighting.stage === "authorizing" ? t("等待认证") : t("等待设备")}</Status>
        {ready ? <button className="button secondary" disabled={lighting.busy} onClick={controller.readLighting}><RefreshCw size={15} /> {t("读取灯光")}</button>
          : supported && <button className="button primary" disabled={lighting.stage !== "idle"} onClick={() => void controller.connectLighting()}>{t("连接灯控")}</button>}
      </div>
    </section>
    <div className="detail-grid">
      <section className="panel control-panel">
        <div className="section-heading"><div><h2><Lightbulb size={18} /> {t("键盘灯效")}</h2><p>{t("选择后自动发送")}</p></div></div>
        <EffectOptions value={lighting.keyboard} modes={catalog?.keyboard ?? []} disabled={!ready} keyboard onChange={controller.setKeyboard} />
      </section>
      <section className="panel control-panel keyboard-brightness">
        <div className="section-heading"><div><h2>{t("背光强度")}</h2><p>{t("设备读回")}</p></div></div>
        <div className="reading">{actual?.brightness ?? "—"}<small>/ {catalog?.keyboard_max ?? 50}</small></div>
        <Slider label="背光亮度" value={lighting.keyboard.brightness} max={catalog?.keyboard_max ?? 50} unit="" disabled={!ready}
          onChange={(brightness) => controller.setKeyboard({ brightness })} />
        <div className="inline-data"><span>{t("当前灯效")}</span><strong>{activeName}</strong></div>
      </section>
    </div>
  </div>;
}

export function ChassisPage({ state, controller }: { state: Hardware; controller: HardwareController }) {
  const { t } = useI18n();
  const { lighting, catalog } = state;
  const available = lighting.probe.zones;
  const zoneLabels = catalog?.zones ?? { front: "前灯条", logo: "A 面徽标", rear: "出风口灯" };
  const reason = lighting.probe.status === "confirmed" ? t("ITE 8233 能力码 {capability} 未报告徽标通道。主板徽标驱动尚未接入。", { capability: lighting.probe.capability })
    : lighting.probe.status === "unconfirmed" ? t("还未读到有效的徽标通道报告。") : t("等待灯控连接后检测徽标。");
  return <div className="page-stack">
    <section className="panel device-header"><span className="device-emblem"><Lightbulb size={25} /></span>
      <div><h2>{t("机身灯光分区")}</h2><p>{t("ITE 8233 · 与键盘共用灯控会话")}</p></div>
      <div className="header-actions"><Status tone={available.length ? "good" : "quiet"}>{available.length ? t("{count} 个可用通道", { count: available.length }) : t("等待通道")}</Status></div>
    </section>
    <div className="chassis-grid">
      {Object.entries(zoneLabels).filter(([zone]) => zone === "logo" || available.includes(zone)).map(([zone, name]) => {
        const mode = lighting.chassis[zone];
        const enabled = available.includes(zone) && lighting.stage === "ready";
        return <section className="panel control-panel" key={zone}>
          <div className="section-heading"><div><h2>{t(name)}</h2><p>{t(enabled ? lighting.zoneFeedback[zone] ?? "调整后自动发送" : "当前不可调节")}</p></div></div>
          {enabled ? <>
            <Slider label={t("{name}亮度", { name: t(name) })} value={mode.brightness} max={100} onChange={(brightness) => controller.setChassis(zone, { brightness })} />
            <EffectOptions value={mode} modes={catalog?.chassis ?? []} disabled={false} onChange={(changes) => controller.setChassis(zone, changes)} />
          </> : <p className="context-note">{reason}</p>}
        </section>;
      })}
    </div>
  </div>;
}

export function CoolerPage({ state, controller }: { state: Hardware; controller: HardwareController }) {
  const { t } = useI18n();
  const { cooler, catalog } = state;
  const idle = !cooler.pending;
  const flowOk = cooler.meter === 1 || cooler.meter === 2;
  const canPump = cooler.connected && cooler.meter !== null && cooler.pending !== "disconnect";
  const canFan = cooler.connected && flowOk && cooler.pending !== "disconnect";
  const devices = Object.entries(cooler.devices);
  const sameName = (name: string) => devices.filter(([, item]) => item.name === name).length > 1;
  return <div className="page-stack">
    <section className="panel device-header cooler-header">
      <span className="device-emblem"><Droplets size={26} /></span>
      <div><h2>{cooler.connectedName || t("外置水冷箱")}</h2><p>{t(cooler.feedback)}</p></div>
      <div className="header-actions">
        {!!devices.length && !cooler.connected && <select aria-label={t("水冷箱设备")} value={cooler.selected} disabled={!idle} onChange={(event) => controller.selectCooler(event.currentTarget.value)}>
          {devices.map(([path, device]) => <option key={path} value={path}>{device.name}{sameName(device.name) ? ` · ${path.slice(-8).replaceAll("_", ":")}` : ""}</option>)}
        </select>}
        {!cooler.connected && <button className="button secondary" disabled={!idle} onClick={() => void controller.findCooler(true)}><ScanLine size={16} /> {t("扫描")}</button>}
        <button className={`button ${cooler.connected ? "secondary" : "primary"}`} disabled={!idle} onClick={controller.toggleCooler}>
          {cooler.connected ? <><Power size={15} /> {t("停止监测")}</> : <><Bluetooth size={16} /> {cooler.selected ? t("连接设备") : t("查找并连接")}</>}
        </button>
      </div>
    </section>
    <div className="cooler-status-grid">
      <div className="panel inline-stat"><Waves size={20} /><div><span>{t("水流状态")}</span><strong>{cooler.connected ? t(flowText(cooler.meter)) : t("等待连接")}</strong></div></div>
      <div className="panel inline-stat"><Bluetooth size={20} /><div><span>{t("蓝牙监测")}</span><strong>{cooler.connected ? t("已连接") : cooler.automatic ? t("自动查找已启用") : t("已暂停")}</strong></div></div>
      <div className="panel inline-stat"><Zap size={20} /><div><span>{t("水冷箱固件")}</span><strong className="mono">{cooler.firmware || "—"}</strong></div></div>
    </div>
    <div className="detail-grid">
      <section className="panel control-panel">
        <div className="section-heading"><div><h2><Droplets size={18} /> {t("水泵档位")}</h2><p>{t(cooler.coolingSent || "更改档位或 PWM 后自动发送")}</p></div></div>
        <Choices label="水泵电压" options={catalog?.pump ?? []} value={cooler.pump} disabled={!canPump}
          onChange={(mode) => controller.setPump(mode, catalog?.pump.find(({ id }) => id === mode)?.duty ?? 45)} />
        <Slider label="水泵 PWM" value={cooler.pumpDuty} min={cooler.pump === "off" ? 0 : 1} max={100}
          disabled={!canPump || cooler.pump === "off"} onChange={(duty) => controller.setPump(cooler.pump, duty)} />
      </section>
      <section className="panel control-panel">
        <div className="section-heading"><div><h2><Fan size={18} /> {t("箱体风扇")}</h2><p>{t("水流正常时可调节")}</p></div><Status tone={flowOk ? "good" : "quiet"}>{flowOk ? t("水流正常") : t("等待水流")}</Status></div>
        <Slider label="风扇占空比" value={cooler.fanDuty} max={100} disabled={!canFan} onChange={controller.setFan} />
      </section>
    </div>
    <section className="panel control-panel">
      <div className="section-heading"><div><h2><Lightbulb size={18} /> {t("水冷箱灯光")}</h2><p>{t(cooler.lightsSent || "水箱和冷排分区调节")}</p></div></div>
      <div className="cooler-lights">
        {Object.entries(cooler.lights).map(([zone, light]) => {
          const title = zone === "tank" ? t("水箱灯") : t("冷排灯");
          const enabled = cooler.connected && cooler.pending !== "disconnect" && (zone === "tank" || cooler.connectedName.toUpperCase().includes("LCT22002"));
          const color = catalog?.cooler_lights.find(({ id }) => id === light.mode)?.color;
          return <div className="light-column" key={zone}><h3>{title}</h3>
            <Choices label={t("{title}模式", { title })} options={catalog?.cooler_lights ?? []} value={light.mode} disabled={!enabled} compact onChange={(mode) => controller.setCoolerLight(zone, { mode })} />
            {color && <ColorField title={t("{title}颜色", { title })} value={light.color} disabled={!enabled} onChange={(value) => controller.setCoolerLight(zone, { color: value })} />}
          </div>;
        })}
      </div>
    </section>
  </div>;
}
