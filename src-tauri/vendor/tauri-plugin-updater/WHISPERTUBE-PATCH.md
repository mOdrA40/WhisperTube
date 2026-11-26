# WhisperTube updater patch

This directory contains the build inputs and licenses from the published
`tauri-plugin-updater` 2.14.1 crate. The registry archive SHA-256 is
`d957abbcb3f9f6c76d22a3a0f012f30880640d0bccb5634d2b12a9937e4c872b`.
The upstream commit is `128190689dbe59b59d344e01bf07b00862aa3791`,
at `plugins/updater` in [Tauri plugins-workspace](https://github.com/tauri-apps/plugins-workspace).

The macOS installer retains a surviving backup even if another entry occupies
the install path. It attempts a privileged install only when the failed
unprivileged attempt left no backup. The privileged fallback uses exact-path
renames so an intervening directory cannot absorb the staged or backup bundle.
After the new bundle is live, cleanup errors are logged without failing the
completed install or preventing the app from restarting.

The macOS application build runs the vendor's filesystem tests on dummy bundles,
including real AppleScript and JavaScript for Automation calls without elevation.
Keep the registry identity in `scripts/cargo-advisory-policy.json` in sync with
the vendored release. Test APFS and administrator authorization before removing
this override in favor of an upstream fix.
