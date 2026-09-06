# WhisperTube 0.1

WhisperTube adalah aplikasi desktop local-first untuk mengubah video dari platform yang didukung menjadi transkrip menggunakan `yt-dlp`, FFmpeg, dan whisper.cpp. Audio serta proses transkripsi tetap berada di perangkat pengguna.

Dokumentasi utama: [README.md](README.md) (English). Dokumentasi Mandarin: [README_CN.md](README_CN.md).

## Cakupan saat ini

- Windows 10/11 x64 adalah target development dan testing utama.
- Bahasa antarmuka default adalah English. Bahasa Indonesia dan 中文（普通话） tersedia di Settings.
- Inferensi CPU disertakan dalam setup development.
- NVIDIA CUDA bersifat opsional dan diunduh terpisah supaya setup dasar tetap kecil.
- Aplikasi mendeteksi OS, arsitektur, GPU, browser, dan profil sebelum menawarkan komponen opsional.

Platform yang dicakup antara lain YouTube, TikTok, X/Twitter, Facebook, Instagram, Reddit, Twitch, Vimeo, Dailymotion, Pinterest, LinkedIn, Tumblr, Bilibili, dan VK. Dukungan mengikuti extractor yt-dlp yang terpasang dan dapat berubah saat platform memperbarui sistemnya.

## Fitur

- Tempel URL video yang didukung dan periksa metadata sebelum download.
- Video publik dan video yang memerlukan login melalui import `cookies.txt` format Netscape.
- Tidak meminta email atau password Google dan tidak menyalin cookies ke database aplikasi.
- Akses `cookies.txt` format Netscape dari berbagai keluarga browser; di macOS, Settings juga dapat mencoba session Safari yang terdeteksi secara langsung.
- Download `bestaudio/best`, normalisasi FFmpeg ke WAV PCM signed 16-bit mono 16 kHz, lalu inferensi lokal whisper.cpp.
- Model: Fast (`base`, ~142 MB), Balanced (`large-v3-turbo-q5_0`, ~547 MB), dan Accurate (`large-v3-q5_0`, ~1,1 GB).
- Verifikasi checksum model, progress, cancel, timestamp, export TXT/SRT/VTT, dan history SQLite lokal.
- Progress model dan download video menampilkan byte yang sudah dipindahkan; download model dapat dibatalkan.
- Saat transkripsi, penggunaan CPU/GPU ditampilkan jika tersedia di platform, dan speed jaringan tampil saat download.
- Workspace dapat dibersihkan dari URL, preview, dan transkrip; history bisa dihapus permanen satuan, beberapa, atau semua item yang tampil.
- Audio sementara dihapus setelah proses kecuali opsi `Simpan audio hasil proses` dinyalakan.

## Setup Windows

Pasang Node.js 22+, Rust, serta Visual Studio Build Tools 2022 dengan workload **Desktop development with C++**, MSVC, dan Windows SDK.

```powershell
cd D:\Projects\WhisperTube
.\scripts\setup-windows.ps1
.\scripts\run-dev.ps1
```

Jika Vite melaporkan port `1420` sedang dipakai, tutup proses development sebelumnya sebelum menjalankan ulang.

Model diunduh dari panel model di aplikasi dan disimpan di app data user, bukan di source tree.

## Akselerasi GPU

Buka **Settings → Hardware**. Aplikasi hanya menampilkan accelerator yang cocok dengan platform, arsitektur, dan GPU yang terdeteksi:

- Windows x64 + NVIDIA terdeteksi: CUDA dari release whisper.cpp resmi yang dipin.
- macOS: pack Apple Metal yang sesuai.
- Windows/Linux x64 + GPU terdeteksi: pack Vulkan yang sesuai dapat ditawarkan sebagai accelerator alternatif.
- Perangkat CPU-only tidak mendapat tombol download accelerator yang tidak relevan.

CUDA tidak dibundel dalam setup dasar. Aplikasi mengunduhnya ke app storage user dan memvalidasi SHA-256 sebelum digunakan.

## Alur transkripsi

1. Tempel link video yang didukung.
2. Klik **Periksa video**.
3. Pastikan metadata tampil.
4. Download dan pilih model.
5. Biarkan **Backend komputasi** pada **Auto** kecuali membutuhkan backend tertentu.
6. Pilih bahasa transkripsi atau **Deteksi otomatis**.
7. Klik **Mulai transkripsi**.

```text
Video source → yt-dlp → FFmpeg WAV 16 kHz mono → whisper.cpp
       → segments/timestamp → JSON + TXT + SRT + VTT + history
```

## Video yang memerlukan login

Login ke platform terkait terlebih dahulu. Di WhisperTube buka **Pengaturan → Akses sumber**, lalu import file `cookies.txt` format Netscape yang masih baru. Di macOS, session Safari yang terdeteksi juga dapat dicoba secara langsung. Kembali ke halaman Transkripsi dan klik **Periksa video**.

`yt-dlp` hanya membaca sesi browser pilihan saat job berjalan. Dukungan video yang memerlukan login dapat berubah ketika platform mengubah extractor, login flow, atau sistem anti-bot.

Di Windows, enkripsi browser berbasis Chromium dapat membuat yt-dlp gagal membuka profile Brave/Chrome/Edge, sehingga file cookies manual adalah jalur utama. Di macOS, penyimpanan cookies Safari mungkin memerlukan izin dari macOS. File/session hanya digunakan oleh proses WhisperTube lokal.

