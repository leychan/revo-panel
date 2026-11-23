export type Model = Record<"sys_vendor" | "product_name" | "board_name" | "bios_version", string | null>;

export interface Sensor {
  device: string;
  chip: string;
  channel: string;
  label: string | null;
  celsius: number;
}

export interface Battery {
  name: string;
  percent: number | null;
  status: string | null;
  health_percent: number | null;
  cycle_count: number | null;
  charge_end_threshold: number | null;
  charge_types: string | null;
  discharge_watts: number | null;
}

export type GpuRuntime = "active" | "suspended" | "transition" | "not_detected" | "unknown";

export interface UsbCPort {
  name: string;
  connected: boolean;
  power_role: string | null;
  input_online: boolean | null;
  power_mode: string | null;
  voltage_volts: number | null;
  current_amps: number | null;
}

export interface Telemetry {
  kernel: string;
  model: Model;
  temperatures: Sensor[];
  cpu_times: [number, number] | null;
  batteries: Battery[];
  power_sources: { name: string; online: boolean }[];
  usb_c_ports: UsbCPort[];
  cpu_turbo_restricted: boolean | null;
  nvidia_runtime: GpuRuntime;
  fans: { label: string | null; channel: string; rpm: number }[];
  lighting_usb: { id: string }[];
}

export interface Mode {
  id: string;
  label: string;
  color: boolean;
  code?: number;
}

export interface Catalog {
  keyboard_max: number;
  keyboard: Mode[];
  chassis: Mode[];
  zones: Record<string, string>;
  pump: { id: string; label: string; duty: number }[];
  cooler_lights: Mode[];
}

export interface Effect {
  name: string;
  color: string;
  speed: number;
  direction: number;
  brightness: number;
}

export interface LightReading {
  brightness: number;
  powered: boolean;
  mode: number;
  speed: number;
  direction: number;
}

export interface ChassisProbe {
  zones: string[];
  status: string;
  capability?: number;
  detail?: string;
}

export interface CoolerDevice {
  name: string;
  known_model: boolean;
  connected: boolean;
  nearby: boolean;
}

export type Service = "lighting" | "cooler" | "turbo";
export type Request = { action: string } & Record<string, string | number | boolean>;

export interface TurboStatus {
  enabled: boolean;
  restricted: boolean;
  power_source: "battery" | "external" | "unknown";
}

export interface Reply {
  code?: number | null;
  detail?: string;
  error?: string;
  state?: LightReading;
  turbo?: TurboStatus;
  chassis_probe?: ChassisProbe;
  chassis?: { name: string; brightness: number };
  event?: string;
  devices?: Record<string, CoolerDevice>;
  name?: string;
  path?: string;
  meter?: number | null;
  version?: string;
  reason?: string;
  message?: string;
  component?: string;
  value?: string | number;
  zone?: string;
  duty?: number;
}

export interface HardwareEvent {
  service: Service;
  kind: "message" | "exit";
  data: Reply;
}

export interface DisplayMode {
  id: string;
  refreshRate: number;
  size: { width: number; height: number };
}

export interface SystemSettings {
  power: { active?: string; options?: { name: string }[]; error?: string };
  display: { name?: string; current_mode?: DisplayMode; modes?: DisplayMode[]; brightness?: number | null; hdr?: boolean; error?: string };
  touchpad: { enabled?: boolean; error?: string };
}

export type Gpu = Record<"name" | "temperature.gpu" | "utilization.gpu" | "memory.used" | "memory.total" | "power.draw", string>;
