# WhisperTube 0.1

Desktop app lokal untuk mengubah video YouTube menjadi transcript memakai `yt-dlp + FFmpeg + whisper.cpp`.

## Target utama versi ini

- Windows 10/11 x64: **jalur development utama dan paling siap dicoba**.
- NVIDIA CUDA: opsional, dipasang terpisah agar setup CPU tidak menjadi ratusan MB.
- macOS/Linux: struktur backend sudah cross-platform dan tersedia bootstrap development, tetapi jalur packaging/distribusi belum sekeras Windows pada v0.1.

## Fitur yang sudah ada

- Paste URL YouTube lalu baca metadata sebelum download.
- Public video.
- Member-only/authenticated video melalui `yt-dlp --cookies-from-browser` dengan pilihan browser dan profile lokal. Browser yang didukung mengikuti platform: Chrome, Edge, Firefox, Brave, Chromium, Opera, Vivaldi, Whale, serta Safari di macOS. Aplikasi tidak meminta password Google.
- Download `bestaudio/best`; **tidak mengubahnya dulu menjadi MP3**.
- FFmpeg menormalisasi ke WAV PCM signed 16-bit, mono, 16 kHz.
- whisper.cpp local inference.
- Model manager:
  - Fast: `base` (~142 MB)
  - Balanced: `large-v3-turbo-q5_0` (~547 MB)
  - Accurate: `large-v3-q5_0` (~1.1 GB)
- Verifikasi SHA-1 model sesuai manifest upstream whisper.cpp sebelum model dipasang.
- CPU fallback.
- NVIDIA CUDA engine opsional.
- Auto backend: memakai CUDA bila NVIDIA + CUDA engine tersedia; jika NVIDIA ada tetapi CUDA belum siap, aplikasi meminta instalasi CUDA atau pilihan CPU secara eksplisit.
- Progress download / conversion / transcription.
- Cancel job yang membunuh child process aktif.
- Transcript bertimestamp.
- Export TXT, SRT, VTT.
- History lokal memakai SQLite.
- Audio sumber dan WAV sementara dihapus setelah selesai kecuali `Keep processed audio` dinyalakan.

---

# Windows 11 — mulai dari ZIP

## 1. Extract ZIP

Contoh:

```text
D:\Projects\WhisperTube
```

Hindari folder yang membutuhkan Administrator seperti `C:\Program Files`.

## 2. Prasyarat development

### A. Node.js

Gunakan Node.js 22 atau lebih baru.

Cek PowerShell:

```powershell
node -v
npm -v
```

### B. Rust

Install Rust dari:

```text
https://rustup.rs/
```

Setelah install, tutup/buka PowerShell dan cek:

```powershell
rustc --version
cargo --version
```

### C. Microsoft C++ Build Tools

Tauri di Windows membutuhkan Microsoft C++ Build Tools.

Install **Visual Studio Build Tools 2022** dan centang workload:

```text
Desktop development with C++
```

Pastikan komponen MSVC + Windows SDK ikut terpasang.

Windows 10/11 biasanya sudah memiliki WebView2 Runtime.

## 3. Buka PowerShell pada folder project

```powershell
cd D:\Projects\WhisperTube
```

Jika PowerShell memblokir `.ps1`, hanya untuk terminal saat ini:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
```

## 4. Jalankan bootstrap

```powershell
.\scripts\setup-windows.ps1
```

Script akan:

1. memeriksa Node/npm/Rust,
2. mengunduh `yt-dlp.exe`,
3. mengunduh FFmpeg,
4. mengunduh whisper.cpp CPU engine v1.9.1,
5. menjalankan `npm install`,
6. mengetes ketiga binary.

Runtime disimpan di:

```text
src-tauri\runtime\windows\
```

Model AI **belum** diunduh pada tahap ini; model diunduh melalui UI.

## 5. Jika punya NVIDIA GPU — opsional tetapi direkomendasikan

Pada packaged EXE, CUDA dapat dipasang langsung dari aplikasi melalui:

```text
Settings → Hardware → Install CUDA acceleration
```

WhisperTube mengunduh release CUDA `whisper.cpp` yang sudah dipin, memvalidasi
SHA-256, melakukan self-check, lalu menyimpan engine di app data user. Tidak
perlu meminta user menjalankan PowerShell. Script berikut tetap tersedia untuk
workflow development dari source:

```powershell
.\scripts\install-cuda-engine.ps1
```

Package CUDA whisper.cpp besar, sehingga sengaja tidak ikut setup dasar.

Jika dipasang dari script development, engine berada di:

```text
src-tauri\runtime\windows\cuda\
```

## Accelerator release untuk maintainer

Workflow `.github/workflows/build-accelerator-packs.yml` membangun pack Metal
dan Vulkan dari source resmi `whisper.cpp`. Jalankan workflow secara manual
untuk test artifact, atau push tag seperti `accelerators-v0.1.0` untuk membuat
GitHub Release beserta file SHA-256.

Repository boleh private selama development/CI. Sebelum EXE production
disebar, release accelerator harus public karena aplikasi user tidak membawa
GitHub token. CUDA Windows tetap memakai release upstream yang dipin.

## 6. Jalankan aplikasi development

```powershell
.\scripts\run-dev.ps1
```

atau:

```powershell
npm run tauri:dev
```

Pertama kali Rust compile akan mengompilasi dependencies Tauri/Rust.

## 7. Download model dari aplikasi

Pada panel kanan pilih model.

Rekomendasi awal:

```text
Balanced — Large V3 Turbo Q5
```

Klik `Download Balanced`. File sekitar 547 MB.

Model tersimpan di app data lokal Windows, bukan di source tree.

Jika CUDA aktif, aplikasi membaca total dan free VRAM NVIDIA untuk memberi
rekomendasi model. Aplikasi menolak kombinasi model CUDA yang melewati batas
VRAM konservatif. Batas ini adalah guardrail, bukan jaminan universal karena
driver dan aplikasi GPU lain dapat memakai VRAM.

## 8. Transkripsi pertama

1. Tempel link YouTube.
2. Klik `Check video`.
3. Pastikan metadata tampil.
4. Pilih `Balanced`.
5. `Compute backend = Auto`.
6. `Language = Auto detect`.
7. Klik `Transcribe now`.

Pipeline internal:

```text
YouTube
  ↓ yt-dlp bestaudio
