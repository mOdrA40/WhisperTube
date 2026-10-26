import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { I18nProvider } from "../../i18n";
import { ControlsCard } from "./ControlsCard";
import type { SystemStatus } from "../../types";

const models = ["base", "large-v3-turbo-q5_0", "large-v3-q5_0", "qwen3-asr-1.7b-q8_0"].map((id) => ({
  id, label: id, description: id, sizeMb: 100, vramRequiredMb: 2048, installed: true,
}));

const macSystem: SystemStatus = {
  qwenBackends: ["auto", "cpu"], ytDlp: true, ffmpeg: true, cpuEngine: true,
  cudaEngine: false, cudaSupported: false, nvidia: false, gpuName: null,
  gpuMemoryMb: null, gpuFreeMemoryMb: null, availableMemoryMb: 8192,
  nvidiaDriverVersion: null, cudaInstallable: false, cpuThreads: 8, recommendation: "CPU",
  recommendedModelId: "base", recommendedBackend: "auto", computeDevices: [],
  jobStorageBytes: 0, jobStorageLimitBytes: 21474836480,
  accelerators: [{ id: "metal", backend: "metal", label: "Metal", supported: true,
    installed: true, downloadable: false, description: "Whisper Metal", memoryAvailableMb: 8192,
    memoryRequiredMb: 2048, memorySufficient: true, preflightChecked: false, preflightCompatible: false }],
};
afterEach(cleanup);
function Fixture({ disabled = false, downloading = false, resumeStage = null, checkingResume = false, retryAvailable = false, system = null }: {
  disabled?: boolean; downloading?: boolean; resumeStage?: "audio" | "download" | null; checkingResume?: boolean; retryAvailable?: boolean; system?: SystemStatus | null;
}) {
  const [id, setId] = useState(models[1].id);
  const noop = () => undefined;
  return <I18nProvider><ControlsCard models={models} modelId={id} selectedModel={models.find((model) => model.id === id)}
    canStart={!checkingResume && !disabled} resumeStage={resumeStage} checkingResume={checkingResume} retryAvailable={retryAvailable} modelDownloadBlocked={false} computeTargetId="auto" language="auto" keepAudio={false}
    system={system} busy={false} operationActive={disabled} runtimeReady={true}
    downloadingModel={downloading ? { base: { id: "base", downloadedBytes: 1, totalBytes: 2, percent: 50, bytesPerSecond: null } } : {}}
    accelerators={system?.accelerators ?? []} installingCuda={false} cudaDownloadPercent={0} installingAccelerator={null} acceleratorDownloadPercent={0}
    networkSpeedBytesPerSecond={null} vramWarning={null} acceleratorWarning={null}
    onInstallCuda={noop} onCancelCuda={noop} onCancelModel={noop} onInstallAccelerator={noop}
    onCancelAccelerator={noop} onModelChange={setId} onComputeTargetChange={noop} onLanguageChange={noop}
    onKeepAudioChange={noop} onDownloadModel={noop} onStart={noop} /></I18nProvider>;
}

