# App updater

WhisperTube memakai Tauri Updater dengan `latest.json` statis. User dapat mengecek dan memasang update dari dalam aplikasi; GitHub Release hanya berfungsi sebagai tempat distribusi artifact di belakang layar.

## Alur release

1. Naikkan versi secara konsisten di `package.json`, `src-tauri/tauri.conf.json`, dan `src-tauri/Cargo.toml`.
2. Push tag aplikasi, misalnya `v0.1.2`.
3. Workflow `.github/workflows/build-application-bundles.yml` membangun installer dan updater artifact bertanda tangan.
4. Workflow mengunggah installer, file `.sig`, dan `latest.json` ke GitHub Release.
5. Aplikasi membaca `latest.json`, memverifikasi signature, lalu mengunduh dan memasang update dari Settings atau banner update.

Workflow memakai format updater native Tauri v2: Windows memakai `.exe` dengan sidecar `.exe.sig`, Linux memakai `.AppImage` dengan sidecar `.AppImage.sig`, dan macOS memakai `.app.tar.gz` dengan sidecar `.app.tar.gz.sig`. Artifact macOS diberi suffix `-x64` atau `-arm64` saat collection agar dua runner tidak bertabrakan.

## GitHub Actions secrets

Updater Tauri harus menggunakan keypair yang sama sepanjang umur instalasi aplikasi. Private key tidak boleh masuk Git.

Tambahkan repository secrets berikut:

- `TAURI_SIGNING_PRIVATE_KEY`: isi lengkap file private key yang dibuat oleh Tauri signer.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: hanya jika private key memakai password.

Key lokal yang dibuat untuk checkout ini berada di luar repository:

```text
C:\Users\AXEL\.tauri\whispertube-updater.key
```

Key tersebut saat ini dibuat tanpa password. Lindungi file itu dan buat backup aman sebelum release publik pertama. Jika private key atau password hilang, instalasi lama tidak dapat menerima update yang ditandatangani dengan benar. Jangan mengganti key setelah user mulai memakai updater kecuali memang merencanakan migrasi khusus.

## Build lokal

Build lokal biasa tetap membuat installer normal tanpa membutuhkan private key:

```powershell
.\scripts\build-windows.ps1
```

Untuk membuat artifact updater secara lokal, set private key melalui environment variable atau path, lalu aktifkan flag build:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = 'C:\Users\AXEL\.tauri\whispertube-updater.key'
$env:TAURI_CREATE_UPDATER_ARTIFACTS = 'true'
.\scripts\build-windows.ps1
```

Workflow release mengaktifkan flag tersebut otomatis. Release tidak akan lengkap jika secret private key belum diisi karena artifact updater dan signature wajib tersedia.

## Batasan penting

- Instalasi sebelum updater ini dirilis harus diperbarui sekali secara manual ke build yang sudah memiliki updater.
- Endpoint `releases/latest` mengikuti release stabil GitHub; prerelease tidak dipilih sebagai release terbaru.
- Update aplikasi mengganti bundle aplikasi. Model Whisper dan runtime yang diunduh ke app data tetap dikelola oleh aplikasi secara terpisah.
- Pada Windows, installer dapat meminta izin UAC dan aplikasi akan ditutup sebelum pemasangan selesai.
