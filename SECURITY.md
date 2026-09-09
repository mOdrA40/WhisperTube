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
The selected file is passed to yt-dlp only while the relevant local operation
is running.

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

The current Rust lockfile has no known vulnerability advisory, but Linux builds
inherit GTK3 `glib 0.18.5` through the Tauri/WebKitGTK stack. RustSec reports an
unsound `VariantStrIter` implementation for that range. The current Tauri and
Wry releases still resolve this Linux stack to GTK3, so the project must not
silently ignore the advisory or claim Linux is fully hardened until the
upstream stack moves to a fixed GLib line or a reviewed compatible patch is
available.
