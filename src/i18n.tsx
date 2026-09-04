import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import type { AcceleratorInfo, ModelInfo, ProgressPayload, ProgressStage, SystemStatus } from "./types";

export type UiLanguage = "en" | "id" | "zh";

export const uiLanguageOptions: Array<{ value: UiLanguage; label: string }> = [
  { value: "en", label: "English" },
  { value: "id", label: "Bahasa Indonesia" },
  { value: "zh", label: "中文（普通话）" },
];

const STORAGE_KEY = "whispertube.ui-language";

const english = {
  "app.tagline": "Local Whisper transcription",
  "app.localInference": "Local inference",
  "nav.transcribe": "Transcribe",
  "nav.history": "History",
  "nav.settings": "Settings",
  "page.transcribe.title": "Transcribe video",
  "page.transcribe.description": "YouTube → local Whisper → transcript. Audio never leaves this device.",
  "page.history.title": "History",
  "page.history.description": "Open your local transcription results again.",
  "page.settings.title": "Settings",
  "page.settings.description": "Manage models, YouTube access, and the compute backend.",
  "status.ready": "Ready",
  "status.setupNeeded": "Setup needed",
  "hardware.cpuThreads": "CPU threads",
  "hardware.vram": "GB VRAM",
  "hardware.cpuCompute": "CPU compute",
  "hardware.notDetected": "Not detected",
  "hardware.detecting": "Detecting hardware…",
  "hardware.gpu": "GPU",
  "hardware.totalVram": "Total VRAM",
  "hardware.freeVram": "Free VRAM",
  "hardware.cudaEngine": "CUDA engine",
  "hardware.recommendation.cudaReady": "{gpu} detected with {vram}. Auto will use CUDA; {model} is recommended.",
  "hardware.recommendation.cudaInstall": "NVIDIA detected ({vram}). Install CUDA acceleration to avoid falling back to CPU.",
  "hardware.recommendation.cudaUnsupported": "NVIDIA detected, but this CUDA package is only available for Windows. Use Auto with Vulkan or CPU.",
  "hardware.recommendation.cpuStrong": "This CPU is suitable for transcription. Start with Balanced; use Fast if it feels slow.",
  "hardware.recommendation.cpuFast": "Start with Fast on this machine for a more responsive experience.",
  "hardware.recommendation.gpu": "A compatible GPU was detected. Install the available accelerator for faster inference.",
  "error.title": "Technical issue",
  "error.dismiss": "Dismiss error",
  "history.eyebrow": "Local library",
  "history.title": "Transcription history",
  "history.description": "Metadata and transcripts are stored only on this device.",
  "history.refresh": "Refresh",
  "history.emptyTitle": "No history yet",
  "history.emptyDescription": "Complete your first transcription and the result will appear here.",
  "history.start": "Start transcription",
  "history.selectAll": "Select all",
  "history.clearSelection": "Clear selection",
  "history.selected": "{count} selected",
  "history.selectItem": "Select this history item",
  "history.deleteSelected": "Delete selected",
  "history.deleteOne": "Delete this history",
  "history.confirmTitle": "Delete history",
  "history.confirmOk": "Delete",
  "history.confirmCancel": "Cancel",
  "history.confirmDelete": "Permanently delete {count} history item(s) and their transcript files? This cannot be undone.",
  "source.badge": "YouTube source",
  "source.headline": "Paste a link. We’ll handle the rest.",
  "source.description": "Public videos and Member-only videos that your browser account is authorized to access.",
  "source.placeholder": "https://www.youtube.com/watch?v=...",
  "source.checking": "Checking",
  "source.checkVideo": "Check video",
  "source.memberHint": "For Member-only videos, choose the logged-in browser session in Settings.",
  "source.access": "YouTube access",
  "source.videoReady": "Video ready",
  "source.clear": "Clear video and transcript",
  "controls.eyebrow": "Transcription",
  "controls.title": "Quality & compute",
  "controls.model": "Model",
  "controls.cudaRequirement": "CUDA ≥ {memory}",
  "controls.downloadingModel": "Downloading {percent}%",
  "controls.cancelModel": "Cancel model download",
  "controls.downloadModel": "Download {model}",
  "controls.nvidiaHint": "NVIDIA detected. Install CUDA so transcription uses the GPU instead of CPU.",
  "controls.installCuda": "Install CUDA acceleration (~678 MB)",
  "controls.installingCuda": "Installing CUDA {percent}%",
  "controls.cancelCuda": "Cancel CUDA download",
  "controls.acceleratorInfo": "{description}. Download it from the project accelerator release.",
  "controls.installAccelerator": "Install {accelerator}",
  "controls.installingAccelerator": "Installing {accelerator} {percent}%",
  "controls.cancelAccelerator": "Cancel accelerator download",
  "controls.language": "Transcription language",
  "controls.computeBackend": "Compute backend",
  "controls.keepAudio": "Keep processed audio",
  "controls.keepAudioHint": "Off by default to save storage.",
  "controls.transcribe": "Transcribe now",
  "controls.runtimeHint": "Runtime is not ready. Run the setup script first.",
  "language.auto": "Auto detect",
  "language.en": "English",
  "language.id": "Indonesian",
  "language.zh": "Chinese (Mandarin)",
  "language.ja": "Japanese",
  "language.ko": "Korean",
  "backend.auto": "Auto — recommended",
  "backend.cpu": "CPU",
  "backend.cuda": "NVIDIA CUDA",
  "backend.cudaUnavailable": "NVIDIA CUDA — unavailable",
  "backend.notInstalled": "{accelerator} — not installed",
  "settings.eyebrow": "Authentication",
  "settings.accessTitle": "YouTube access",
  "settings.accessIntro": "WhisperTube never asks for your Google email or password. For Member-only videos, yt-dlp reads a local browser session that is already signed in.",
  "settings.browserSession": "Browser session",
  "settings.browserDetectNote": "Only supported browsers detected on this device are listed. Choose any listed browser and profile that is signed in to YouTube.",
  "settings.publicOnly": "Public videos only",
  "settings.browserProfile": "Browser profile",
  "settings.default": "Default",
  "settings.profileNote": "Profiles are detected from local browser configuration. Login access is checked when you check a video.",
  "settings.noBrowsers": "No supported browser session was detected. Public videos still work without cookies. Supported browsers are detected automatically when available.",
  "settings.cookies": "The app does not store cookies. yt-dlp reads the selected browser session only while the job is running.",
  "settings.interfaceLanguage": "Interface language",
  "settings.interfaceLanguageNote": "Choose the language used by WhisperTube. English is the default.",
  "settings.hardwareEyebrow": "Hardware",
  "settings.computeTitle": "Compute engine",
  "settings.cpuEngine": "CPU engine",
  "settings.installed": "Installed",
  "settings.missing": "Missing",
  "settings.unavailable": "Unavailable",
  "settings.gpu": "GPU",
  "settings.totalVram": "Total VRAM",
  "settings.freeVram": "Free VRAM",
  "settings.cudaEngine": "CUDA engine",
  "settings.optional": "Optional",
  "settings.cudaInfo": "The CUDA engine is downloaded from an official whisper.cpp release and stored in app storage (~678 MB).",
  "settings.installCuda": "Install CUDA acceleration",
  "settings.installingCuda": "Installing CUDA {percent}%",
  "settings.cudaInstalled": "CUDA engine installed",
  "settings.cancelCuda": "Cancel CUDA download",
  "settings.cudaUnavailable": "CUDA is available only for a detected NVIDIA GPU on supported Windows builds. This device can use the other compatible backend or CPU.",
  "settings.noGpu": "No compatible GPU was detected. CPU inference remains available.",
  "settings.acceleratorInstalled": "Installed",
  "settings.acceleratorInstall": "Install",
  "settings.acceleratorInstalling": "{percent}%",
  "settings.acceleratorInfo": "Available for this operating system and detected GPU.",
  "settings.storageEyebrow": "Storage",
  "settings.modelsTitle": "Whisper models",
  "settings.deleteModel": "Delete model",
  "settings.download": "Download",
  "settings.runtimeEyebrow": "Runtime",
  "settings.externalTitle": "External components",
  "settings.ready": "Ready",
  "settings.recheck": "Re-check components",
  "accelerator.metal": "Apple Metal",
  "accelerator.metalDescription": "Apple Metal GPU acceleration for macOS",
  "accelerator.vulkan": "Vulkan",
  "accelerator.vulkanDescription": "Cross-vendor Vulkan GPU acceleration",
  "model.fast.label": "Fast",
  "model.fast.description": "Lightweight for CPUs and simpler laptops",
  "model.balanced.label": "Balanced",
  "model.balanced.description": "Default: fast and accurate for general use",
  "model.accurate.label": "Accurate",
  "model.accurate.description": "Higher accuracy, with more compute and time required",
  "progress.processing": "Processing",
  "progress.idle": "Ready",
  "progress.downloading": "Downloading audio",
  "progress.converting": "Preparing audio",
  "progress.transcribing": "Transcribing",
  "progress.finalizing": "Saving result",
  "progress.done": "Complete",
  "progress.error": "Failed",
  "progress.local": "Processing locally…",
  "progress.unknownSize": "size unknown",
  "progress.startingJob": "Starting the transcription job…",
  "progress.preparingDownload": "Preparing the download…",
  "progress.downloadingAudio": "Downloading the best available audio from YouTube…",
  "progress.normalizingAudio": "Preparing audio for Whisper…",
  "progress.convertingAudio": "Converting to 16 kHz mono PCM…",
  "progress.loadingModel": "Loading the Whisper model…",
  "progress.whisperVia": "Whisper is running via {backend}…",
  "progress.finalizingResult": "Finalizing the transcript and saving history…",
  "progress.complete": "Transcription complete.",
  "progress.autoBackend": "Auto backend",
  "progress.cancel": "Cancel",
  "transcript.complete": "Transcription complete",
  "transcript.copied": "Copied",
  "transcript.copy": "Copy",
  "transcript.search": "Search the transcript…",
  "transcript.segments": "segments",
  "transcript.empty": "No matching transcript segments.",
  "error.emptyUrl": "Paste a YouTube URL first.",
  "error.installerBusy": "Another runtime installer is running. Wait for it to finish or cancel it first.",
  "error.videoCheckFirst": "Check the video first.",
  "error.modelNotDownloaded": "The selected model has not been downloaded.",
  "error.runtimeIncomplete": "The runtime is incomplete. Run the setup script first.",
  "error.cudaRequired": "CUDA acceleration is not installed. Install CUDA first so the job does not fall back to CPU.",
  "error.cudaUnavailable": "CUDA requires a detected NVIDIA GPU and is only available on supported Windows builds.",
  "error.vramUnreadable": "NVIDIA VRAM could not be read. Close other GPU-heavy apps and re-check components.",
  "error.vramTooLow": "{model} needs about {required} of free VRAM, but only {available} is available.",
  "error.acceleratorMissing": "{accelerator} is not installed. Install it from Settings first.",
} as const;

