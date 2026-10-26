import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppErrorBoundary } from "./AppErrorBoundary";

const relaunch = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
const exit = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));

vi.mock("@tauri-apps/plugin-process", () => ({ relaunch, exit }));

function BrokenChild(): never {
  throw new Error("render failure");
}

describe("AppErrorBoundary", () => {
  afterEach(() => {
    cleanup();
    relaunch.mockClear();
    exit.mockClear();
    vi.restoreAllMocks();
    window.localStorage.clear();
  });

  it("keeps a render failure inside a recoverable fallback", () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);

    render(
      <AppErrorBoundary>
        <BrokenChild />
      </AppErrorBoundary>,
    );

    expect(screen.getByRole("alert").textContent).toContain("The interface encountered a problem");
    expect(screen.getByRole("button", { name: "Restart application" })).toBeTruthy();
    errorSpy.mockRestore();
  });

  it.each([
    ["id", "Antarmuka mengalami masalah", "Mulai ulang aplikasi"],
    ["zh", "界面出现问题", "重启应用"],
    ["invalid", "The interface encountered a problem", "Restart application"],
  ])("recovers without a provider using stored language %s", (language, title, button) => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    window.localStorage.setItem("whispertube.ui-language", language);
    render(<AppErrorBoundary><BrokenChild /></AppErrorBoundary>);
    expect(screen.getByRole("heading").textContent).toBe(title);
    expect(screen.getByRole("button", { name: button })).toBeTruthy();
  });

  it("uses English when storage is unavailable", () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("unavailable"); });
    render(<AppErrorBoundary><BrokenChild /></AppErrorBoundary>);
    expect(screen.getByRole("button", { name: "Restart application" })).toBeTruthy();
  });

  it("relaunches the desktop process instead of only reloading the WebView", async () => {
    const errorSpy = vi.spyOn(console, "error").mockImplementation(() => undefined);

    render(
      <AppErrorBoundary>
        <BrokenChild />
      </AppErrorBoundary>,
    );

    screen.getByRole("button", { name: "Restart application" }).click();
    await vi.waitFor(() => expect(relaunch).toHaveBeenCalledOnce());
    expect(exit).not.toHaveBeenCalled();
    errorSpy.mockRestore();
  });

  it("exits on failed crash recovery instead of hiding a broken window to the tray", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    relaunch.mockRejectedValueOnce(new Error("restart unavailable"));
    render(<AppErrorBoundary><BrokenChild /></AppErrorBoundary>);
    screen.getByRole("button", { name: "Restart application" }).click();
    await vi.waitFor(() => expect(exit).toHaveBeenCalledWith(1));
    expect(relaunch).toHaveBeenCalledOnce();
  });
});
