//! Durable transcription checkpoints. A CLI process cannot resume its decoder
//! state, so only completed, validated chunks are committed. A retry redoes at
//! most the interrupted chunk, with bounded audio memory.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use crate::types::{
    Segment, TranscriptRequest, MAX_TRANSCRIPT_RESULT_BYTES, MAX_TRANSCRIPT_SEGMENTS,
    MAX_TRANSCRIPT_TEXT_BYTES,
};

pub const SAMPLE_RATE: u64 = 16_000;
pub const CHUNK_SECONDS: u64 = 300;
pub const MAX_CHECKPOINT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_CHUNK_SAMPLES: u64 = SAMPLE_RATE * CHUNK_SECONDS;
const CHECKPOINT_VERSION: u8 = 1;
// whisper.cpp decodes 30-second windows (WHISPER_CHUNK_SIZE in whisper.h).
// Its final timestamp can include the unused tail of that decoder window.
const WHISPER_DECODE_WINDOW_MS: u64 = 30_000;

pub struct PcmAudio {
    path: PathBuf,
    data_offset: u64,
    pub samples: u64,
    digest: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AudioChunk {
    pub index: usize,
    pub start: u64,
    pub end: u64,
}

#[derive(Deserialize, Serialize)]
pub struct ChunkOutput {
    pub language: String,
    pub backend: String,
    pub segments: Vec<Segment>,
}

#[derive(Deserialize, Serialize)]
struct SavedChunk {
    version: u8,
    key: String,
    chunk: AudioChunk,
    inference_language: String,
    output: ChunkOutput,
}

pub struct CompletedTranscript {
    pub language: String,
    pub backend: String,
    pub segments: Vec<Segment>,
    pub text: String,
}

fn ordinary_file(path: &Path) -> Result<File, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|e| format!("Gagal membaca file resume: {e}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("File resume bukan file biasa.".into());
    }
    File::open(path).map_err(|e| format!("Gagal membuka file resume: {e}"))
}

impl PcmAudio {
    pub fn inspect(
        path: &Path,
        duration: f64,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self, String> {
        let mut file = ordinary_file(path)?;
        let size = file
            .metadata()
            .map_err(|e| format!("Gagal membaca ukuran WAV: {e}"))?
            .len();
        let mut header = [0; 12];
        file.read_exact(&mut header)
            .map_err(|e| format!("Header WAV tidak lengkap: {e}"))?;
        if &header[..4] != b"RIFF"
            || &header[8..] != b"WAVE"
            || u32::from_le_bytes(header[4..8].try_into().unwrap()) as u64 + 8 != size
        {
            return Err("WAV tidak lengkap atau bukan RIFF/WAVE PCM yang didukung.".into());
        }
        let mut valid_format = false;
        let mut data = None;
        let mut offset = 12u64;
        while offset + 8 <= size {
            if cancelled() {
                return Err("Job dibatalkan.".into());
            }
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| format!("Gagal membaca WAV: {e}"))?;
            let mut chunk_header = [0; 8];
            file.read_exact(&mut chunk_header)
                .map_err(|e| format!("Chunk WAV tidak lengkap: {e}"))?;
            let length = u32::from_le_bytes(chunk_header[4..].try_into().unwrap()) as u64;
            let end = offset + 8 + length;
            if end > size {
                return Err("Data WAV terpotong.".into());
            }
            match &chunk_header[..4] {
                b"fmt " if length >= 16 => {
                    let mut format = [0; 16];
                    file.read_exact(&mut format)
                        .map_err(|e| format!("Format WAV tidak lengkap: {e}"))?;
                    valid_format = u16::from_le_bytes(format[0..2].try_into().unwrap()) == 1
                        && u16::from_le_bytes(format[2..4].try_into().unwrap()) == 1
                        && u32::from_le_bytes(format[4..8].try_into().unwrap())
                            == SAMPLE_RATE as u32
                        && u32::from_le_bytes(format[8..12].try_into().unwrap())
                            == SAMPLE_RATE as u32 * 2
                        && u16::from_le_bytes(format[12..14].try_into().unwrap()) == 2
                        && u16::from_le_bytes(format[14..16].try_into().unwrap()) == 16;
                }
                b"data" => {
                    if !valid_format || data.is_some() || length < 2 || !length.is_multiple_of(2) {
                        return Err("Format atau data PCM WAV tidak valid.".into());
                    }
                    data = Some((offset + 8, length / 2));
                }
                _ => {}
            }
            offset = end + length % 2;
        }
        let (data_offset, samples) = data.ok_or("WAV tidak memiliki data PCM.")?;
        if !duration.is_finite()
            || duration <= 0.0
            || duration > crate::transcription::MAX_MEDIA_DURATION_SECONDS
            || samples > ((duration.max(1.0) + 1.0) * SAMPLE_RATE as f64) as u64
        {
            return Err("Durasi WAV berada di luar batas media.".into());
        }
        Ok(Self {
            path: path.into(),
            data_offset,
            samples,
            digest: String::new(),
        })
    }

