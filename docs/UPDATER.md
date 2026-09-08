# Application Updater

WhisperTube uses the Tauri v2 updater. The updater reads a static latest.json
manifest from the stable GitHub Release and verifies each artifact with the
embedded updater public key before installation.

## Application release flow

1. Set the same application version in package.json,
   src-tauri/tauri.conf.json, and src-tauri/Cargo.toml.
2. Commit the versioned application changes on the intended release branch,
   normally main.
3. Push a new application tag such as v0.1.3.
4. build-application-bundles.yml runs its reusable quality gate, builds the
   native bundles, and collects checksums and updater signatures.
5. The workflow validates that the tag version matches all three version
   files, generates latest.json, and publishes the GitHub Release.
6. The application checks the updater endpoint, presents the available update,
   verifies the signature, and downloads and installs the update.

The application release workflow is triggered by workflow_dispatch or tags
matching v*. A manual run is intended for QA artifacts; a tag is the normal
path for a public application release.

## Updater artifact formats

The workflow uses the native Tauri updater formats:

- Windows: an .exe installer with an accompanying .exe.sig file;
- Linux: an .AppImage with an accompanying .AppImage.sig file;
- macOS: an .app.tar.gz archive with an accompanying .app.tar.gz.sig file.

The two macOS runners add -x64 and -arm64 to the collected archive names so
the Intel and Apple Silicon artifacts remain distinct in the manifest.

The published application release also contains the NSIS, DMG, Debian, and
AppImage installers plus SHA-256 sidecar files. The updater targets the
AppImage on Linux; the Debian package remains a direct-install distribution.

## Updater endpoint

The endpoint configured in src-tauri/tauri.conf.json is:

    https://github.com/mOdrA40/WhisperTube/releases/latest/download/latest.json

The releases/latest endpoint follows the stable GitHub Release and does not
select a prerelease. Until a stable application release is published, the
endpoint may legitimately return no manifest.

## Signing key and repository secret

The updater keypair must remain stable for the lifetime of installed builds.
Changing the key after users install the application requires a deliberately
planned key migration.

Configure this repository secret:

- TAURI_SIGNING_PRIVATE_KEY: the complete updater private key in the format
  expected by the Tauri signer.

Never commit the private key, put it in a workflow log, or include it in a
public issue. The public key belongs in src-tauri/tauri.conf.json.

The current release workflow does not map a signing-key password. If the
private key is changed to a password-protected key, add the matching
TAURI_SIGNING_PRIVATE_KEY_PASSWORD secret and map it into both native build
jobs before creating a release.

## Local builds

A normal Windows installer build does not create updater artifacts:

    .\scripts\build-windows.ps1

The build scripts accept an external private-key path when updater artifacts
are deliberately enabled. Keep the path outside the repository:

    $env:TAURI_CREATE_UPDATER_ARTIFACTS = 'true'
    $env:TAURI_SIGNING_PRIVATE_KEY_PATH = '<absolute path outside the repository>'
    .\scripts\build-windows.ps1

The same environment variables are supported by the macOS and Linux build
scripts. Clear the environment variables after the test build and do not
paste the private key itself into a command or log.

## Accelerator releases are separate

Accelerator packs use build-accelerator-packs.yml and tags matching
accelerators-v*. They are not application updater artifacts and must not use
the v* application tag namespace.

If a new accelerator pack is referenced by the application catalog, publish
the accelerator release first, synchronize its verified hashes into the
application source, and then produce a new application release. Published
accelerator assets must remain immutable for the lifetime of the application
build that references their hashes.

## Important limitations

- An installation created before updater support needs one manual installation
  of a build that includes the updater.
- The updater replaces the application bundle. User models and downloaded
  runtimes remain in application data and are managed separately.
- On Windows, the installer may request UAC and the application closes before
  installation completes.
- Updater signatures do not replace Windows Authenticode signing or Apple
  Developer ID signing and notarization.
