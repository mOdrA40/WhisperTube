import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { I18nProvider } from "../../i18n";
import { AppShell } from "./AppShell";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({}),
}));

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  vi.restoreAllMocks();
});

it.each([
  ["en", ["Transcribe", "History", "Settings"]],
  ["id", ["Transkripsi", "Riwayat", "Pengaturan"]],
  ["zh", ["转录", "历史记录", "设置"]],
] as const)("keeps %s navigation named when responsive labels are hidden", (language, names) => {
  window.localStorage.setItem("whispertube.ui-language", language);
  vi.spyOn(window.navigator, "userAgent", "get").mockReturnValue("Windows");
  const onTabChange = vi.fn();
  const { container } = render(
    <I18nProvider>
      <AppShell tab="transcribe" historyCount={3} runtimeReady systemLoading={false} system={null} onTabChange={onTabChange}>
        <p>Content</p>
      </AppShell>
    </I18nProvider>,
  );
  // Model the responsive visibility independently of jsdom's media-query layout.
  container.querySelectorAll<HTMLElement>(".nav-item-label, .nav-item-badge").forEach((label) => {
    label.style.display = "none";
  });
  const navigation = within(screen.getByRole("navigation"));
  expect(navigation.getAllByRole("button")).toHaveLength(3);
  names.forEach((name, index) => {
    const button = navigation.getByRole("button", { name });
    fireEvent.click(button);
    expect(onTabChange).toHaveBeenLastCalledWith(["transcribe", "history", "settings"][index]);
  });
});