    pub fn open(path: &Path, duration: f64, cancelled: impl Fn() -> bool) -> Result<Self, String> {
        let mut audio = Self::inspect(path, duration, &cancelled)?;
        let mut file = ordinary_file(path)?;
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            if cancelled() {
                return Err("Job dibatalkan.".into());
            }
            let read = file
                .read(&mut buffer)
                .map_err(|e| format!("Gagal membaca fingerprint audio: {e}"))?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
        }
        audio.digest = format!("{:x}", hash.finalize());
        Ok(audio)
    }

    pub fn plan(&self, cancelled: impl Fn() -> bool) -> Result<Vec<AudioChunk>, String> {
        let mut file = ordinary_file(&self.path)?;
        let mut chunks = Vec::new();
        let mut start = 0;
        while start < self.samples {
            if cancelled() {
                return Err("Job dibatalkan.".into());
            }
            // Absorb a tiny tail; no chunk exceeds 301 seconds.
            let target = if self.samples - start <= MAX_CHUNK_SAMPLES + SAMPLE_RATE {
                self.samples
            } else {
                start + MAX_CHUNK_SAMPLES
            };
            let mut end = target;
            if target < self.samples {
                // Prefer a quiet 200 ms window in the last 10 seconds. With no
                // silence, use the hard boundary; never invent missing audio.
                let scan_start = target - 10 * SAMPLE_RATE;
                file.seek(SeekFrom::Start(self.data_offset + scan_start * 2))
                    .map_err(|e| format!("Gagal mencari jeda audio: {e}"))?;
                let mut bytes = vec![0; (10 * SAMPLE_RATE * 2) as usize];
                file.read_exact(&mut bytes)
                    .map_err(|e| format!("Gagal membaca jeda audio: {e}"))?;
                let values = bytes
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]) as i64)
                    .collect::<Vec<_>>();
                let window = (SAMPLE_RATE / 5) as usize;
                let mut energy = values[..window].iter().map(|v| v * v).sum::<i64>();
                for index in 0..=values.len() - window {
                    if energy <= 200 * 200 * window as i64 {
                        end = scan_start + index as u64 + window as u64 / 2;
                    }
                    if index + window < values.len() {
                        energy += values[index + window].pow(2) - values[index].pow(2);
                    }
                }
            }
            chunks.push(AudioChunk {
                index: chunks.len(),
                start,
                end,
            });
            start = end;
        }
        Ok(chunks)
    }

    fn write_chunk(&self, chunk: &AudioChunk, path: &Path) -> Result<(), String> {
        let mut input = ordinary_file(&self.path)?;
        input
            .seek(SeekFrom::Start(self.data_offset + chunk.start * 2))
            .map_err(|e| format!("Gagal membaca potongan audio: {e}"))?;
        let samples = chunk.end - chunk.start;
        let padded_samples = samples.max(SAMPLE_RATE);
        let mut bytes = Vec::with_capacity((padded_samples * 2 + 44) as usize);
        write_wav_header(&mut bytes, padded_samples * 2)?;
        let read = input
            .take(samples * 2)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Gagal membaca potongan audio: {e}"))?;
        if read as u64 != samples * 2 {
            return Err("Audio berubah atau terpotong selama transkripsi.".into());
        }
        bytes.resize((padded_samples * 2 + 44) as usize, 0);
        atomic_write(path, &bytes)
    }
}

pub(crate) fn write_wav_header(writer: &mut impl Write, bytes: u64) -> Result<(), String> {
    let write = || -> std::io::Result<()> {
        writer.write_all(b"RIFF")?;
        writer.write_all(&((bytes + 36) as u32).to_le_bytes())?;
        writer.write_all(b"WAVEfmt ")?;
        writer.write_all(&16u32.to_le_bytes())?;
        writer.write_all(&1u16.to_le_bytes())?;
        writer.write_all(&1u16.to_le_bytes())?;
        writer.write_all(&(SAMPLE_RATE as u32).to_le_bytes())?;
        writer.write_all(&(SAMPLE_RATE as u32 * 2).to_le_bytes())?;
        writer.write_all(&2u16.to_le_bytes())?;
        writer.write_all(&16u16.to_le_bytes())?;
        writer.write_all(b"data")?;
        writer.write_all(&(bytes as u32).to_le_bytes())
    };
    let mut write = write;
    write().map_err(|e| format!("Gagal membuat header potongan WAV: {e}"))
}