export type TranslationKey = keyof typeof english;
export type Translate = (
  key: TranslationKey,
  variables?: Record<string, string | number>,
) => string;

const messages: Record<UiLanguage, Record<TranslationKey, string>> = {
  en: english,
  id: {
    "app.tagline": "Transkripsi Whisper lokal",
    "app.localInference": "Inferensi lokal",
    "nav.transcribe": "Transkripsi",
    "nav.history": "Riwayat",
    "nav.settings": "Pengaturan",
    "page.transcribe.title": "Transkripsi video",
    "page.transcribe.description": "YouTube → Whisper lokal → transkrip. Audio tidak pernah meninggalkan perangkat ini.",
    "page.history.title": "Riwayat",
    "page.history.description": "Buka kembali hasil transkripsi lokal.",
    "page.settings.title": "Pengaturan",
    "page.settings.description": "Atur model, akses YouTube, dan backend komputasi.",
    "status.ready": "Siap",
    "status.setupNeeded": "Perlu setup",
    "hardware.cpuThreads": "thread CPU",
    "hardware.vram": "GB VRAM",
    "hardware.cpuCompute": "Komputasi CPU",
    "hardware.notDetected": "Tidak terdeteksi",
    "hardware.detecting": "Mendeteksi hardware…",
    "hardware.gpu": "GPU",
    "hardware.totalVram": "Total VRAM",
    "hardware.freeVram": "VRAM bebas",
    "hardware.cudaEngine": "Engine CUDA",
    "hardware.recommendation.cudaReady": "{gpu} terdeteksi dengan {vram}. Auto akan memakai CUDA; {model} direkomendasikan.",
    "hardware.recommendation.cudaInstall": "NVIDIA terdeteksi ({vram}). Pasang akselerasi CUDA agar tidak kembali ke CPU.",
    "hardware.recommendation.cudaUnsupported": "NVIDIA terdeteksi, tetapi paket CUDA ini hanya tersedia untuk Windows. Gunakan Auto dengan Vulkan atau CPU.",
    "hardware.recommendation.cpuStrong": "CPU ini cukup untuk transkripsi. Mulai dengan Balanced; gunakan Fast jika terasa lambat.",
    "hardware.recommendation.cpuFast": "Mulai dengan Fast agar aplikasi tetap responsif di mesin ini.",
    "hardware.recommendation.gpu": "GPU yang kompatibel terdeteksi. Pasang akselerator yang tersedia untuk inferensi lebih cepat.",
    "error.title": "Kendala teknis",
    "error.dismiss": "Tutup pesan error",
    "history.eyebrow": "Pustaka lokal",
    "history.title": "Riwayat transkripsi",
    "history.description": "Metadata dan transkrip hanya disimpan di komputer ini.",
    "history.refresh": "Segarkan",
    "history.emptyTitle": "Belum ada riwayat",
    "history.emptyDescription": "Selesaikan transkripsi pertama dan hasilnya akan muncul di sini.",
    "history.start": "Mulai transkripsi",
    "history.selectAll": "Pilih semua",
    "history.clearSelection": "Batalkan pilihan",
    "history.selected": "{count} dipilih",
    "history.selectItem": "Pilih riwayat ini",
    "history.deleteSelected": "Hapus yang dipilih",
    "history.deleteOne": "Hapus riwayat ini",
    "history.confirmTitle": "Hapus riwayat",
    "history.confirmOk": "Hapus",
    "history.confirmCancel": "Batal",
    "history.confirmDelete": "Hapus permanen {count} riwayat beserta file transkripnya? Tindakan ini tidak dapat dibatalkan.",
    "source.badge": "Sumber YouTube",
    "source.headline": "Tempel link. Sisanya kami urus.",
    "source.description": "Video publik dan video Member-only yang memang boleh diakses oleh akun browser kamu.",
    "source.placeholder": "https://www.youtube.com/watch?v=...",
    "source.checking": "Memeriksa",
    "source.checkVideo": "Periksa video",
    "source.memberHint": "Untuk video Member-only, pilih sesi browser yang sudah login di Pengaturan.",
    "source.access": "Akses YouTube",
    "source.videoReady": "Video siap",
    "source.clear": "Bersihkan video dan transkrip",
    "controls.eyebrow": "Transkripsi",
    "controls.title": "Kualitas & komputasi",
    "controls.model": "Model",
    "controls.cudaRequirement": "CUDA ≥ {memory}",
    "controls.downloadingModel": "Mengunduh {percent}%",
    "controls.cancelModel": "Batalkan unduhan model",
    "controls.downloadModel": "Unduh {model}",
    "controls.nvidiaHint": "NVIDIA terdeteksi. Pasang CUDA agar transkripsi memakai GPU, bukan CPU.",
    "controls.installCuda": "Pasang akselerasi CUDA (~678 MB)",
    "controls.installingCuda": "Memasang CUDA {percent}%",
    "controls.cancelCuda": "Batalkan unduhan CUDA",
    "controls.acceleratorInfo": "{description}. Unduh dari release accelerator project.",
    "controls.installAccelerator": "Pasang {accelerator}",
    "controls.installingAccelerator": "Memasang {accelerator} {percent}%",
    "controls.cancelAccelerator": "Batalkan unduhan accelerator",
    "controls.language": "Bahasa transkripsi",
    "controls.computeBackend": "Backend komputasi",
    "controls.keepAudio": "Simpan audio hasil proses",
    "controls.keepAudioHint": "Nonaktif secara default untuk menghemat storage.",
    "controls.transcribe": "Mulai transkripsi",
    "controls.runtimeHint": "Runtime belum siap. Jalankan script setup terlebih dahulu.",
    "language.auto": "Deteksi otomatis",
    "language.en": "Inggris",
    "language.id": "Indonesia",
    "language.zh": "Mandarin (Tionghoa)",
    "language.ja": "Jepang",
    "language.ko": "Korea",
    "backend.auto": "Auto — direkomendasikan",
    "backend.cpu": "CPU",
    "backend.cuda": "NVIDIA CUDA",
    "backend.cudaUnavailable": "NVIDIA CUDA — tidak tersedia",
    "backend.notInstalled": "{accelerator} — belum terpasang",
    "settings.eyebrow": "Autentikasi",
    "settings.accessTitle": "Akses YouTube",
    "settings.accessIntro": "WhisperTube tidak pernah meminta email atau password Google. Untuk video Member-only, yt-dlp membaca sesi browser lokal yang sudah login.",
    "settings.browserSession": "Sesi browser",
    "settings.browserDetectNote": "Hanya browser yang didukung dan terdeteksi di perangkat ini yang ditampilkan. Pilih browser dan profil yang sudah login ke YouTube.",
    "settings.publicOnly": "Video publik saja",
    "settings.browserProfile": "Profil browser",
    "settings.default": "Default",
    "settings.profileNote": "Profil dibaca dari konfigurasi browser lokal. Akses login diperiksa saat kamu memeriksa video.",
    "settings.noBrowsers": "Tidak ada sesi browser yang didukung dan terdeteksi. Video publik tetap bisa diproses tanpa cookies. Browser yang didukung akan terdeteksi otomatis jika tersedia.",
    "settings.cookies": "Aplikasi tidak menyimpan cookies. yt-dlp hanya membaca sesi browser yang dipilih selama job berjalan.",
    "settings.interfaceLanguage": "Bahasa antarmuka",
    "settings.interfaceLanguageNote": "Pilih bahasa yang digunakan WhisperTube. Default-nya English.",
    "settings.hardwareEyebrow": "Hardware",
    "settings.computeTitle": "Engine komputasi",
    "settings.cpuEngine": "Engine CPU",
    "settings.installed": "Terpasang",
    "settings.missing": "Tidak ada",
    "settings.unavailable": "Tidak tersedia",
    "settings.gpu": "GPU",
    "settings.totalVram": "Total VRAM",
    "settings.freeVram": "VRAM bebas",
    "settings.cudaEngine": "Engine CUDA",
    "settings.optional": "Opsional",
    "settings.cudaInfo": "Engine CUDA diunduh dari release resmi whisper.cpp dan disimpan di storage aplikasi (~678 MB).",
    "settings.installCuda": "Pasang akselerasi CUDA",
    "settings.installingCuda": "Memasang CUDA {percent}%",
    "settings.cudaInstalled": "Engine CUDA terpasang",
    "settings.cancelCuda": "Batalkan unduhan CUDA",
    "settings.cudaUnavailable": "CUDA hanya tersedia untuk GPU NVIDIA yang terdeteksi pada build Windows yang didukung. Perangkat ini dapat memakai backend kompatibel lain atau CPU.",
    "settings.noGpu": "Tidak ada GPU kompatibel yang terdeteksi. Inferensi CPU tetap tersedia.",
    "settings.acceleratorInstalled": "Terpasang",
    "settings.acceleratorInstall": "Pasang",
    "settings.acceleratorInstalling": "{percent}%",
    "settings.acceleratorInfo": "Tersedia untuk sistem operasi dan GPU yang terdeteksi.",
    "settings.storageEyebrow": "Storage",
    "settings.modelsTitle": "Model Whisper",
    "settings.deleteModel": "Hapus model",
    "settings.download": "Unduh",
    "settings.runtimeEyebrow": "Runtime",
    "settings.externalTitle": "Komponen eksternal",
    "settings.ready": "Siap",
    "settings.recheck": "Periksa ulang komponen",
    "accelerator.metal": "Apple Metal",
    "accelerator.metalDescription": "Akselerasi GPU Apple Metal untuk macOS",
    "accelerator.vulkan": "Vulkan",
    "accelerator.vulkanDescription": "Akselerasi GPU Vulkan lintas vendor",
    "model.fast.label": "Fast",
    "model.fast.description": "Ringan untuk CPU dan laptop sederhana",
    "model.balanced.label": "Balanced",
    "model.balanced.description": "Default: cepat dan akurat untuk penggunaan umum",
    "model.accurate.label": "Accurate",
    "model.accurate.description": "Akurasi lebih tinggi, membutuhkan komputasi dan waktu lebih banyak",
    "progress.processing": "Memproses",
    "progress.idle": "Siap",
    "progress.downloading": "Mengunduh audio",
    "progress.converting": "Menyiapkan audio",
    "progress.transcribing": "Mentranskripsi",
    "progress.finalizing": "Menyimpan hasil",
    "progress.done": "Selesai",
    "progress.error": "Gagal",
    "progress.local": "Memproses secara lokal…",
    "progress.unknownSize": "ukuran tidak diketahui",
    "progress.startingJob": "Memulai job transkripsi…",
    "progress.preparingDownload": "Menyiapkan unduhan…",
    "progress.downloadingAudio": "Mengunduh audio terbaik yang tersedia dari YouTube…",
    "progress.normalizingAudio": "Menyiapkan audio untuk Whisper…",
    "progress.convertingAudio": "Mengonversi ke PCM mono 16 kHz…",
    "progress.loadingModel": "Memuat model Whisper…",
    "progress.whisperVia": "Whisper berjalan melalui {backend}…",
    "progress.finalizingResult": "Merapikan transkrip dan menyimpan riwayat…",
    "progress.complete": "Transkripsi selesai.",
    "progress.autoBackend": "Backend Auto",
    "progress.cancel": "Batalkan",
    "transcript.complete": "Transkripsi selesai",
    "transcript.copied": "Tersalin",
    "transcript.copy": "Salin",
    "transcript.search": "Cari di transkrip…",
    "transcript.segments": "segmen",
    "transcript.empty": "Tidak ada segmen transkrip yang cocok.",
    "error.emptyUrl": "Tempel URL YouTube terlebih dahulu.",
    "error.installerBusy": "Installer runtime lain sedang berjalan. Tunggu sampai selesai atau batalkan terlebih dahulu.",
    "error.videoCheckFirst": "Periksa video terlebih dahulu.",
    "error.modelNotDownloaded": "Model yang dipilih belum diunduh.",
    "error.runtimeIncomplete": "Runtime belum lengkap. Jalankan script setup terlebih dahulu.",
    "error.cudaRequired": "Akselerasi CUDA belum terpasang. Pasang CUDA terlebih dahulu agar job tidak kembali ke CPU.",
    "error.cudaUnavailable": "CUDA membutuhkan GPU NVIDIA yang terdeteksi dan hanya tersedia pada build Windows yang didukung.",
    "error.vramUnreadable": "VRAM NVIDIA tidak terbaca. Tutup aplikasi yang memakai GPU lalu periksa ulang komponen.",
    "error.vramTooLow": "{model} membutuhkan sekitar {required} VRAM bebas, tetapi yang tersedia hanya {available}.",
    "error.acceleratorMissing": "{accelerator} belum terpasang. Pasang dari Pengaturan terlebih dahulu.",
  },
  zh: {
    "app.tagline": "本地 Whisper 转录",
    "app.localInference": "本地推理",
    "nav.transcribe": "转录",
    "nav.history": "历史记录",
    "nav.settings": "设置",
    "page.transcribe.title": "转录视频",
    "page.transcribe.description": "YouTube → 本地 Whisper → 转录文本。音频不会离开此设备。",
    "page.history.title": "历史记录",
    "page.history.description": "重新打开本地转录结果。",
    "page.settings.title": "设置",
    "page.settings.description": "管理模型、YouTube 访问权限和计算后端。",
    "status.ready": "就绪",
    "status.setupNeeded": "需要设置",
    "hardware.cpuThreads": "CPU 线程",
    "hardware.vram": "GB 显存",
    "hardware.cpuCompute": "CPU 计算",
    "hardware.notDetected": "未检测到",
    "hardware.detecting": "正在检测硬件…",
    "hardware.gpu": "GPU",
    "hardware.totalVram": "总显存",
    "hardware.freeVram": "可用显存",
    "hardware.cudaEngine": "CUDA 引擎",
    "hardware.recommendation.cudaReady": "检测到 {gpu}，可用显存为 {vram}。自动模式将使用 CUDA；推荐 {model}。",
    "hardware.recommendation.cudaInstall": "检测到 NVIDIA（{vram}）。安装 CUDA 加速以避免退回 CPU。",
    "hardware.recommendation.cudaUnsupported": "检测到 NVIDIA，但此 CUDA 软件包仅适用于 Windows。请使用带 Vulkan 的自动模式或 CPU。",
    "hardware.recommendation.cpuStrong": "这台电脑的 CPU 适合转录。建议从 Balanced 开始；如果速度较慢可使用 Fast。",
    "hardware.recommendation.cpuFast": "建议在这台设备上使用 Fast，以保持更好的响应速度。",
    "hardware.recommendation.gpu": "检测到兼容的 GPU。安装可用的加速器即可提高推理速度。",
    "error.title": "技术问题",
    "error.dismiss": "关闭错误提示",
    "history.eyebrow": "本地库",
    "history.title": "转录历史",
    "history.description": "元数据和转录文本只保存在此设备上。",
    "history.refresh": "刷新",
    "history.emptyTitle": "还没有历史记录",
    "history.emptyDescription": "完成第一次转录后，结果会显示在这里。",
    "history.start": "开始转录",
    "history.selectAll": "全选",
    "history.clearSelection": "取消选择",
    "history.selected": "已选择 {count} 项",
    "history.selectItem": "选择此历史记录",
    "history.deleteSelected": "删除所选项目",
    "history.deleteOne": "删除此历史记录",
    "history.confirmTitle": "删除历史记录",
    "history.confirmOk": "删除",
    "history.confirmCancel": "取消",
    "history.confirmDelete": "永久删除 {count} 条历史记录及其转录文件？此操作无法撤销。",
    "source.badge": "YouTube 来源",
    "source.headline": "粘贴链接，剩下的交给我们。",
    "source.description": "支持公开视频，以及浏览器账号有权限访问的会员专属视频。",
    "source.placeholder": "https://www.youtube.com/watch?v=...",
    "source.checking": "检查中",
    "source.checkVideo": "检查视频",
    "source.memberHint": "对于会员专属视频，请在设置中选择已登录的浏览器会话。",
    "source.access": "YouTube 访问",
    "source.videoReady": "视频已就绪",
    "source.clear": "清除视频和转录文本",
    "controls.eyebrow": "转录",
    "controls.title": "质量与计算",
    "controls.model": "模型",
    "controls.cudaRequirement": "CUDA ≥ {memory}",
    "controls.downloadingModel": "正在下载 {percent}%",
    "controls.cancelModel": "取消模型下载",
    "controls.downloadModel": "下载 {model}",
    "controls.nvidiaHint": "检测到 NVIDIA。安装 CUDA 后，转录会使用 GPU 而不是 CPU。",
    "controls.installCuda": "安装 CUDA 加速（约 678 MB）",
    "controls.installingCuda": "正在安装 CUDA {percent}%",
    "controls.cancelCuda": "取消 CUDA 下载",
    "controls.acceleratorInfo": "{description}。请从项目的加速器 release 下载。",
    "controls.installAccelerator": "安装 {accelerator}",
    "controls.installingAccelerator": "正在安装 {accelerator} {percent}%",
    "controls.cancelAccelerator": "取消加速器下载",
    "controls.language": "转录语言",
    "controls.computeBackend": "计算后端",
    "controls.keepAudio": "保留处理后的音频",
    "controls.keepAudioHint": "默认关闭，以节省存储空间。",
    "controls.transcribe": "立即转录",
    "controls.runtimeHint": "运行环境尚未就绪。请先运行设置脚本。",
    "language.auto": "自动检测",
    "language.en": "英语",
    "language.id": "印度尼西亚语",
    "language.zh": "中文（普通话）",
    "language.ja": "日语",
    "language.ko": "韩语",
    "backend.auto": "自动 — 推荐",
    "backend.cpu": "CPU",
    "backend.cuda": "NVIDIA CUDA",
    "backend.cudaUnavailable": "NVIDIA CUDA — 不可用",
    "backend.notInstalled": "{accelerator} — 未安装",
    "settings.eyebrow": "身份验证",
    "settings.accessTitle": "YouTube 访问",
    "settings.accessIntro": "WhisperTube 不会要求你的 Google 邮箱或密码。对于会员专属视频，yt-dlp 只会读取已登录的本地浏览器会话。",
    "settings.browserSession": "浏览器会话",
    "settings.browserDetectNote": "这里只显示设备上检测到的受支持浏览器。请选择已登录 YouTube 的浏览器和配置文件。",
    "settings.publicOnly": "仅公开视频",
    "settings.browserProfile": "浏览器配置文件",
    "settings.default": "默认",
    "settings.profileNote": "配置文件来自本地浏览器配置。检查视频时才会验证登录状态。",
    "settings.noBrowsers": "未检测到受支持的浏览器会话。公开视频无需 cookies 仍可处理。应用会自动显示设备上可用的受支持浏览器。",
    "settings.cookies": "应用不会保存 cookies。yt-dlp 只在任务运行期间读取选中的浏览器会话。",
    "settings.interfaceLanguage": "界面语言",
    "settings.interfaceLanguageNote": "选择 WhisperTube 使用的语言。默认语言为 English。",
    "settings.hardwareEyebrow": "硬件",
    "settings.computeTitle": "计算引擎",
    "settings.cpuEngine": "CPU 引擎",
    "settings.installed": "已安装",
    "settings.missing": "缺少",
    "settings.unavailable": "不可用",
    "settings.gpu": "GPU",
    "settings.totalVram": "总显存",
    "settings.freeVram": "可用显存",
    "settings.cudaEngine": "CUDA 引擎",
    "settings.optional": "可选",
    "settings.cudaInfo": "CUDA 引擎来自官方 whisper.cpp release，并安装到应用存储中（约 678 MB）。",
    "settings.installCuda": "安装 CUDA 加速",
    "settings.installingCuda": "正在安装 CUDA {percent}%",
    "settings.cudaInstalled": "CUDA 引擎已安装",
    "settings.cancelCuda": "取消 CUDA 下载",
    "settings.cudaUnavailable": "CUDA 仅适用于受支持的 Windows 构建版本，并且需要检测到 NVIDIA GPU。此设备可以使用其他兼容后端或 CPU。",
    "settings.noGpu": "未检测到兼容的 GPU。仍可使用 CPU 推理。",
    "settings.acceleratorInstalled": "已安装",
    "settings.acceleratorInstall": "安装",
    "settings.acceleratorInstalling": "{percent}%",
    "settings.acceleratorInfo": "适用于此操作系统以及检测到的 GPU。",
    "settings.storageEyebrow": "存储",
    "settings.modelsTitle": "Whisper 模型",
    "settings.deleteModel": "删除模型",
    "settings.download": "下载",
    "settings.runtimeEyebrow": "运行环境",
    "settings.externalTitle": "外部组件",
    "settings.ready": "就绪",
    "settings.recheck": "重新检查组件",
    "accelerator.metal": "Apple Metal",
    "accelerator.metalDescription": "macOS 的 Apple Metal GPU 加速",
    "accelerator.vulkan": "Vulkan",
    "accelerator.vulkanDescription": "跨厂商 Vulkan GPU 加速",
    "model.fast.label": "Fast",
    "model.fast.description": "适合 CPU 和基础笔记本的轻量模型",
    "model.balanced.label": "Balanced",
    "model.balanced.description": "默认选择：速度与准确度兼顾，适合一般用途",
    "model.accurate.label": "Accurate",
    "model.accurate.description": "准确度更高，但需要更多计算资源和时间",
    "progress.processing": "处理中",
    "progress.idle": "就绪",
    "progress.downloading": "正在下载音频",
    "progress.converting": "正在准备音频",
    "progress.transcribing": "正在转录",
    "progress.finalizing": "正在保存结果",
    "progress.done": "完成",
    "progress.error": "失败",
    "progress.local": "正在本地处理…",
    "progress.unknownSize": "大小未知",
    "progress.startingJob": "正在启动转录任务…",
    "progress.preparingDownload": "正在准备下载…",
    "progress.downloadingAudio": "正在从 YouTube 下载最佳可用音频…",
    "progress.normalizingAudio": "正在为 Whisper 准备音频…",
    "progress.convertingAudio": "正在转换为 16 kHz 单声道 PCM…",
    "progress.loadingModel": "正在加载 Whisper 模型…",
    "progress.whisperVia": "Whisper 正在通过 {backend} 运行…",
    "progress.finalizingResult": "正在整理转录文本并保存历史记录…",
    "progress.complete": "转录完成。",
    "progress.autoBackend": "自动后端",
    "progress.cancel": "取消",
    "transcript.complete": "转录完成",
    "transcript.copied": "已复制",
    "transcript.copy": "复制",
    "transcript.search": "搜索转录文本…",
    "transcript.segments": "段",
    "transcript.empty": "没有匹配的转录片段。",
    "error.emptyUrl": "请先粘贴 YouTube URL。",
    "error.installerBusy": "另一个运行环境安装程序正在运行。请等待完成或先取消。",
    "error.videoCheckFirst": "请先检查视频。",
    "error.modelNotDownloaded": "所选模型尚未下载。",
    "error.runtimeIncomplete": "运行环境不完整。请先运行设置脚本。",
    "error.cudaRequired": "尚未安装 CUDA 加速。请先安装 CUDA，以免任务退回 CPU。",
    "error.cudaUnavailable": "CUDA 需要检测到 NVIDIA GPU，并且仅适用于受支持的 Windows 构建版本。",
    "error.vramUnreadable": "无法读取 NVIDIA 显存。请关闭其他高 GPU 占用应用，然后重新检查组件。",
    "error.vramTooLow": "{model} 需要约 {required} 可用显存，但当前只有 {available}。",
    "error.acceleratorMissing": "{accelerator} 尚未安装。请先从设置中安装。",
  },
};