## Build installer

```powershell
.\scripts\build-windows.ps1
```

Installer berada di sekitar `src-tauri\target\release\bundle\`. `npm run tauri:build` adalah release build penuh: compile Rust mode release, build frontend, bundling runtime, dan membuat installer.

## Pembaruan aplikasi

WhisperTube memiliki updater di dalam aplikasi. Aplikasi memeriksa release bertanda tangan, menampilkan banner jika versi baru tersedia, lalu mengunduh dan memasang updater dari Settings tanpa user membuka GitHub. GitHub Release tetap menjadi tempat distribusi file di belakang layar, bukan backend aplikasi.

Detail konfigurasi key, secret GitHub Actions, dan alur tag release ada di [docs/UPDATER.md](docs/UPDATER.md).

## Build macOS dan Linux dari source

Build macOS/Linux harus dijalankan di host OS masing-masing karena Tauri memakai
WebView dan toolchain native. Repository sekarang menyediakan script bootstrap
dan build native.

### macOS

Pasang Xcode Command Line Tools, Homebrew, Rust, dan Node.js, lalu jalankan dari
root repository:

```bash
xcode-select --install
chmod +x scripts/setup-macos.sh scripts/build-macos.sh
./scripts/setup-macos.sh
./scripts/build-macos.sh
```

Script setup membuat runtime CPU dan Metal secara terpisah. Bundle `.app` dan
`.dmg` berada di `src-tauri/target/release/bundle/`.

### Linux (Ubuntu/Debian)

Pasang dependency Tauri:

```bash
sudo apt-get update
sudo apt-get install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf xdg-utils cmake pkg-config git nasm xz-utils
```

Kemudian:

```bash
chmod +x scripts/setup-linux.sh scripts/build-linux.sh
./scripts/setup-linux.sh
./scripts/build-linux.sh
```

Script Linux membangun FFmpeg statik yang dipin serta engine CPU, lalu membuat
bundle Debian dan AppImage di
`src-tauri/target/release/bundle/`. Distro lain membutuhkan padanan dependency
WebKitGTK, compiler, OpenSSL, FFmpeg, dan packaging-nya sendiri. Untuk
kompatibilitas glibc, build sebaiknya dilakukan pada base distro tertua yang
ingin didukung.

Workflow opsional `.github/workflows/build-application-bundles.yml` mengulang
build native tersebut di runner Windows, macOS, dan Linux GitHub. Jalankan
manual untuk mendapat artifact sementara, atau push tag aplikasi seperti
`v0.1.2` untuk menerbitkan installer native NSIS, DMG, Debian, dan AppImage
dalam GitHub Release. Artifact updater memiliki signature Tauri terpisah untuk
verifikasi update; Windows belum memiliki Authenticode signing dan macOS memakai
ad-hoc signing gratis tetapi belum dinotarize Apple pada v0.1.
Setiap installer memiliki sidecar SHA-256, dan bundle aplikasi menyertakan
license project serta third-party notices.

## Accelerator release untuk maintainer

Workflow `.github/workflows/build-accelerator-packs.yml` membuat pack Metal/Vulkan dari source resmi whisper.cpp. Repository boleh private saat development/CI, tetapi release accelerator harus public sebelum EXE dibagikan karena aplikasi tidak membawa GitHub token.

Setelah release public selesai, sinkronkan checksum ke source lalu build ulang aplikasi:

```powershell
.\scripts\sync-accelerator-hashes.ps1
.\scripts\sync-accelerator-hashes.ps1 -Apply
```

Perintah pertama hanya membaca dan menampilkan hash. Opsi `-Apply` menulis hash ke katalog aplikasi agar tombol download Metal/Vulkan aktif. Review diff dan jangan mengganti asset release setelah EXE dibangun.

## Struktur dan data lokal

- `src/components/`: komponen UI.
- `src/i18n.tsx`: terjemahan English/Indonesia/Mandarin.
- `src/assets/fonts/`: font antarmuka yang dibundel.
- `src/hooks/`: state dan action aplikasi.
- `src/services/`: batas IPC Tauri.
- `src-tauri/src/`: command dan modul domain Rust.

Model, job, export, dan `whispertube.db` disimpan di app-local-data sesuai OS.

## Batasan v0.1

- CUDA yang dipin saat ini khusus Windows x64 dengan driver NVIDIA terdeteksi.
- Metal/Vulkan memerlukan asset release GitHub publik yang cocok.
- Video yang memerlukan login bergantung pada kompatibilitas versi yt-dlp serta perubahan keamanan browser/platform.
- Belum ada playlist/batch job, speaker diarization, word-level subtitle editing, atau auto-update runtime/model.
- Build macOS/Linux sudah tersedia melalui script native dan CI. macOS memakai ad-hoc signing tetapi belum dinotarize; signing Windows/Linux dan QA lintas distro/hardware belum selesai di v0.1.
- Bootstrap macOS/Linux membangun FFmpeg statik dari source archive yang dipin; distribusi ke mesin bersih tetap membutuhkan QA dependency dan lisensi per platform.

## Lisensi

Source code WhisperTube berlisensi MIT. Runtime dan font mengikuti lisensi upstream masing-masing; lihat [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