original audio stream
  ↓ FFmpeg
WAV PCM s16le / mono / 16 kHz
  ↓ whisper.cpp
segments + timestamps
  ↓
JSON + TXT + SRT + VTT + SQLite history
```

## 9. Video YouTube Member

Login YouTube seperti biasa di Chrome/Edge/Firefox/Brave terlebih dahulu.

Di WhisperTube:

```text
Settings → YouTube access → Browser session
```

Pilih browser yang memiliki membership tersebut.

Lalu kembali ke Transcribe dan klik `Check video`.

WhisperTube tidak meminta email/password Google dan tidak menulis cookies ke database aplikasi. `yt-dlp` membaca browser session ketika process dijalankan.

Catatan: YouTube terus mengubah extractor, login flow, dan PO Token. Dukungan authenticated video bergantung juga pada kompatibilitas versi `yt-dlp` terbaru.

---

# Build installer Windows

Setelah development berhasil:

```powershell
.\scripts\build-windows.ps1
```

Hasil Tauri berada di sekitar:

```text
src-tauri\target\release\bundle\
```

Untuk produk yang akan didistribusikan, jangan langsung publish build development ini sebelum audit lisensi exact FFmpeg/yt-dlp binary, signing, auto-update, dan installer QA.

---

# Struktur project

```text
WhisperTube/
├─ src/
│  ├─ App.tsx              # UI dan flow frontend
│  ├─ main.tsx
│  └─ styles.css
│
├─ src-tauri/
│  ├─ src/
│  │  ├─ lib.rs            # orchestration Rust
│  │  └─ main.rs
│  ├─ capabilities/
│  ├─ runtime/             # binary runtime hasil bootstrap
│  ├─ Cargo.toml
│  └─ tauri.conf.json
│
├─ scripts/
│  ├─ setup-windows.ps1
│  ├─ install-cuda-engine.ps1
│  ├─ run-dev.ps1
│  ├─ build-windows.ps1
│  ├─ diagnose-windows.ps1
│  ├─ setup-macos.sh
│  └─ setup-linux.sh
│
├─ THIRD_PARTY_NOTICES.md
├─ docs/
│  ├─ ARCHITECTURE.md
│  └─ TROUBLESHOOTING.md
└─ README.md
```

---

# Lokasi data user

Tauri menentukan app-local-data sesuai OS. Di dalamnya WhisperTube membuat kurang lebih:

```text
models/
jobs/
whispertube.db
```

Setiap job menyimpan final transcript + exports. Audio sementara dibuang bila opsi keep audio mati.

---

# Security decisions

- URL dibatasi ke host YouTube resmi untuk command downloader.
- Frontend tidak diberi arbitrary shell execution.
- Semua process external dipanggil oleh Rust dengan argument array, bukan string command hasil concatenation.
- Model hanya dari manifest yang dikenal dan diverifikasi checksum.
- Tidak meminta Google credentials.
- Satu transcription job aktif pada satu waktu di v0.1.
- Cancel membunuh process tree di Windows memakai `taskkill /T /F`.

---

# Known limitations v0.1

1. CUDA bootstrap yang disediakan khusus Windows x64/NVIDIA.
2. AMD/Intel Vulkan belum dibundel karena official upstream Windows release belum konsisten menyediakan Vulkan binary; arsitektur backend dapat diperluas setelah engine pack Vulkan kita build sendiri di CI.
3. Member-only YouTube dapat rusak sewaktu-waktu jika YouTube mengubah authentication/PO Token; update yt-dlp adalah garis pertahanan pertama.
4. Belum ada playlist/batch jobs.
5. Belum ada speaker diarization.
6. Belum ada word-level subtitle editing/player seek.
7. macOS/Linux bootstrap ditujukan untuk development, belum installer production-grade.
8. Belum ada auto-updater komponen runtime.

Ini batasan yang sengaja dibuat eksplisit agar v0.1 tetap dapat diuji dan di-debug dengan benar.
