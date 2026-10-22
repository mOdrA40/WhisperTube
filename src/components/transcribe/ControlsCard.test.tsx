import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { I18nProvider } from "../../i18n";
import { ControlsCard } from "./ControlsCard";

const models = ["base", "large-v3-turbo-q5_0", "large-v3-q5_0"].map((id) => ({
  id, label: id, description: id, sizeMb: 100, vramRequiredMb: 2048, installed: true,
}));
afterEach(cleanup);
function Fixture({ disabled = false, downloading = false }: { disabled?: boolean; downloading?: boolean }) {
  const [id, setId] = useState(models[1].id);
  const noop = () => undefined;
  return <I18nProvider><ControlsCard models={models} modelId={id} selectedModel={models.find((model) => model.id === id)}
    canStart={false} modelDownloadBlocked={false} computeTargetId="auto" language="auto" keepAudio={false}
    system={null} busy={false} operationActive={disabled} runtimeReady={true}
    downloadingModel={downloading ? { base: { id: "base", downloadedBytes: 1, totalBytes: 2, percent: 50, bytesPerSecond: null } } : {}}
    accelerators={[]} installingCuda={false} cudaDownloadPercent={0} installingAccelerator={null} acceleratorDownloadPercent={0}
    networkSpeedBytesPerSecond={null} vramWarning={null} acceleratorWarning={null}
    onInstallCuda={noop} onCancelCuda={noop} onCancelModel={noop} onInstallAccelerator={noop}
    onCancelAccelerator={noop} onModelChange={setId} onComputeTargetChange={noop} onLanguageChange={noop}
    onKeepAudioChange={noop} onDownloadModel={noop} onStart={noop} /></I18nProvider>;
}

describe("model radio keyboard interaction", () => {
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
