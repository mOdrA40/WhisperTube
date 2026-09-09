import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppErrorBoundary } from "./AppErrorBoundary";

const relaunch = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));

vi.mock("@tauri-apps/plugin-process", () => ({ relaunch }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ close: vi.fn().mockResolvedValue(undefined) }),
}));

function BrokenChild(): never {
  throw new Error("render failure");
}

describe("AppErrorBoundary", () => {
  afterEach(() => {
    cleanup();
    relaunch.mockClear();
  });

  it("keeps a render failure inside a recoverable fallback", () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);

    render(
      <AppErrorBoundary>
        <BrokenChild />
      </AppErrorBoundary>,
    );

    expect(screen.getByRole("alert").textContent).toContain("Antarmuka mengalami masalah");
    expect(screen.getByRole("button", { name: "Muat ulang aplikasi" })).toBeTruthy();
    errorSpy.mockRestore();
  });

  it("relaunches the desktop process instead of only reloading the WebView", async () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);

    render(
      <AppErrorBoundary>
        <BrokenChild />
      </AppErrorBoundary>,
    );

    screen.getByRole("button", { name: "Muat ulang aplikasi" }).click();
    await vi.waitFor(() => expect(relaunch).toHaveBeenCalledOnce());
    errorSpy.mockRestore();
  });
});
