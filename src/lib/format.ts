export const stageLabels = {
  idle: "Siap",
  downloading: "Mengunduh audio",
  converting: "Menyiapkan audio",
  transcribing: "Mentranskripsi",
  finalizing: "Menyimpan hasil",
  done: "Selesai",
  error: "Gagal",
} as const;

export function formatDuration(totalSeconds: number) {
  if (!Number.isFinite(totalSeconds) || totalSeconds <= 0) return "—";
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = Math.floor(totalSeconds % 60);
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`
    : `${minutes}:${String(seconds).padStart(2, "0")}`;
}

export function friendlyError(error: unknown) {
  const raw = String(error ?? "Terjadi kesalahan yang tidak diketahui");
  return raw.replace(/^Error:\s*/i, "");
}

export function formatMemory(megabytes: number | null | undefined, notDetected = "Not detected") {
  if (megabytes === null || megabytes === undefined || megabytes <= 0) return notDetected;
  return `${(megabytes / 1024).toFixed(1)} GB`;
}

export function formatBytes(bytes: number | null | undefined, unknown = "—") {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes) || bytes < 0) {
    return unknown;
  }
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  const precision = unitIndex === 0 ? 0 : value >= 10 ? 0 : 1;
  return `${value.toFixed(precision)} ${units[unitIndex]}`;
}
