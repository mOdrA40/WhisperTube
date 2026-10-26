import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import type { TranscriptResult } from "../../types";
import { TranscribePage } from "./TranscribePage";

const model = { id: "base", label: "Fast", description: "", sizeMb: 142, vramRequiredMb: 2048, installed: true };
const result: TranscriptResult = { historyId: 1, title: "A transcript", channel: "Channel", language: "en", duration: 60,
  model: "base", backend: "cpu", segments: [], text: "", txtPath: "", srtPath: "", vttPath: "", audioPath: null };
function props(): Parameters<typeof TranscribePage>[0] {
  return {
    url: "https://example.test/video", busy: false, operationActive: false, inspecting: false, cancellingInspection: false,
    metadata: null, hasResult: false, progress: { stage: "idle", messageCode: "", message: "", percent: 0, backend: null,
      downloadedBytes: null, totalBytes: null, networkBytesPerSecond: null, cpuUsagePercent: null, gpuUsagePercent: null },
    transcriptionElapsedMs: null, selectedModel: model, canStart: true, resumeStage: null, checkingResume: false,
    retryAvailable: false, modelDownloadBlocked: false, backend: "auto", computeTargetId: "auto", system: null,
    models: [model], modelId: "base", language: "auto", keepAudio: false, runtimeReady: true, downloadingModel: {},
    accelerators: [], installingCuda: false, cudaDownloadPercent: 0, installingAccelerator: null,
    acceleratorDownloadPercent: 0, networkSpeedBytesPerSecond: null, vramWarning: null, acceleratorWarning: null,
    result: null, copied: false, searchQuery: "", filteredSegments: [], onTabChange: vi.fn(), onUrlChange: vi.fn(),
    onInspect: vi.fn(), onClear: vi.fn(), onCancelInspection: vi.fn(), onCancelTranscription: vi.fn(),
    onModelChange: vi.fn(), onComputeTargetChange: vi.fn(), onLanguageChange: vi.fn(), onKeepAudioChange: vi.fn(),
    onDownloadModel: vi.fn(), onCancelModel: vi.fn(), onInstallCuda: vi.fn(), onCancelCuda: vi.fn(),
    onInstallAccelerator: vi.fn(), onCancelAccelerator: vi.fn(), onStart: vi.fn(), onCopy: vi.fn(),
    onExport: vi.fn(), onRevealAudio: vi.fn(), onSearchChange: vi.fn(),
  };
}

it("gives the document room while retaining one mounted configuration and its callbacks", () => {
  const initial = props();
  const { rerender, container } = render(<I18nProvider><TranscribePage {...initial} /></I18nProvider>);
  const radio = screen.getByRole("radio");
  const configuration = container.querySelector("#transcription-configuration");
  expect(screen.getAllByRole("radiogroup")).toHaveLength(1);
  rerender(<I18nProvider><TranscribePage {...initial} result={result} hasResult /></I18nProvider>);
  expect(configuration).toBe(container.querySelector("#transcription-configuration"));
  expect(screen.queryByRole("radiogroup")).toBeNull();
  const toggle = screen.getByRole("button", { name: "Transcription settings" });
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(toggle);
  expect(screen.getByRole("radio")).toBe(radio);
  expect(screen.getAllByRole("radiogroup")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Transcribe now" }));
  expect(initial.onStart).toHaveBeenCalledOnce();
  expect(initial.onInspect).not.toHaveBeenCalled();
  expect(initial.onModelChange).not.toHaveBeenCalled();
});

it("keeps progress and cancellation active while the previous document is hidden", () => {
  const initial = props();
  render(<I18nProvider><TranscribePage {...initial} result={result} hasResult busy operationActive canStart={false}
    progress={{ ...initial.progress, stage: "transcribing", backend: "cpu", percent: 42 }} /></I18nProvider>);
  expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("42");
  expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(initial.onCancelTranscription).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "Transcription settings" }));
  expect((screen.getByRole("button", { name: "Transcribe now" }) as HTMLButtonElement).disabled).toBe(true);
});

it("never collapses an active component download when a saved transcript is open", () => {
  const initial = props();
  const absent = { ...model, installed: false };
  render(<I18nProvider><TranscribePage {...initial} result={result} hasResult operationActive canStart={false}
    models={[absent]} selectedModel={absent}
    downloadingModel={{ base: { id: "base", downloadedBytes: 32, totalBytes: 142, percent: 22, bytesPerSecond: 4 } }} /></I18nProvider>);
  const setup = screen.getByRole("button", { name: "Component setup in progress" }) as HTMLButtonElement;
  expect(setup.disabled).toBe(true);
  expect(setup.getAttribute("aria-expanded")).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: "Cancel model download" }));
  expect(initial.onCancelModel).toHaveBeenCalledOnce();
  expect((screen.getByRole("button", { name: "Copy" }) as HTMLButtonElement).disabled).toBe(true);
});
