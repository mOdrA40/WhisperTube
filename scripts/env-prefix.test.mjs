// @vitest-environment node
import { afterEach, expect, it, vi } from "vitest";
import { resolveConfig } from "vite";

afterEach(() => vi.unstubAllEnvs());

it("keeps signing credentials out of the frontend environment while preserving public Tauri fields", async () => {
  vi.stubEnv("TAURI_SIGNING_PRIVATE_KEY", "dummy signing credential; not a real key");
  vi.stubEnv("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "dummy password");
  vi.stubEnv("TAURI_ENV_PLATFORM", "fixture-platform");
  vi.stubEnv("VITE_PUBLIC_FIXTURE", "public fixture");
  const config = await resolveConfig({ logLevel: "silent" }, "build", "production");
  expect(config.env.TAURI_SIGNING_PRIVATE_KEY).toBeUndefined();
  expect(config.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD).toBeUndefined();
  expect(config.env.TAURI_ENV_PLATFORM).toBe("fixture-platform");
  expect(config.env.VITE_PUBLIC_FIXTURE).toBe("public fixture");
});