const modelTranslationKeys: Record<string, { label: TranslationKey; description: TranslationKey }> = {
  base: { label: "model.fast.label", description: "model.fast.description" },
  "large-v3-turbo-q5_0": { label: "model.balanced.label", description: "model.balanced.description" },
  "large-v3-q5_0": { label: "model.accurate.label", description: "model.accurate.description" },
};

type I18nContextValue = {
  language: UiLanguage;
  setLanguage: (language: UiLanguage) => void;
  t: Translate;
};

const I18nContext = createContext<I18nContextValue | null>(null);

function readInitialLanguage(): UiLanguage {
  if (typeof window === "undefined") return "en";
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    return stored === "id" || stored === "zh" ? stored : "en";
  } catch {
    return "en";
  }
}

function interpolate(template: string, variables?: Record<string, string | number>) {
  return template.replace(/\{(\w+)\}/g, (placeholder, name: string) => {
    const value = variables?.[name];
    return value === undefined ? placeholder : String(value);
  });
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [language, setLanguageState] = useState<UiLanguage>(readInitialLanguage);

  const setLanguage = useCallback((nextLanguage: UiLanguage) => {
    setLanguageState(nextLanguage);
    try {
      window.localStorage.setItem(STORAGE_KEY, nextLanguage);
    } catch {
      // The app still works if persistent browser storage is unavailable.
    }
  }, []);

  useEffect(() => {
    document.documentElement.lang = language === "zh" ? "zh-CN" : language;
  }, [language]);

  const t = useCallback<Translate>((key, variables) => {
    return interpolate(messages[language][key] ?? messages.en[key], variables);
  }, [language]);

  const value = useMemo(() => ({ language, setLanguage, t }), [language, setLanguage, t]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n() {
  const context = useContext(I18nContext);
  if (!context) throw new Error("useI18n must be used inside I18nProvider");
  return context;
}

export function getModelCopy(model: ModelInfo, t: Translate) {
  const keys = modelTranslationKeys[model.id];
  return keys
    ? { label: t(keys.label), description: t(keys.description) }
    : { label: model.label, description: model.description };
}

export function getModelLabel(modelId: string, t: Translate) {
  const keys = modelTranslationKeys[modelId];
  return keys ? t(keys.label) : modelId;
}

export function getAcceleratorCopy(accelerator: Pick<AcceleratorInfo, "backend" | "label" | "description">, t: Translate) {
  if (accelerator.backend === "metal") {
    return { label: t("accelerator.metal"), description: t("accelerator.metalDescription") };
  }
  if (accelerator.backend === "vulkan") {
    return { label: t("accelerator.vulkan"), description: t("accelerator.vulkanDescription") };
  }
  return { label: accelerator.label, description: accelerator.description };
}

export function getRecommendation(system: SystemStatus | null, t: Translate) {
  if (!system) return t("hardware.detecting");
  const model = getModelLabel(system.recommendedModelId, t);
  const vram = formatMemoryText(system.gpuFreeMemoryMb ?? system.gpuMemoryMb, t("hardware.notDetected"));
  if (system.nvidia && system.cudaSupported && system.cudaEngine) {
    return t("hardware.recommendation.cudaReady", {
      gpu: system.gpuName ?? "NVIDIA GPU",
      vram,
      model,
    });
  }
  if (system.nvidia && system.cudaSupported) {
    return t("hardware.recommendation.cudaInstall", { vram });
  }
  if (system.nvidia && !system.cudaSupported) {
    return t("hardware.recommendation.cudaUnsupported");
  }
  if (system.gpuName && system.accelerators.some((accelerator) => accelerator.supported)) {
    return t("hardware.recommendation.gpu");
  }
  return system.cpuThreads >= 12
    ? t("hardware.recommendation.cpuStrong")
    : t("hardware.recommendation.cpuFast");
}

function formatMemoryText(megabytes: number | null | undefined, fallback: string) {
  if (megabytes === null || megabytes === undefined || megabytes <= 0) return fallback;
  return `${(megabytes / 1024).toFixed(1)} GB`;
}

const stageKeys: Record<ProgressStage, TranslationKey> = {
  idle: "progress.idle",
  downloading: "progress.downloading",
  converting: "progress.converting",
  transcribing: "progress.transcribing",
  finalizing: "progress.finalizing",
  done: "progress.done",
  error: "progress.error",
};

export function getProgressStageLabel(stage: ProgressStage, t: Translate) {
  return t(stageKeys[stage]);
}

export function getProgressMessage(
  progress: Pick<ProgressPayload, "stage" | "message" | "backend">,
  requestedBackend: string,
  t: Translate,
) {
  const rawMessage = progress.message.trim();
  const normalizedMessage = rawMessage.toLowerCase();
  const actualBackend = progress.backend ?? requestedBackend;

  if (normalizedMessage.includes("menyiapkan download")) return t("progress.preparingDownload");
  if (normalizedMessage.includes("mengunduh best available")) return t("progress.downloadingAudio");
  if (normalizedMessage.includes("menormalisasi audio")) return t("progress.normalizingAudio");
  if (normalizedMessage.includes("konversi ke pcm")) return t("progress.convertingAudio");
  if (normalizedMessage.includes("memuat model whisper")) return t("progress.loadingModel");
  if (normalizedMessage.includes("merapikan transcript")) return t("progress.finalizingResult");
  if (normalizedMessage.includes("transkripsi selesai")) return t("progress.complete");
  if (progress.stage === "transcribing" && progress.backend) {
    return t("progress.whisperVia", { backend: actualBackend.toUpperCase() });
  }
  if (!rawMessage) return t("progress.startingJob");
  return rawMessage || t("progress.local");
}
