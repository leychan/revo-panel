import { useEffect, useRef } from "react";
import type { CSSProperties, ReactNode } from "react";
import { useI18n } from "./i18n";

export function Status({ children, tone = "quiet" }: { children: ReactNode; tone?: "good" | "quiet" | "warn" }) {
  return <span className={`status status--${tone}`}><i />{children}</span>;
}

export function Slider({ label, value, min = 0, max, unit = "%", disabled, onChange }: {
  label: string; value: number; min?: number; max: number; unit?: string; disabled?: boolean; onChange: (value: number) => void;
}) {
  const { t } = useI18n();
  return <label className="slider-field">
    <span className="slider-caption"><span>{t(label)}</span><strong>{value}<small>{unit}</small></strong></span>
    <input type="range" min={min} max={max} value={value} disabled={disabled}
      style={{ "--fill": `${max > min ? (value - min) / (max - min) * 100 : 0}%` } as CSSProperties}
      onChange={(event) => onChange(Number(event.currentTarget.value))} />
  </label>;
}

export function Choices<T extends string | number>({ label, options, value, disabled, onChange, compact = false }: {
  label: string; options: readonly { id: T; label: string }[]; value: T; disabled?: boolean; onChange: (value: T) => void; compact?: boolean;
}) {
  const { t } = useI18n();
  return <div className={`choices ${compact ? "choices--compact" : ""}`} role="group" aria-label={t(label)}>
    {options.map((option) => <button key={option.id} type="button" aria-pressed={option.id === value}
      className={option.id === value ? "selected" : undefined} disabled={disabled}
      onClick={() => { if (option.id !== value) onChange(option.id); }}>{t(option.label)}</button>)}
  </div>;
}

const presets = ["#32D4FF", "#95F2CA", "#C2AAF8", "#FF8DA0", "#FFD19A", "#F3F7F6"];

export function ColorField({ title, value, disabled, colors = presets, onChange }: { title: string; value: string; disabled?: boolean; colors?: string[]; onChange: (color: string) => void }) {
  const { t } = useI18n();
  return <div className="color-field" role="group" aria-label={t(title)}>
    {colors.map((color) => <button type="button" key={color} aria-label={`${t(title)} ${color}`} title={color}
      aria-pressed={value.toUpperCase() === color} disabled={disabled} className="color-chip"
      style={{ "--color": color } as CSSProperties} onClick={() => { if (value.toUpperCase() !== color) onChange(color); }} />)}
    <label className="color-custom" aria-label={t("自定{title}", { title: t(title) })}>
      <input type="color" value={value} disabled={disabled} onChange={(event) => onChange(event.currentTarget.value.toUpperCase())} />
      <span>{value.toUpperCase()}</span>
    </label>
  </div>;
}

export type Sample = { time: number; value: number };

export function TemperatureChart({ samples, themeKey }: { samples: Sample[]; themeKey: string }) {
  const { t } = useI18n();
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const surface = canvas.current;
    if (!surface) return;
    const draw = () => {
      const context = surface.getContext("2d");
      const { width, height } = surface.getBoundingClientRect();
      if (!context || width < 60 || height < 50) return;
      const scale = window.devicePixelRatio || 1;
      const theme = getComputedStyle(document.documentElement);
      const colors = (name: string) => theme.getPropertyValue(name).trim();
      surface.width = Math.round(width * scale);
      surface.height = Math.round(height * scale);
      context.scale(scale, scale);
      const left = 34, top = 13, right = width - 8, bottom = height - 22;
      const min = Math.min(20, Math.floor(Math.min(20, ...samples.map((sample) => sample.value)) / 20) * 20);
      const max = Math.max(100, Math.ceil(Math.max(100, ...samples.map((sample) => sample.value)) / 20) * 20);
      const y = (celsius: number) => bottom - (celsius - min) / (max - min) * (bottom - top);
      context.font = '10px "DejaVu Sans Mono", monospace';
      context.fillStyle = colors("--muted");
      for (let value = min; value <= max; value += 20) {
        context.beginPath();
        context.strokeStyle = colors("--line");
        context.moveTo(left, y(value));
        context.lineTo(right, y(value));
        context.stroke();
        context.fillText(`${value}°`, 0, y(value) + 4);
      }
      for (let step = 1; step < 6; step++) {
        const x = left + (right - left) * step / 6;
        context.beginPath();
        context.moveTo(x, top);
        context.lineTo(x, bottom);
        context.stroke();
      }
      context.fillText("−60s", left, height - 3);
      const now = t("现在");
      context.fillText(now, right - context.measureText(now).width, height - 3);
      if (!samples.length) return;
      const end = samples[samples.length - 1].time;
      const points = samples.filter((sample) => end - sample.time <= 60_000)
        .map((sample) => [right - (end - sample.time) / 60_000 * (right - left), y(sample.value)] as const);
      const line = new Path2D();
      line.moveTo(...points[0]);
      points.slice(1).forEach((point) => line.lineTo(...point));
      const last = points[points.length - 1];
      const area = new Path2D(line);
      area.lineTo(last[0], bottom);
      area.lineTo(points[0][0], bottom);
      area.closePath();
      const tint = context.createLinearGradient(0, top, 0, bottom);
      tint.addColorStop(0, colors("--chart-tint"));
      tint.addColorStop(1, "transparent");
      context.fillStyle = tint;
      context.fill(area);
      context.lineWidth = 2;
      context.strokeStyle = colors("--accent-text");
      context.lineJoin = "round";
      context.stroke(line);
      context.beginPath();
      context.arc(last[0], last[1], 3.5, 0, Math.PI * 2);
      context.fillStyle = colors("--accent-text");
      context.fill();
    };
    const observer = new ResizeObserver(draw);
    observer.observe(surface);
    draw();
    return () => observer.disconnect();
  }, [samples, themeKey, t]);
  return <canvas className="temperature-chart" aria-label={t("最近 60 秒的 CPU 封装温度")} ref={canvas} />;
}
