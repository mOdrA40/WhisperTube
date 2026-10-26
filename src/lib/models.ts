import type { BackendChoice, SystemStatus } from "../types";

export const QWEN_MODEL_ID = "qwen3-asr-1.7b-q8_0";

export function supportsModelBackend(id: string, backend: BackendChoice, system: SystemStatus | null): boolean {
  return !isQwenModel(id) || Boolean(system?.qwenBackends?.includes(backend));
}

export function isQwenModel(id: string): boolean {
  return id === QWEN_MODEL_ID;
}

export function modelProvider(id: string): "openai" | "alibaba" {
  return isQwenModel(id) ? "alibaba" : "openai";
}
