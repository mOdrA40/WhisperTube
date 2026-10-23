import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import { TranscriptCard } from "./TranscriptCard";
afterEach(cleanup);
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