fn owned_directory(path: &Path) -> Result<(), String> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let meta = fs::symlink_metadata(path)
                .map_err(|e| format!("Gagal memvalidasi folder resume: {e}"))?;
            if meta.is_dir() && !meta.file_type().is_symlink() {
                Ok(())
            } else {
                Err("Folder resume bukan direktori biasa.".into())
            }
        }
        Err(e) => Err(format!("Gagal membuat folder resume: {e}")),
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| format!("Gagal membuat checkpoint sementara: {e}"))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("Gagal menyimpan checkpoint: {e}"))?;
        drop(file);
        fs::rename(&temporary, path).map_err(|e| format!("Gagal mengaktifkan checkpoint: {e}"))
    })();
    if result.is_err() {
        if let Err(error) = fs::remove_file(&temporary) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("WhisperTube checkpoint cleanup warning: {error}");
            }
        }
    }
    result
}

fn timestamp_ms(value: &str) -> Result<u64, String> {
    let parts = value.split([':', ',', '.']).collect::<Vec<_>>();
    if parts.len() != 4
        || parts
            .iter()
            .any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("Timestamp checkpoint tidak valid.".into());
    }
    let numbers = parts
        .iter()
        .map(|s| s.parse::<u64>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Timestamp checkpoint di luar batas.".to_string())?;
    if numbers[0] > 24
        || numbers[1] >= 60
        || numbers[2] >= 60
        || numbers[3] >= 1000
        || parts[3].len() != 3
    {
        return Err("Timestamp checkpoint di luar batas.".into());
    }
    Ok(((numbers[0] * 60 + numbers[1]) * 60 + numbers[2]) * 1000 + numbers[3])
}

fn timestamp(value: u64, separator: char) -> String {
    format!(
        "{:02}:{:02}:{:02}{separator}{:03}",
        value / 3_600_000,
        value / 60_000 % 60,
        value / 1000 % 60,
        value % 1000
    )
}

fn validate_output(output: &ChunkOutput, chunk: &AudioChunk) -> Result<(), String> {
    if output.language.is_empty()
        || output.language.len() > 16
        || !output
            .language
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || b == b'-')
        || !matches!(output.backend.as_str(), "cpu" | "cuda" | "metal" | "vulkan")
        || output.segments.len() > MAX_TRANSCRIPT_SEGMENTS.min(10_000)
    {
        return Err("Metadata hasil checkpoint tidak valid.".into());
    }
    let duration_ms = (chunk.end - chunk.start).max(SAMPLE_RATE) * 1000 / SAMPLE_RATE;
    let mut previous_start = 0;
    let mut text_bytes = 0usize;
    for segment in &output.segments {
        let start = timestamp_ms(&segment.from)?;
        let end = timestamp_ms(&segment.to)?;
        if start < previous_start || end < start || end > duration_ms + 1000 {
            return Err("Rentang timestamp checkpoint tidak valid.".into());
        }
        previous_start = start;
        text_bytes = text_bytes.saturating_add(segment.text.len() + 1);
        if text_bytes > MAX_TRANSCRIPT_TEXT_BYTES {
            return Err("Teks checkpoint terlalu besar.".into());
        }
    }
    Ok(())
}

fn normalize_engine_output(output: &mut ChunkOutput, chunk: &AudioChunk) -> Result<(), String> {
    let duration_ms = (chunk.end - chunk.start) * 1000 / SAMPLE_RATE;
    if let Some(last) = output.segments.last_mut() {
        let start = timestamp_ms(&last.from)?;
        let end = timestamp_ms(&last.to)?;
        // Only repair the final decoder endpoint when its segment starts in
        // real audio and fits one native decoding window. Do not normalize
        // persisted checkpoints, earlier outliers, reversed or unordered ranges.
        // Clipping before saving preserves the text and makes resume validate
        // the same bounded timestamps that will appear in the merged exports.
        if start < duration_ms && end > duration_ms && end - start <= WHISPER_DECODE_WINDOW_MS {
            eprintln!(
                "WhisperTube: timestamp akhir bagian {} dibatasi dari {end} ms ke {duration_ms} ms sesuai durasi audio.",
                chunk.index + 1
            );
            last.to = timestamp(duration_ms, ',');
        }
    }
    validate_output(output, chunk)
}

