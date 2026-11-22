import { useEffect, useLayoutEffect, useState } from "react";
import { Palette } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { native } from "./bridge";
import { Choices, ColorField } from "./ui";
import { useI18n } from "./i18n";

type Mode = "dark" | "light" | "system";
interface Appearance { mode: Mode; accent: string }
type Rgb = [number, number, number];

// 沿用原来的存储键，升级后可以直接读取已保存的外观设置。
const storageKey = "easy-openrevo.appearance.v1";
const fallback: Appearance = { mode: "dark", accent: "#A1F2CE" };
const choices = [{ id: "light", label: "浅色" }, { id: "dark", label: "深色" }, { id: "system", label: "跟随系统" }] as const;
const accents = ["#A1F2CE", "#75CBF4", "#B8A4F0", "#F9BD76", "#EF9EAF"];
const validColor = /^#[0-9A-Fa-f]{6}$/;

function loadAppearance(): Appearance {
  try {
    const saved: Partial<Appearance> | null = JSON.parse(localStorage.getItem(storageKey) ?? "null");
    return {
      mode: choices.find(({ id }) => id === saved?.mode)?.id ?? fallback.mode,
      accent: typeof saved?.accent === "string" && validColor.test(saved.accent) ? saved.accent.toUpperCase() : fallback.accent,
    };
  } catch {
    return fallback;
  }
}

function rgb(hex: string): Rgb {
  return [1, 3, 5].map((index) => Number.parseInt(hex.slice(index, index + 2), 16)) as Rgb;
}

function luminance(color: Rgb) {
  return color.reduce((total, channel, index) => {
    const s = channel / 255;
    return total + [0.2126, 0.7152, 0.0722][index] * (s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4);
  }, 0);
}

function contrast(a: Rgb, b: Rgb) {
  const [la, lb] = [luminance(a), luminance(b)];
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

function readableAccent(seed: Rgb, surfaces: Rgb[], target: number): Rgb {
  // 自选浅色或深色仍可作为填充色，文字、焦点与图表线色单独调整对比度。
  const stronger = surfaces[0][0] > 128 ? 0 : 255;
  for (let step = 0; step <= 20; step++) {
    const shade = seed.map((channel) => Math.round(channel + (stronger - channel) * step / 20)) as Rgb;
    if (surfaces.every((surface) => contrast(shade, surface) >= target)) return shade;
  }
  return [stronger, stronger, stronger];
}

function apply(appearance: Appearance, resolved: "dark" | "light") {
  const root = document.documentElement;
  root.dataset.theme = resolved;
  const seed = rgb(appearance.accent);
  const surfaces = (resolved === "dark" ? ["#171E1C", "#22322A"] : ["#FCFEFC", "#E6F0EA"]).map(rgb);
  const ink = readableAccent(seed, surfaces, 4.5);
  const black: Rgb = [0, 0, 0], white: Rgb = [255, 255, 255];
  root.style.setProperty("--accent", appearance.accent);
  root.style.setProperty("--accent-text", `rgb(${ink.join(",")})`);
  root.style.setProperty("--accent-on", contrast(seed, black) >= contrast(seed, white) ? "#000000" : "#FFFFFF");
  root.style.setProperty("--chart-tint", `rgba(${ink.join(",")}, 0.27)`);
}

export function useAppearance() {
  const [appearance, setAppearance] = useState(loadAppearance);
  const [systemDark, setSystemDark] = useState(() => window.matchMedia("(prefers-color-scheme: dark)").matches);
  const resolved = appearance.mode === "system" ? (systemDark ? "dark" : "light") : appearance.mode;

  useEffect(() => {
    const scheme = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () => setSystemDark(scheme.matches);
    update();
    scheme.addEventListener("change", update);
    return () => scheme.removeEventListener("change", update);
  }, []);

  useLayoutEffect(() => apply(appearance, resolved), [appearance, resolved]);
  useEffect(() => {
    try { localStorage.setItem(storageKey, JSON.stringify(appearance)); } catch { /* 当前窗口仍可调整。 */ }
  }, [appearance]);
  useEffect(() => {
    if (native) void getCurrentWindow().setTheme(appearance.mode === "system" ? null : appearance.mode).catch(console.error);
  }, [appearance.mode]);

  return { appearance, setAppearance, themeKey: `${resolved}:${appearance.accent}` };
}

export function AppearanceSettings({ appearance, change }: { appearance: Appearance; change: (appearance: Appearance) => void }) {
  const { t, preference, setPreference } = useI18n();
  return <section className="panel control-panel appearance-panel">
    <div className="section-heading"><div><h2><Palette size={18} /> {t("外观")}</h2></div></div>
    <div className="appearance-fields">
      <div><h3>{t("语言")}</h3><Choices label="语言"
        options={[{ id: "system", label: "跟随系统" }, { id: "zh-CN", label: "简体中文" }, { id: "en", label: "英文" }] as const}
        value={preference} compact onChange={setPreference} /></div>
      <div><h3>{t("明暗主题")}</h3><Choices label="明暗主题" options={choices} value={appearance.mode} compact
        onChange={(mode) => change({ ...appearance, mode })} /></div>
      <div><h3>{t("主题色")}</h3><ColorField title="主题色" colors={accents} value={appearance.accent}
        onChange={(accent) => change({ ...appearance, accent })} /></div>
    </div>
  </section>;
}
