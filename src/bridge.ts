import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { demo } from "./demo";
import type { Catalog, Gpu, HardwareEvent, Request, Service, SystemSettings, Telemetry } from "./types";

export const native = isTauri();

const systemRequests = new Set<Promise<unknown>>();

function track<T>(request: Promise<T>): Promise<T> {
  systemRequests.add(request);
  return request.finally(() => { systemRequests.delete(request); });
}

export const bridge = {
  telemetry: () => native ? invoke<Telemetry>("telemetry") : demo.telemetry(),
  catalog: () => native ? invoke<Catalog>("device_catalog") : Promise.resolve(demo.catalog),
  system: (request: Request) => track(native ? invoke<SystemSettings>("system_request", { request }) : demo.system(request)),
  gpu: () => track(native ? invoke<{ gpus: Gpu[] }>("system_request", { request: { action: "gpu" } }) : demo.gpu()),
  finishSystem: () => Promise.allSettled(systemRequests),
  events: (receive: (event: HardwareEvent) => void) => native
    ? listen<HardwareEvent>("hardware-event", ({ payload }) => receive(payload))
    : demo.events(receive),
  start: (service: Service, elevate = false) => native
    ? invoke<void>("start_hardware", { service, elevate })
    : demo.start(service),
  send: (service: Service, request: Request) => native
    ? invoke<void>("send_hardware", { service, request })
    : demo.send(service, request),
  stop: () => native ? invoke<void>("stop_hardware") : Promise.resolve(),
};
