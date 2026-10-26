import { render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import { ProgressCard } from "./ProgressCard";

it("distinguishes the selected GPU from a backend actually reported by the runtime", () => {
  const progress = { stage: "transcribing" as const, messageCode: "", message: "Processing", percent: 42, backend: null as string | null,
    downloadedBytes: null, totalBytes: null, networkBytesPerSecond: null, cpuUsagePercent: null, gpuUsagePercent: 0 };
  const { rerender } = render(<I18nProvider><ProgressCard progress={progress} selectedModel={undefined} backend="cuda"
    networkSpeedBytesPerSecond={null} transcriptionElapsedMs={null} onCancel={vi.fn()} /></I18nProvider>);
  expect(screen.getByText(/Awaiting runtime backend/)).toBeTruthy();
  expect(screen.queryByText(/• CUDA/)).toBeNull();
  expect(screen.getByText("0%")).toBeTruthy();
  rerender(<I18nProvider><ProgressCard progress={{ ...progress, backend: "cpu" }} selectedModel={undefined} backend="cuda"
    networkSpeedBytesPerSecond={null} transcriptionElapsedMs={null} onCancel={vi.fn()} /></I18nProvider>);
  expect(screen.getByText(/• CPU/)).toBeTruthy();
  expect(screen.queryByText(/Awaiting runtime backend/)).toBeNull();
});
