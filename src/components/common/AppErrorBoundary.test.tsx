import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AppErrorBoundary } from "./AppErrorBoundary";

function BrokenChild(): never {
  throw new Error("render failure");
}

describe("AppErrorBoundary", () => {
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
});
