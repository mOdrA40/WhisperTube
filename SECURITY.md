# Security policy

## Reporting a vulnerability

Do not open a public issue for a security vulnerability. Send a private report to
the repository maintainer with reproduction steps, affected version, platform,
and the smallest useful log or sample. Remove cookies, tokens, private keys, and
personal data before sending any attachment.

## Local credentials

WhisperTube may use a user-selected Netscape `cookies.txt` file locally for
login-protected media. The file is not uploaded by the application and must not
be committed to this repository. Keep `.env*`, credential files, private keys,
and release signing material outside source control.

## Release trust

Tauri updater artifacts are verified with the embedded updater public key. That
does not replace OS distribution trust: Windows Authenticode signing and Apple
Developer ID signing/notarization must be configured before calling a release
production-ready.
