import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import { SettingsPage } from "./SettingsPage";

describe("SettingsPage local data reset", () => {
  it("requires explicit confirmation, keeps data on cancel, and surfaces reset failures", async () => {
    const onResetUserData = vi.fn().mockResolvedValue(undefined);

    render(
      <I18nProvider>
        <SettingsPage
          cookiesPath=""
          usingSafariSession={false}
          browsers={[]}
          system={null}
          models={[{
            id: "base",
            label: "Fast",
            description: "Lightweight",
            sizeMb: 142,
            vramRequiredMb: 2048,
            installed: true,
          }]}
          modelDownloadBlockReasons={{}}
          busy={false}
          resettingData={false}
          historyTotalCount={3}
          downloadingModel={{}}
          accelerators={[]}
          installingCuda={false}
          cudaDownloadPercent={0}
          installingAccelerator={null}
          acceleratorDownloadPercent={0}
          networkSpeedBytesPerSecond={null}
          appUpdate={null}
          appUpdateStatus="idle"
          appUpdateProgress={{ downloadedBytes: 0, totalBytes: null, percent: 0 }}
          appUpdateError={null}
          updateChecking={false}
          onSelectCookiesFile={vi.fn()}
          onClearCookiesFile={vi.fn()}
          onUseSafariSession={vi.fn()}
          onOpenUrl={vi.fn()}
          onDownloadModel={vi.fn()}
          onCancelModel={vi.fn()}
          onRemoveModel={vi.fn().mockResolvedValue(undefined)}
          onResetUserData={onResetUserData}
          onRefresh={vi.fn()}
          onInstallCuda={vi.fn()}
          onCancelCuda={vi.fn()}
          onInstallAccelerator={vi.fn()}
          onCancelAccelerator={vi.fn()}
          onCheckForUpdate={vi.fn()}
          onInstallAppUpdate={vi.fn()}
        />
      </I18nProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Reset WhisperTube data" }));
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(screen.getByText("3 saved history item(s) and files in transcription jobs")).toBeTruthy();
    expect(onResetUserData).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Keep my data" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(onResetUserData).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Delete model" }));
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(screen.getByText(/Delete the Fast model\?/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Keep model" }));
    expect(screen.queryByRole("dialog")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Reset WhisperTube data" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    await waitFor(() => expect(onResetUserData).toHaveBeenCalledOnce());
    expect(screen.queryByRole("dialog")).toBeNull();

    onResetUserData.mockRejectedValueOnce(new Error("Database sedang terkunci"));
    fireEvent.click(screen.getByRole("button", { name: "Reset WhisperTube data" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("Database sedang terkunci"));
    expect(screen.getByRole("dialog")).toBeTruthy();
  });
});
