# Security Policy

## Scope

WhisperTube is a local-first desktop application. Video audio and transcript
processing are performed on the user's device. The application can nevertheless
handle sensitive local inputs, including imported cookies.txt session files, downloaded
media, transcript content, and model/runtime files.

## Reporting a vulnerability

Do not open a public issue for a security vulnerability. If GitHub's private
vulnerability reporting is enabled for this repository, use the repository's
Security tab. Otherwise, contact the maintainer through a private channel.

Include only the information needed to reproduce and assess the issue:

- affected version or commit;
- operating system and architecture;
- reproduction steps;
- expected and observed behavior;
- the smallest useful log or sample.

Remove cookies, tokens, private keys, credentials, personal data, and unrelated
logs before sending an attachment. Do not publish a proof of concept that
contains real account data.

## Local credentials and cookies

WhisperTube may use a user-selected Netscape-format cookies.txt file for
login-protected media. The application does not ask for the website password,
does not upload the file, and does not copy cookie contents into its database.
The original selected file is left unchanged. For each relevant local operation,
WhisperTube creates a temporary copy under its app-local `cookie-jars` directory
and passes that copy to yt-dlp. The copy is removed when the operation finishes
or is cancelled. A crash or cleanup failure can leave a copy on disk; startup
attempts cleanup again and reports failures. Cookie contents are not copied into
the history database. Treat the app-local directory and any backups as sensitive.

On Unix, app-local storage and job directories are restricted to the owning OS
account (0700), including existing application directories. macOS extended ACLs
on these owned roots are cleared before use. Symlinked storage roots and roots
owned by another OS account are rejected; parent directories outside the app's
storage are not changed. This also protects existing internal files and SQLite
sidecars. It cannot revoke access to bytes or open file handles obtained before
the permissions were tightened. User-selected external exports retain their
separate destination permission policy.

Do not commit any of the following:

- cookies.txt files or browser profile exports;
- .env files or credential files;
- private keys or updater signing material;
- tokens, passwords, or account data.

## Release trust

The Tauri updater verifies updater artifacts with the public key embedded in
the application configuration. This protects the updater channel, but it does
not provide operating-system distribution trust.

Before calling a production release ready, configure the appropriate platform
trust mechanisms:

- Windows Authenticode signing;
- Apple Developer ID signing and notarization;
- Linux packaging and distribution verification appropriate to the target.

The updater signing private key must remain outside source control and must be
provided to release automation only through the repository secret
TAURI_SIGNING_PRIVATE_KEY.

## Dependency audit status

The 2026-10-24 remediation raises the Tokio minimum and lockfile to 1.53.3 for
RUSTSEC-2026-0386. No exception was added for this advisory. The stricter gate
again reports only the five upstream residuals described below.

Dependency status is a dated observation, not a permanent assurance. Run
`npm audit --audit-level=high` and `cargo audit --file src-tauri/Cargo.lock`
against the checked-in lockfiles and a current advisory database. Run both
locally before pushing release changes. The dependency-audit.yml workflow also
runs npm and Cargo audits for dependency-file changes. Release workflows do not
wait for a separate test/lint quality gate. Do not suppress audit failures to publish a release.
Check the resolved versions and enabled features as well as the advisory count.

The remediation baseline checked on 2026-10-23 reports zero npm vulnerabilities
and zero Cargo vulnerability matches. Updating the compatible tauri-utils
dependency to 2.10.2 removed all five unmaintained unic-* packages. Cargo still
reports four unmaintained package warnings and one unsoundness warning.
The remaining GTK3 macro and zbus 5 dependencies come from upstream stable
desktop components; replacing them with GTK4 or zbus 6 is not an API-compatible
dependency substitution. In particular, Linux builds
inherit GTK3 `glib 0.18.5` through Tauri/WebKitGTK; RUSTSEC-2024-0429 concerns
its `VariantStrIter` implementation. No compatible fixed GTK3 dependency line
has been applied here. This residual requires upstream tracking and native
Linux validation; a passing audit exit code does not mean Linux is fully
hardened or that the application is free of vulnerabilities.

Run `node scripts/check-cargo-advisories.mjs` for the stricter regression gate
also used in dependency CI. It prints every tracked residual, fails for any
vulnerability or unavailable audit, and rejects new advisories or changes to
their package version/kind. `scripts/cargo-advisory-policy.json` records the
exact residuals, owner, upstream dependency path and resolution requirements.
This is not a blanket advisory ignore: the four unmaintained packages and glib
unsoundness remain open until a compatible upstream migration is verified.

The application ZIP reader uses ZIP 9.0.5 or newer. The updater's optional
Windows ZIP / Linux gzip support is disabled: the application release workflow
publishes signed raw `.exe` and `.AppImage` updater payloads. macOS
`.app.tar.gz` support remains enabled through the updater's unconditional
macOS dependencies. Keep these feature choices and
`.github/workflows/build-application-bundles.yml` in sync when changing formats.

For each advisory, document the affected dependency path, enabled platform and
features, attack prerequisites, available patch, and remaining uncertainty.
Prefer compatible patched versions. Any exception must be specific, reviewed,
and explain its residual risk; neither a scanner warning nor its absence alone
establishes exploitability. Native runtime binaries, OS distribution trust,
and untested platforms require separate verification from npm/Cargo audits.
