# Native UI QA checklist

This checklist covers desktop behavior that unit tests and the browser preview
cannot establish. Run it on the supported Windows machine before a UI or
packaging release.

## Windows WebView2

- Run `npm run tauri:dev` and confirm the app renders without a blank window.
- Resize to the minimum `760x650` window size; confirm no horizontal clipping or
  unreachable primary action.
- Repeat at Windows display scaling 100%, 125%, 150%, and 200%.
- Use Settings > Accessibility > High contrast and confirm buttons, focus
  rings, dialogs, progress bars, and selected options remain distinguishable.
- Minimize, maximize, restore, and close the frameless window repeatedly.
- Open each CustomSelect, navigate with Arrow/Home/End/typeahead, select an
  option, and confirm focus remains usable.
- Open reset, model-delete, history-delete, and cookie-guide dialogs; verify
  focus trapping, Escape behavior, and focus restoration.
- Load a transcript with thousands of segments; scroll, search, and confirm
  only the visible segment window is mounted and scrolling remains responsive.

## Accessibility smoke test

- Run the frontend accessibility tests with `npm run test:frontend -- --run`.
- Check the main flow with a keyboard only: URL input, metadata inspection,
  backend selection, transcription, cancellation, history, and export.
- Confirm progress and error announcements are readable by a screen reader.

## Release boundary

This checklist is manual native validation. Passing TypeScript, Rust, or axe
tests alone does not prove WebView2 compositor, scaling, high-contrast, or
installer behavior.