/// The callback runs only missing/invalid chunks. It must return only after the
/// native child exits successfully and its complete JSON has been decoded.
pub fn transcribe<F, C, P>(
    audio: &PcmAudio,
    job_dir: &Path,
    request: &TranscriptRequest,
    mut run: F,
    cancelled: C,
    mut progress: P,
) -> Result<CompletedTranscript, String>
where
    F: FnMut(&Path, &Path, &str, (f64, f64)) -> Result<ChunkOutput, String>,
    C: Fn() -> bool,
    P: FnMut(f64, bool),
{
    let chunks = audio.plan(&cancelled)?;
    let identity = serde_json::to_vec(&(
        CHECKPOINT_VERSION,
        env!("CARGO_PKG_VERSION"),
        &audio.digest,
        audio.samples,
        &request.url,
        request.duration.to_bits(),
        crate::models::model_spec(&request.model_id)?.sha256,
        &request.language,
        &request.backend,
        &request.compute_device_id,
        CHUNK_SECONDS,
    ))
    .map_err(|e| format!("Gagal membuat identitas checkpoint: {e}"))?;
    let key = format!("{:x}", Sha256::digest(identity));
    let root = job_dir.join(".checkpoints");
    owned_directory(&root)?;
    let directory = root.join(&key);
    owned_directory(&directory)?;
    let chunk_audio = directory.join("current.wav");
    let prefix = directory.join("current");
    let mut segments = Vec::new();
    let mut text_bytes = 0usize;
    let mut detected_language = None;
    let mut backends = std::collections::BTreeSet::new();
    for chunk in chunks {
        if cancelled() {
            return Err("Job dibatalkan.".into());
        }
        let inference_language = if request.language == "auto" {
            detected_language.as_deref().unwrap_or("auto")
        } else {
            &request.language
        };
        let path = directory.join(format!("{:04}.json", chunk.index));
        let saved = match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("Gagal membaca checkpoint: {e}")),
            Ok(metadata) if metadata.len() > MAX_CHECKPOINT_BYTES => {
                eprintln!("WhisperTube: checkpoint bagian {} terlalu besar; bagian ini akan diproses ulang.", chunk.index + 1);
                None
            }
            Ok(_) => {
                let mut bytes = Vec::new();
                ordinary_file(&path)?
                    .take(MAX_CHECKPOINT_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| format!("Gagal membaca checkpoint: {e}"))?;
                let decoded = serde_json::from_slice::<SavedChunk>(&bytes)
                    .ok()
                    .filter(|saved| {
                        bytes.len() as u64 <= MAX_CHECKPOINT_BYTES
                            && saved.version == CHECKPOINT_VERSION
                            && saved.key == key
                            && saved.chunk == chunk
                            && saved.inference_language == inference_language
                            && validate_output(&saved.output, &chunk).is_ok()
                    });
                if decoded.is_none() {
                    eprintln!("WhisperTube: checkpoint bagian {} tidak valid; bagian ini akan diproses ulang.", chunk.index + 1);
                }
                decoded
            }
        };
        let start_percent = chunk.start as f64 / audio.samples as f64 * 100.0;
        let end_percent = chunk.end as f64 / audio.samples as f64 * 100.0;
        let reused = saved.is_some();
        let output = if let Some(saved) = saved {
            saved.output
        } else {
            progress(start_percent, false);
            audio.write_chunk(&chunk, &chunk_audio)?;
            // Remove previous process outputs before launching: an interrupted
            // CLI must never be mistaken for a fresh successful transcript.
            for extension in ["json", "txt", "srt", "vtt"] {
                let output = prefix.with_extension(extension);
                match fs::remove_file(output) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(format!("Gagal membersihkan output potongan: {e}")),
                }
            }
            let mut output = run(
                &chunk_audio,
                &prefix,
                inference_language,
                (start_percent, end_percent),
            )?;
            normalize_engine_output(&mut output, &chunk)?;
            let saved = SavedChunk {
                version: CHECKPOINT_VERSION,
                key: key.clone(),
                chunk: chunk.clone(),
                inference_language: inference_language.into(),
                output,
            };
            let bytes = serde_json::to_vec(&saved)
                .map_err(|e| format!("Gagal serialize checkpoint: {e}"))?;
            if bytes.len() as u64 > MAX_CHECKPOINT_BYTES {
                return Err("Checkpoint terlalu besar.".into());
            }
            atomic_write(&path, &bytes)?;
            // Rename in the parent job directory renews its retention age.
            // Writes inside the nested checkpoint directory alone do not.
            atomic_write(&job_dir.join(".in-progress"), b"checkpoint")?;
            saved.output
        };
        progress(end_percent, reused);
        if detected_language.is_none() && output.language != "unknown" {
            detected_language = Some(output.language.clone());
        }
        backends.insert(output.backend);
        let offset_ms = chunk.start * 1000 / SAMPLE_RATE;
        let end_ms = chunk.end * 1000 / SAMPLE_RATE;
        for mut segment in output.segments {
            let start = (timestamp_ms(&segment.from)? + offset_ms).min(end_ms);
            let end = (timestamp_ms(&segment.to)? + offset_ms).min(end_ms);
            if start >= end {
                continue;
            }
            segment.from = timestamp(start, ',');
            segment.to = timestamp(end, ',');
            text_bytes = text_bytes.saturating_add(segment.text.len() + 1);
            if text_bytes > MAX_TRANSCRIPT_TEXT_BYTES || segments.len() >= MAX_TRANSCRIPT_SEGMENTS {
                return Err("Hasil gabungan transkripsi melebihi batas ukuran aman.".into());
            }
            segments.push(segment);
        }
    }
    let text = segments
        .iter()
        .map(|s: &Segment| s.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(CompletedTranscript {
        language: detected_language.unwrap_or_else(|| "unknown".into()),
        backend: backends.into_iter().collect::<Vec<_>>().join("+"),
        segments,
        text,
    })
}