describe("model radio keyboard interaction", () => {
  it.each([false, true])("offers Qwen Metal only when its native runtime supports it (Apple Silicon: %s)", (appleSilicon) => {
    const status: SystemStatus = { ...macSystem,
      qwenBackends: appleSilicon ? ["auto", "cpu", "metal"] : ["auto", "cpu"],
    };
    render(<Fixture system={status} />);
    const compute = () => {
      const selectors = screen.getAllByRole("combobox");
      fireEvent.click(selectors[selectors.length - 1]);
    };
    compute();
    // Whisper continues to offer its installed Metal accelerator on Intel.
    expect(screen.getAllByRole("option")).toHaveLength(3);
    fireEvent.keyDown(screen.getByRole("listbox"), { key: "Escape" });
    fireEvent.click(screen.getByRole("combobox", { name: "Model provider" }));
    fireEvent.click(screen.getByRole("option", { name: "Alibaba · Qwen" }));
    compute();
    expect(screen.getAllByRole("option")).toHaveLength(appleSilicon ? 3 : 2);
    expect(screen.queryByRole("option", { name: "Apple Metal" }) !== null).toBe(appleSilicon);
  });

  it("offers Apple Silicon Qwen Metal without a Whisper Metal pack", () => {
    const status: SystemStatus = { ...macSystem, qwenBackends: ["auto", "cpu", "metal"], accelerators: [] };
    render(<Fixture system={status} />);
    fireEvent.click(screen.getByRole("combobox", { name: "Model provider" }));
    fireEvent.click(screen.getByRole("option", { name: "Alibaba · Qwen" }));
    const selectors = screen.getAllByRole("combobox");
    fireEvent.click(selectors[selectors.length - 1]);
    expect(screen.getByRole("option", { name: "Apple Metal" })).toBeTruthy();
  });
  it("keeps providers separate and restores Whisper when switching back", () => {
    render(<Fixture />);
    expect(screen.getAllByRole("radio")).toHaveLength(3);
    expect(screen.queryByText("Qwen3-ASR 1.7B · Q8")).toBeNull();
    fireEvent.click(screen.getByRole("combobox", { name: "Model provider" }));
    fireEvent.click(screen.getByRole("option", { name: "Alibaba · Qwen" }));
    expect(screen.getAllByRole("radio")).toHaveLength(1);
    expect(screen.getByRole("radio").getAttribute("aria-checked")).toBe("true");
    expect(screen.getByRole("note").textContent).toContain("approximate");
    expect(screen.queryByText("Balanced")).toBeNull();
    fireEvent.click(screen.getByRole("combobox", { name: "Model provider" }));
    fireEvent.click(screen.getByRole("option", { name: "OpenAI · Whisper" }));
    expect(screen.getAllByRole("radio")).toHaveLength(3);
    expect(screen.getByRole("radio", { name: /Balanced/ }).getAttribute("aria-checked")).toBe("true");
  });
  it.each([{ disabled: true }, { downloading: true }])("locks provider switching during work: %j", (props) => {
    render(<Fixture {...props} />);
    expect((screen.getByRole("combobox", { name: "Model provider" }) as HTMLButtonElement).disabled).toBe(true);
  });
  it("does not offer CUDA when the installed engine has no visible devices", () => {
    const system = { nvidia: true, cudaSupported: true, cudaInstallable: true, cudaEngine: true,
      computeDevices: [], accelerators: [], recommendedModelId: "base", cpuThreads: 8,
      gpuName: "NVIDIA GPU", gpuFreeMemoryMb: 4096 } as unknown as SystemStatus;
    render(<Fixture system={system} />);
    const selectors = screen.getAllByRole("combobox");
    fireEvent.click(selectors[selectors.length - 1]);
    expect(screen.queryByRole("option", { name: "NVIDIA CUDA" })).toBeNull();
    expect(screen.getAllByRole("option")).toHaveLength(2);
  });
  it.each(["audio", "download"] as const)("labels an available %s cache as Continue", (resumeStage) => {
    render(<Fixture resumeStage={resumeStage} />);
    expect(screen.getByRole("button", { name: "Continue transcription" })).toBeTruthy();
    expect(screen.getByRole("status").textContent).toContain(resumeStage === "audio" ? "when settings match" : "try to continue the download");
  });

  it("shows Start without saved progress and disables the action during lookup", () => {
    const { rerender } = render(<Fixture />);
    expect(screen.getByRole("button", { name: "Transcribe now" })).toBeTruthy();
    rerender(<Fixture checkingResume />);
    expect((screen.getByRole("button", { name: "Checking saved progress…" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("offers Retry without promising saved progress when the attempt failed before caching", () => {
    render(<Fixture retryAvailable />);
    expect(screen.getByRole("button", { name: "Try transcription again" })).toBeTruthy();
    expect(screen.getByRole("status").textContent).toContain("start from the beginning");
  });
  it("has one tab stop, moves selection with arrows, wraps, and supports Home/End", () => {
    render(<Fixture />);
    const radios = screen.getAllByRole("radio");
    expect(radios.map((radio) => radio.tabIndex)).toEqual([-1, 0, -1]);
    radios[1].focus();
    for (const [key, index] of [["ArrowRight", 2], ["ArrowDown", 0], ["ArrowLeft", 2], ["ArrowUp", 1], ["Home", 0], ["End", 2]] as const) {
      fireEvent.keyDown(document.activeElement!, { key });
      expect(document.activeElement).toBe(radios[index]);
      expect(radios[index].getAttribute("aria-checked")).toBe("true");
      expect(radios.map((radio) => radio.tabIndex)).toEqual(radios.map((_, position) => position === index ? 0 : -1));
    }
  });

  it.each([{ disabled: true }, { downloading: true }])("does not change a disabled model group: %j", (props) => {
    render(<Fixture {...props} />);
    const radios = screen.getAllByRole("radio");
    expect(radios.every((radio) => (radio as HTMLButtonElement).disabled)).toBe(true);
    fireEvent.keyDown(radios[1], { key: "ArrowRight" });
    expect(radios[1].getAttribute("aria-checked")).toBe("true");
  });
});
