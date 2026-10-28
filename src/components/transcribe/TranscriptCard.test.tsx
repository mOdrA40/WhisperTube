import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import { TranscriptCard } from "./TranscriptCard";
afterEach(cleanup);
it("shows a persisted recovery warning when reopening a transcript", () => {
  render(<I18nProvider><TranscriptCard result={{ historyId: 1, title: "Recovered recording", channel: "Local audio", language: "id", duration: 60,
    audioRecovered: true, model: "base", backend: "cpu", segments: [], text: "", txtPath: "", srtPath: "", vttPath: "", audioPath: null }}
    operationActive={false} copied={false} searchQuery="" filteredSegments={[]} onCopy={vi.fn()} onExport={vi.fn()}
    onRevealAudio={vi.fn()} onSearchChange={vi.fn()} transcriptionElapsedMs={null} /></I18nProvider>);
  expect(screen.getByRole("status").textContent).toContain("timestamps may shift");
  expect(screen.queryByText("Transcription complete")).toBeNull();
  expect(screen.getByText("Available audio transcribed")).toBeTruthy();
});
it("shows the actual audio coverage without claiming the full source was transcribed", () => {
  render(<I18nProvider><TranscriptCard result={{ historyId: 1, title: "Short audio", channel: "Channel", language: "en", duration: 1, sourceDuration: 60,
    model: "base", backend: "cpu", segments: [], text: "", txtPath: "", srtPath: "", vttPath: "", audioPath: null }}
    operationActive={false} copied={false} searchQuery="" filteredSegments={[]} onCopy={vi.fn()} onExport={vi.fn()}
    onRevealAudio={vi.fn()} onSearchChange={vi.fn()} transcriptionElapsedMs={null} /></I18nProvider>);
  expect(screen.getByText("Available audio transcribed")).toBeTruthy();
  expect(screen.queryByText("Transcription complete")).toBeNull();
  expect(screen.getByRole("status").textContent).toContain("00:01");
  expect(screen.getByRole("status").textContent).toContain("01:00");
  expect((screen.getByRole("button", { name: "TXT" }) as HTMLButtonElement).disabled).toBe(false);
});
it("keeps saved audio accessible while disabling deleted transcript actions", () => {
  const onRevealAudio = vi.fn();
  const onCopy = vi.fn();
  const onExport = vi.fn();
  render(<I18nProvider><TranscriptCard result={{ historyId: 1, title: "Audio only", channel: "Channel", language: "en", duration: 60,
    model: "base", backend: "cpu", segments: [], text: "", txtPath: "", srtPath: "", vttPath: "", audioPath: "D:/audio.wav", transcriptAvailable: false }}
    operationActive={false} copied={false} searchQuery="" filteredSegments={[]} onCopy={onCopy} onExport={onExport}
    onRevealAudio={onRevealAudio} onSearchChange={vi.fn()} transcriptionElapsedMs={null} /></I18nProvider>);
  for (const name of ["Copy", "TXT", "SRT", "VTT"]) {
    const button = screen.getByRole("button", { name });
    expect((button as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(button);
  }
  expect(onCopy).not.toHaveBeenCalled();
  expect(onExport).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Show saved audio" }));
  expect(onRevealAudio).toHaveBeenCalledOnce();
});

it("discloses approximate Qwen timing when opening a saved transcript", () => {
  render(<I18nProvider><TranscriptCard result={{ historyId: 1, title: "Saved Qwen", channel: "", language: "id", duration: 20,
    model: "qwen3-asr-1.7b-q8_0", backend: "cuda", segments: [], text: "", txtPath: "", srtPath: "", vttPath: "", audioPath: null }}
    operationActive={false} copied={false} searchQuery="" filteredSegments={[]} onCopy={vi.fn()} onExport={vi.fn()}
    onRevealAudio={vi.fn()} onSearchChange={vi.fn()} transcriptionElapsedMs={null} /></I18nProvider>);
  expect(screen.getByRole("note").textContent).toContain("not speech alignment");
});