pub fn write_exports(prefix: &Path, result: &CompletedTranscript) -> Result<(), String> {
    let mut srt = String::new();
    let mut vtt = String::from("WEBVTT\n\n");
    for (index, segment) in result.segments.iter().enumerate() {
        srt.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            index + 1,
            segment.from,
            segment.to,
            segment.text
        ));
        vtt.push_str(&format!(
            "{} --> {}\n{}\n\n",
            segment.from.replace(',', "."),
            segment.to.replace(',', "."),
            segment.text
        ));
    }
    for (extension, bytes) in [
        ("txt", result.text.as_bytes()),
        ("srt", srt.as_bytes()),
        ("vtt", vtt.as_bytes()),
    ] {
        if bytes.len() as u64 > MAX_TRANSCRIPT_RESULT_BYTES {
            return Err("Export gabungan terlalu besar.".into());
        }
        atomic_write(&prefix.with_extension(extension), bytes)?;
    }
    Ok(())
}

pub fn cleanup(job_dir: &Path) -> Result<(), String> {
    let directory = job_dir.join(".checkpoints");
    match fs::symlink_metadata(&directory) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Gagal membaca checkpoint: {e}")),
        Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {
            fs::remove_dir_all(directory).map_err(|e| format!("Gagal membersihkan checkpoint: {e}"))
        }
        Ok(_) => Err("Folder checkpoint bukan direktori biasa.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Fixture {
        root: PathBuf,
        wav: PathBuf,
        request: TranscriptRequest,
    }
    impl Fixture {
        fn new(seconds: u64) -> Self {
            let root = std::env::temp_dir()
                .join(format!("whispertube-checkpoint-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&root).unwrap();
            fs::write(root.join(".in-progress"), b"").unwrap();
            let wav = root.join("audio.wav");
            let mut file = File::create(&wav).unwrap();
            write_wav_header(&mut file, seconds * SAMPLE_RATE * 2).unwrap();
            file.set_len(seconds * SAMPLE_RATE * 2 + 44).unwrap();
            Self {
                root,
                wav,
                request: TranscriptRequest {
                    url: "https://www.youtube.com/watch?v=resume-test".into(),
                    title: "Fixture".into(),
                    channel: "Tests".into(),
                    duration: seconds as f64,
                    cookies_path: None,
                    backend: "cpu".into(),
                    compute_device_id: None,
                    language: "id".into(),
                    model_id: "base".into(),
                    keep_audio: false,
                },
            }
        }
        fn audio(&self) -> PcmAudio {
            PcmAudio::open(&self.wav, self.request.duration, || false).unwrap()
        }
        fn checkpoint_directory(&self) -> PathBuf {
            fs::read_dir(self.root.join(".checkpoints"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
    fn output(text: &str) -> ChunkOutput {
        ChunkOutput {
            language: "id".into(),
            backend: "cpu".into(),
            segments: vec![Segment {
                from: "00:00:00,000".into(),
                to: "00:00:01,000".into(),
                text: text.into(),
            }],
        }
    }

    #[test]
    fn completed_chunks_survive_failure_and_restart_without_repeating_work() {
        let fixture = Fixture::new(620);
        let audio = fixture.audio();
        let mut calls = 0;
        let failed = transcribe(
            &audio,
            &fixture.root,
            &fixture.request,
            |_, prefix, _, _| {
                calls += 1;
                if calls == 2 {
                    fs::write(prefix.with_extension("json"), b"partial output").unwrap();
                    Err("RAM tidak cukup".into())
                } else {
                    Ok(output("pertama"))
                }
            },
            || false,
            |_, _| {},
        );
        assert!(failed.is_err());
        assert_eq!(calls, 2);
        assert!(fixture.checkpoint_directory().join("0000.json").exists());
        assert!(!fixture.checkpoint_directory().join("0001.json").exists());
        drop(audio); // Retry reconstructs everything from disk, not retained state.
        let audio = fixture.audio();
        let mut resumed_calls = 0;
        let mut reused = 0;
        let resumed = transcribe(
            &audio,
            &fixture.root,
            &fixture.request,
            |chunk, prefix, _, _| {
                assert!(!prefix.with_extension("json").exists());
                assert!(PcmAudio::open(chunk, 301.0, || false).is_ok());
                resumed_calls += 1;
                Ok(output("lanjutan"))
            },
            || false,
            |_, saved| {
                reused += usize::from(saved);
            },
        )
        .unwrap();
        assert_eq!(resumed_calls, 2);
        assert_eq!(reused, 1);
        assert_eq!(resumed.segments.len(), 3);
        assert_eq!(resumed.text, "pertama\nlanjutan\nlanjutan");
        assert_eq!(resumed.segments[1].from, "00:04:59,900");
        assert_eq!(resumed.segments[2].from, "00:09:59,800");
        write_exports(&fixture.root.join("transcript"), &resumed).unwrap();
        let srt = fs::read_to_string(fixture.root.join("transcript.srt")).unwrap();
        let vtt = fs::read_to_string(fixture.root.join("transcript.vtt")).unwrap();
        assert!(srt.contains("2\n00:04:59,900 --> 00:05:00,900"));
        assert!(vtt.starts_with("WEBVTT\n\n"));
        assert!(vtt.contains("00:04:59.900 --> 00:05:00.900"));
        // Finalization retry never invokes the engine again.
        transcribe(
            &audio,
            &fixture.root,
            &fixture.request,
            |_, _, _, _| panic!("committed chunks must be reused"),
            || false,
            |_, _| {},
        )
        .unwrap();
        cleanup(&fixture.root).unwrap();
        assert!(!fixture.root.join(".checkpoints").exists());
    }

    #[test]
    fn cancellation_preserves_completed_chunk_and_retry_continues() {
        let fixture = Fixture::new(620);
        let audio = fixture.audio();
        let cancelled = Cell::new(false);
        let mut calls = 0;
        assert!(transcribe(
            &audio,
            &fixture.root,
            &fixture.request,
            |_, _, _, _| {
                calls += 1;
                Ok(output("saved"))
            },
            || cancelled.get(),
            |percent, _| {
                if percent > 0.0 {
                    cancelled.set(true);
                }
            }
        )
        .is_err());
        assert_eq!(calls, 1);
        let mut calls = 0;
        transcribe(
            &fixture.audio(),
            &fixture.root,
            &fixture.request,
            |_, _, _, _| {
                calls += 1;
                Ok(output("retry"))
            },
            || false,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(calls, 2);
    }

    #[test]
    fn corrupted_checkpoint_recomputes_only_its_chunk() {
        let fixture = Fixture::new(620);
        let audio = fixture.audio();
        transcribe(
            &audio,
            &fixture.root,
            &fixture.request,
            |_, _, _, _| Ok(output("good")),
            || false,
            |_, _| {},
        )
        .unwrap();
        fs::write(
            fixture.checkpoint_directory().join("0001.json"),
            b"{truncated",
        )
        .unwrap();
        let mut calls = 0;
        let completed = transcribe(
            &audio,
            &fixture.root,
            &fixture.request,
            |_, _, _, _| {
                calls += 1;
                Ok(output("repaired"))
            },
            || false,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(completed.text, "good\nrepaired\ngood");
    }

    #[test]
    fn fingerprint_rejects_different_model_language_backend_or_audio() {
        let fixture = Fixture::new(2);
        let audio = fixture.audio();
        transcribe(
            &audio,
            &fixture.root,
            &fixture.request,
            |_, _, _, _| Ok(output("old")),
            || false,
            |_, _| {},
        )
        .unwrap();
        for (model, language, backend) in [
            ("large-v3-turbo-q5_0", "id", "cpu"),
            ("base", "en", "cpu"),
            ("base", "id", "auto"),
        ] {
            let request = TranscriptRequest {
                model_id: model.into(),
                language: language.into(),
                backend: backend.into(),
                ..fixture.request.clone()
            };
            let mut calls = 0;
            transcribe(
                &audio,
                &fixture.root,
                &request,
                |_, _, _, _| {
                    calls += 1;
                    Ok(output("new"))
                },
                || false,
                |_, _| {},
            )
            .unwrap();
            assert_eq!(calls, 1);
        }
        let mut file = OpenOptions::new().write(true).open(&fixture.wav).unwrap();
        file.seek(SeekFrom::Start(44)).unwrap();
        file.write_all(&[1, 0]).unwrap();
        drop(file);
        let mut calls = 0;
        transcribe(
            &fixture.audio(),
            &fixture.root,
            &fixture.request,
            |_, _, _, _| {
                calls += 1;
                Ok(output("changed audio"))
            },
            || false,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(calls, 1);
    }

    #[test]
    fn auto_language_is_carried_across_chunks_and_resume() {
        let mut fixture = Fixture::new(620);
        fixture.request.language = "auto".into();
        let mut languages = Vec::new();
        transcribe(
            &fixture.audio(),
            &fixture.root,
            &fixture.request,
            |_, _, language, _| {
                languages.push(language.to_string());
                if languages.len() == 2 {
                    Err("interrupted".into())
                } else {
                    Ok(output("saved"))
                }
            },
            || false,
            |_, _| {},
        )
        .err()
        .unwrap();
        assert_eq!(languages, ["auto", "id"]);
        let mut languages = Vec::new();
        transcribe(
            &fixture.audio(),
            &fixture.root,
            &fixture.request,
            |_, _, language, _| {
                languages.push(language.to_string());
                Ok(output("resumed"))
            },
            || false,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(languages, ["id", "id"]);
    }

    #[test]
    fn plan_covers_audio_once_and_prefers_a_silent_boundary() {
        let fixture = Fixture::new(620);
        let mut file = OpenOptions::new().write(true).open(&fixture.wav).unwrap();
        file.seek(SeekFrom::Start(44 + 290 * SAMPLE_RATE * 2))
            .unwrap();
        let noise = [0xd0, 0x07].repeat((10 * SAMPLE_RATE) as usize); // constant +2000
        file.write_all(&noise).unwrap();
        file.seek(SeekFrom::Start(44 + 295 * SAMPLE_RATE * 2))
            .unwrap();
        file.write_all(&vec![0; (SAMPLE_RATE * 2 / 2) as usize])
            .unwrap(); // 0.5s quiet
        drop(file);
        let audio = fixture.audio();
        let chunks = audio.plan(|| false).unwrap();
        assert_eq!(chunks[0].start, 0);
        assert_eq!(chunks.last().unwrap().end, audio.samples);
        assert!(chunks[0].end > 295 * SAMPLE_RATE && chunks[0].end < 296 * SAMPLE_RATE);
        for chunk in &chunks {
            assert!(chunk.end > chunk.start && chunk.end - chunk.start <= 301 * SAMPLE_RATE);
        }
        for pair in chunks.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
    }

    #[test]
    fn decoder_tail_overrun_is_clipped_before_checkpoint_and_reused_on_retry() {
        let fixture = Fixture::new(299);
        // Reproduce the observed engine output on a 298.4453125-second chunk.
        let samples = 4_775_125;
        let mut wav = OpenOptions::new().write(true).open(&fixture.wav).unwrap();
        write_wav_header(&mut wav, samples * 2).unwrap();
        wav.set_len(samples * 2 + 44).unwrap();
        drop(wav);
        let audio = fixture.audio();
        let completed = transcribe(
            &audio,
            &fixture.root,
            &fixture.request,
            |_, _, _, _| {
                Ok(ChunkOutput {
                    language: "en".into(),
                    backend: "cpu".into(),
                    segments: vec![
                        Segment {
                            from: "00:04:55,780".into(),
                            to: "00:04:58,040".into(),
                            text: "before tail".into(),
                        },
                        Segment {
                            from: "00:04:58,040".into(),
                            to: "00:05:28,020".into(),
                            text: "tail".into(),
                        },
                    ],
                })
            },
            || false,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(completed.segments.len(), 2);
        assert_eq!(completed.segments[0].to, "00:04:58,040");
        assert_eq!(completed.segments[1].from, "00:04:58,040");
        assert_eq!(completed.segments[1].to, "00:04:58,445");
        assert_eq!(completed.text, "before tail\ntail");
        let saved: SavedChunk = serde_json::from_slice(
            &fs::read(fixture.checkpoint_directory().join("0000.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(saved.output.segments[1].to, "00:04:58,445");
        let resumed = transcribe(
            &audio,
            &fixture.root,
            &fixture.request,
            |_, _, _, _| panic!("normalized checkpoint must be reusable"),
            || false,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(resumed.text, completed.text);
        assert_eq!(resumed.segments[1].to, "00:04:58,445");
    }

    #[test]
    fn decoder_tail_normalization_does_not_accept_other_invalid_ranges() {
        let chunk = AudioChunk {
            index: 0,
            start: 0,
            end: 300 * SAMPLE_RATE,
        };
        for (from, to) in [
            ("00:04:59,000", "00:05:29,001"), // More than one decoder window.
            ("00:05:01,010", "00:05:29,000"), // Starts outside actual audio.
            ("00:04:59,000", "00:04:58,000"), // Reversed.
            ("00:04:59,000", "00:05:90,000"), // Malformed.
        ] {
            let mut invalid = output("invalid");
            invalid.segments[0].from = from.into();
            invalid.segments[0].to = to.into();
            assert!(
                normalize_engine_output(&mut invalid, &chunk).is_err(),
                "{from} -> {to}"
            );
        }
        let mut out_of_order = output("first");
        out_of_order.segments[0].from = "00:04:59,500".into();
        out_of_order.segments[0].to = "00:04:59,600".into();
        out_of_order.segments.push(Segment {
            from: "00:04:59,000".into(),
            to: "00:05:29,000".into(),
            text: "backwards".into(),
        });
        assert!(normalize_engine_output(&mut out_of_order, &chunk).is_err());
        let mut earlier_outlier = output("earlier outlier");
        earlier_outlier.segments[0].to = "00:05:29,000".into();
        earlier_outlier.segments.push(Segment {
            from: "00:04:59,000".into(),
            to: "00:05:29,000".into(),
            text: "tail".into(),
        });
        assert!(normalize_engine_output(&mut earlier_outlier, &chunk).is_err());
        let mut raw_tail = output("must not repair saved checkpoints");
        raw_tail.segments[0].from = "00:04:59,000".into();
        raw_tail.segments[0].to = "00:05:29,000".into();
        assert!(validate_output(&raw_tail, &chunk).is_err());
        normalize_engine_output(&mut raw_tail, &chunk).unwrap();
        assert_eq!(raw_tail.segments[0].to, "00:05:00,000");
    }

    #[test]
    fn rejects_truncated_wav_and_invalid_output_timestamps() {
        let fixture = Fixture::new(2);
        let mut invalid = output("bad");
        invalid.segments[0].to = "00:00:90,000".into();
        assert!(transcribe(
            &fixture.audio(),
            &fixture.root,
            &fixture.request,
            |_, _, _, _| Ok(ChunkOutput {
                language: invalid.language.clone(),
                backend: invalid.backend.clone(),
                segments: invalid.segments.clone()
            }),
            || false,
            |_, _| {}
        )
        .is_err());
        assert!(!fixture.checkpoint_directory().join("0000.json").exists());
        File::create(&fixture.wav).unwrap().set_len(50).unwrap();
        assert!(PcmAudio::open(&fixture.wav, 2.0, || false).is_err());
    }

    #[test]
    fn checkpoint_storage_error_is_not_reported_as_success() {
        let fixture = Fixture::new(2);
        fs::write(fixture.root.join(".checkpoints"), b"not a directory").unwrap();
        let mut calls = 0;
        assert!(transcribe(
            &fixture.audio(),
            &fixture.root,
            &fixture.request,
            |_, _, _, _| {
                calls += 1;
                Ok(output("wrong"))
            },
            || false,
            |_, _| {}
        )
        .is_err());
        assert_eq!(calls, 0);
    }

    #[test]
    fn oversized_or_forged_checkpoint_is_never_reused() {
        let fixture = Fixture::new(2);
        let audio = fixture.audio();
        transcribe(
            &audio,
            &fixture.root,
            &fixture.request,
            |_, _, _, _| Ok(output("valid")),
            || false,
            |_, _| {},
        )
        .unwrap();
        let path = fixture.checkpoint_directory().join("0000.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["chunk"]["start"] = serde_json::json!(16000);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let mut calls = 0;
        transcribe(
            &audio,
            &fixture.root,
            &fixture.request,
            |_, _, _, _| {
                calls += 1;
                Ok(output("recomputed"))
            },
            || false,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(calls, 1);
        File::create(&path)
            .unwrap()
            .set_len(MAX_CHECKPOINT_BYTES + 1)
            .unwrap();
        let mut calls = 0;
        transcribe(
            &audio,
            &fixture.root,
            &fixture.request,
            |_, _, _, _| {
                calls += 1;
                Ok(output("recomputed again"))
            },
            || false,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(calls, 1);
    }
}
