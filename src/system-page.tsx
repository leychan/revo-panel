import { useCallback, useEffect, useState } from "react";
import type { ReactNode } from "react";
import { Gauge, Monitor, MousePointer2, RefreshCw, Zap } from "lucide-react";
import { bridge } from "./bridge";
import type { TurboState } from "./hardware";
import { SettingsGrid } from "./settings-grid";
import type { Gpu, Request, SystemSettings } from "./types";
import { Choices, Slider, Status } from "./ui";
import { useI18n } from "./i18n";

const profiles: Record<string, string> = { "power-saver": "省电", balanced: "均衡", performance: "性能" };

export function SystemPage({ appearancePanel, turbo, turboRestricted, setTurboPolicy }: {
  appearancePanel: ReactNode; turbo: TurboState; turboRestricted: boolean | null; setTurboPolicy: (enabled: boolean) => void;
}) {
  const { t } = useI18n();
  const [actual, setActual] = useState<SystemSettings | null>(null);
  const [selected, setSelected] = useState({ profile: "", brightness: 54, mode: "" });
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState("等待读取系统设置");
  const [gpus, setGpus] = useState<Gpu[] | null>(null);

  const run = useCallback(async (request: Request) => {
    setBusy(true);
    setFeedback(request.action === "read" || request.action === "gpu" ? "正在读取系统状态" : "正在应用设置");
    try {
      if (request.action === "gpu") {
        setGpus((await bridge.gpu()).gpus);
      } else {
        const current = await bridge.system(request);
        setActual(current);
        setSelected({ profile: current.power.active ?? "", brightness: current.display.brightness ?? 54,
          mode: current.display.current_mode?.id ?? "" });
      }
      setFeedback(request.action === "read" || request.action === "gpu" ? "当前状态已读取" : "设置已读回");
    } catch (error) {
      setFeedback(String(error));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => { void run({ action: "read" }); }, [run]);

  const power = actual?.power;
  const turboEnabled = turbo.actual?.enabled ?? turbo.preferred;
  const restricted = turboEnabled ? turbo.actual?.restricted ?? turboRestricted : turboRestricted;
  const display = actual?.display;
  const touchpad = actual?.touchpad;
  const size = display?.current_mode?.size;
  return <div className="page-stack">
    {appearancePanel}
    <div className="toolbar"><p role="status">{t(feedback)}</p><button className="button secondary" disabled={busy} onClick={() => void run({ action: "read" })}><RefreshCw size={15} /> {t("刷新设置")}</button></div>
    <SettingsGrid cards={{
      power: {
        title: t("系统电源档位"), icon: <Zap size={18} />,
        description: power?.active ? t("当前 · {profile}", { profile: t(profiles[power.active] ?? power.active) }) : t("等待 Linux 电源服务"),
        content: <>
          <Choices label="系统电源档位" options={power?.options?.map(({ name }) => ({ id: name, label: profiles[name] ?? name })) ?? []}
            value={selected.profile} disabled={busy} onChange={(profile) => setSelected({ ...selected, profile })} />
          <button className="button primary" disabled={busy || !selected.profile || selected.profile === power?.active}
            onClick={() => void run({ action: "power", value: selected.profile })}>{t("应用电源档位")}</button>
          {power?.error && <p className="context-note">{t(power.error)}</p>}
          <div className="turbo-setting">
            <label className="option-row">
              <span>{t("电池供电时限制 CPU 睿频")}</span>
              <input type="checkbox" role="switch" checked={turboEnabled}
                disabled={turbo.busy || (turboRestricted == null && !turboEnabled)}
                onChange={(event) => setTurboPolicy(event.currentTarget.checked)} />
            </label>
            <p className="context-note">{restricted == null ? t("当前驱动未提供睿频控制") : restricted ? t("当前睿频已限制") : t("当前睿频允许")}</p>
            {turbo.feedback && <p className="context-note" role="status">{t(turbo.feedback)}</p>}
          </div>
        </>,
      },
      display: {
        title: t("内置屏幕"), icon: <Monitor size={18} />,
        description: size ? `${size.width} × ${size.height} · ${display?.name}` : t("等待 KDE 显示接口"),
        content: <>
          <Slider label="屏幕亮度" min={1} max={100} value={selected.brightness} disabled={busy || display?.brightness == null || display.hdr}
            onChange={(brightness) => setSelected({ ...selected, brightness })} />
          <button className="button secondary" disabled={busy || display?.brightness == null || display.hdr || selected.brightness === display.brightness}
            onClick={() => void run({ action: "brightness", value: selected.brightness })}>{t("应用屏幕亮度")}</button>
          <div className="option-row"><span>{t("刷新率")}</span><Choices label="刷新率" options={display?.modes?.map((mode) => ({ id: mode.id, label: `${Math.round(mode.refreshRate * 100) / 100} Hz` })) ?? []}
            value={selected.mode} disabled={busy} compact onChange={(mode) => setSelected({ ...selected, mode })} /></div>
          <button className="button secondary" disabled={busy || !selected.mode || selected.mode === display?.current_mode?.id}
            onClick={() => void run({ action: "refresh_rate", value: selected.mode })}>{t("应用刷新率")}</button>
          {display?.error && <p className="context-note">{t(display.error)}</p>}
        </>,
      },
      gpu: {
        title: t("NVIDIA 独显"), icon: <Gauge size={18} />, description: t("按需读取驱动数据"),
        content: <>
          <button className="button secondary" disabled={busy} onClick={() => void run({ action: "gpu" })}>{t("读取独显")}</button>
          {gpus?.map((gpu) => <div className="gpu-data" key={gpu.name}><h3>{gpu.name}</h3><p>{gpu["temperature.gpu"]} °C · {t("负载 {value}", { value: `${gpu["utilization.gpu"]}%` })} · {gpu["power.draw"]} W</p><p>{t("显存")} {gpu["memory.used"]} / {gpu["memory.total"]} MiB</p></div>)}
          {gpus?.length === 0 && <p className="context-note">{t("未读到 NVIDIA 显卡")}</p>}
        </>,
      },
      touchpad: {
        title: t("内置触控板"), icon: <MousePointer2 size={18} />, description: t("KDE 输入设备"),
        status: <Status tone={touchpad?.enabled ? "good" : "quiet"}>{touchpad?.enabled === undefined ? t("接口不可用") : touchpad.enabled ? t("已启用") : t("已停用")}</Status>,
        content: <>
          <button className="button secondary" disabled={busy || touchpad?.enabled === undefined}
            onClick={() => void run({ action: "touchpad", enabled: !touchpad?.enabled })}>{touchpad?.enabled ? t("停用触控板") : t("启用触控板")}</button>
          {touchpad?.error && <p className="context-note">{t(touchpad.error)}</p>}
        </>,
      },
    }} />
  </div>;
}
