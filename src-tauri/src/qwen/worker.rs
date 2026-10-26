use libloading::Library;
use std::{
    ffi::{c_char, c_void, CStr, CString},
    fs,
    mem::{align_of, size_of},
    path::Path,
    ptr,
};

use super::WorkerOutput;
use crate::types::Segment;

// Enum values and field order are from the pinned public header. Before any
// initializer writes into these structs, compare both sizeof and alignof with
// the library. All pointer fields are borrowed only during synchronous calls.
#[repr(C)]
#[derive(Default)]
struct LoadParams {
    size: u64,
    backend: i32,
    device: *mut c_void,
    no_mmap: bool,
}
#[repr(C)]
#[derive(Default)]
struct SessionParams {
    size: u64,
    threads: i32,
    kv_type: i32,
    n_ctx: i32,
}
#[repr(C)]
#[derive(Default)]
struct RunParams {
    size: u64,
    task: i32,
    timestamps: i32,
    pnc: i32,
    itn: i32,
    diarize: i32,
    language: *const c_char,
    target_language: *const c_char,
    keep_tags: bool,
    family: *const c_void,
    drafts: i32,
    vocabulary: *const *const c_char,
    vocabulary_count: i32,
    prompt: *const c_char,
    prefix: *const c_char,
}
#[repr(C)]
#[derive(Default)]
struct BackendParams {
    size: u64,
    directory: *const c_char,
    mask: u32,
}

macro_rules! symbol {
    ($lib:expr, $name:literal, $ty:ty) => {
        *$lib
            .get::<$ty>(concat!($name, "\0").as_bytes())
            .map_err(|e| format!("Qwen ABI {}: {e}", $name))?
    };
}

fn cstring(value: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| "Argumen Qwen mengandung NUL.".into())
}

// SAFETY: only call with a pointer returned by this pinned library, while its
// owning model/session is alive. Never expose borrowed pointers to the caller.
unsafe fn owned_string(value: *const c_char) -> Result<String, String> {
    if value.is_null() {
        return Ok(String::new());
    }
    CStr::from_ptr(value)
        .to_str()
        .map(str::to_owned)
        .map_err(|e| format!("Qwen UTF-8 invalid: {e}"))
}

fn timestamp(samples: usize) -> String {
    let ms = samples as u64 * 1000 / 16000;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

fn chunk_spans(pcm: &[f32]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0;
    while start < pcm.len() {
        // Keep a subsecond tail with the preceding chunk. Prefer a quiet
        // boundary near 20 seconds without dropping or duplicating samples.
        let mut end = (start + 20 * 16000).min(pcm.len());
        if pcm.len() - start <= 21 * 16000 {
            end = pcm.len();
        } else {
            let scan = end - 3 * 16000;
            let window = 3200;
            let mut boundary = end;
            let mut energy: f64 = pcm[scan..scan + window]
                .iter()
                .map(|v| (*v as f64).powi(2))
                .sum();
            for index in scan..=end - window {
                if energy <= (200.0f64 / 32768.0).powi(2) * window as f64 {
                    boundary = index + window / 2;
                }
                if index + window < end {
                    energy += (pcm[index + window] as f64).powi(2) - (pcm[index] as f64).powi(2);
                }
            }
            end = boundary;
        }
        spans.push((start, end));
        start = end;
    }
    spans
}

fn read_pcm(path: &Path) -> Result<Vec<f32>, String> {
    let size = fs::metadata(path)
        .map_err(|e| format!("Qwen WAV: {e}"))?
        .len();
    if !(46..=301 * 16000 * 2 + 44).contains(&size) {
        return Err("Ukuran potongan WAV Qwen tidak valid.".into());
    }
    let bytes = fs::read(path).map_err(|e| format!("Qwen WAV: {e}"))?;
    // The parent writes this canonical header, never arbitrary user media.
    if bytes.len() < 46
        || bytes.len() > 301 * 16000 * 2 + 44
        || &bytes[..4] != b"RIFF"
        || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize + 8 != bytes.len()
        || bytes[16..20] != 16u32.to_le_bytes()
        || &bytes[8..16] != b"WAVEfmt "
        || &bytes[36..40] != b"data"
        || bytes[20..36] != [1, 0, 1, 0, 128, 62, 0, 0, 0, 125, 0, 0, 2, 0, 16, 0]
        || u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize != bytes.len() - 44
        || !(bytes.len() - 44).is_multiple_of(2)
    {
        return Err("Qwen membutuhkan potongan WAV PCM 16 kHz mono 16-bit yang valid.".into());
    }
    Ok(bytes[44..]
        .chunks_exact(2)
        .map(|p| i16::from_le_bytes([p[0], p[1]]) as f32 / 32768.0)
        .collect())
}

fn run(args: &[std::ffi::OsString]) -> Result<(), String> {
    if args.len() != 6 {
        return Err("Argumen worker Qwen tidak lengkap.".into());
    }
    let library = Path::new(&args[0]);
    let model_path = cstring(
        Path::new(&args[1])
            .to_str()
            .ok_or("Path model bukan UTF-8.")?,
    )?;
    let pcm = read_pcm(Path::new(&args[2]))?;
    let backend = args[4].to_str().ok_or("Backend Qwen invalid.")?;
    let (backend_id, mask) = match backend {
        "cpu" => (1, 1),
        "cuda" => (5, 9),
        "metal" => (2, 3),
        _ => return Err("Backend Qwen belum didukung.".into()),
    };
    let language = args[5].to_str().ok_or("Bahasa Qwen invalid.")?;
    let language_hint = cstring(language)?;
    // SAFETY: parent validates the pinned runtime inventory and holds a
    // RuntimeSnapshot for this child's lifetime. No library search by name.
    unsafe {
        let lib = Library::new(library).map_err(|e| format!("Gagal memuat runtime Qwen: {e}"))?;
        let version = symbol!(
            lib,
            "transcribe_version",
            unsafe extern "C" fn() -> *const c_char
        );
        if owned_string(version())? != "0.4.1" {
            return Err("Versi ABI Qwen berbeda dari 0.4.1.".into());
        }
        let abi_size = symbol!(
            lib,
            "transcribe_abi_struct_size",
            unsafe extern "C" fn(i32) -> usize
        );
        let abi_align = symbol!(
            lib,
            "transcribe_abi_struct_align",
            unsafe extern "C" fn(i32) -> usize
        );
        for (id, size, align) in [
            (0, size_of::<LoadParams>(), align_of::<LoadParams>()),
            (1, size_of::<SessionParams>(), align_of::<SessionParams>()),
            (2, size_of::<RunParams>(), align_of::<RunParams>()),
            (15, size_of::<BackendParams>(), align_of::<BackendParams>()),
        ] {
            if abi_size(id) != size || abi_align(id) != align {
                return Err(format!("Layout ABI Qwen tidak cocok: {id}."));
            }
        }
        let init_backend = symbol!(
            lib,
            "transcribe_backend_init_params_init",
            unsafe extern "C" fn(*mut BackendParams)
        );
        let init = symbol!(
            lib,
            "transcribe_init_backends_ex",
            unsafe extern "C" fn(*const BackendParams) -> i32
        );
        let load_init = symbol!(
            lib,
            "transcribe_model_load_params_init",
            unsafe extern "C" fn(*mut LoadParams)
        );
        let session_init = symbol!(
            lib,
            "transcribe_session_params_init",
            unsafe extern "C" fn(*mut SessionParams)
        );
        let run_init = symbol!(
            lib,
            "transcribe_run_params_init",
            unsafe extern "C" fn(*mut RunParams)
        );
        let load = symbol!(
            lib,
            "transcribe_model_load_file",
            unsafe extern "C" fn(*const c_char, *const LoadParams, *mut *mut c_void) -> i32
        );
        let free_model = symbol!(
            lib,
            "transcribe_model_free",
            unsafe extern "C" fn(*mut c_void)
        );
        let create = symbol!(
            lib,
            "transcribe_session_init",
            unsafe extern "C" fn(*mut c_void, *const SessionParams, *mut *mut c_void) -> i32
        );
        let free_session = symbol!(
            lib,
            "transcribe_session_free",
            unsafe extern "C" fn(*mut c_void)
        );
        let execute = symbol!(
            lib,
            "transcribe_run",
            unsafe extern "C" fn(*mut c_void, *const f32, i32, *const RunParams) -> i32
        );
        let text = symbol!(
            lib,
            "transcribe_full_text",
            unsafe extern "C" fn(*const c_void) -> *const c_char
        );
        let detected = symbol!(
            lib,
            "transcribe_detected_language",
            unsafe extern "C" fn(*const c_void) -> *const c_char
        );
        let resolved = symbol!(
            lib,
            "transcribe_model_backend",
            unsafe extern "C" fn(*const c_void) -> *const c_char
        );
        let status_text = symbol!(
            lib,
            "transcribe_status_string",
            unsafe extern "C" fn(i32) -> *const c_char
        );
        let check = |status| -> Result<(), String> {
            if status == 0 {
                Ok(())
            } else {
                Err(format!(
                    "Qwen: {} ({status})",
                    owned_string(status_text(status))?
                ))
            }
        };
        let dir = cstring(
            library
                .parent()
                .ok_or("Folder runtime invalid.")?
                .to_str()
                .ok_or("Path runtime bukan UTF-8.")?,
        )?;
        let mut bp = BackendParams::default();
        init_backend(&mut bp);
        bp.directory = dir.as_ptr();
        bp.mask = mask;
        check(init(&bp))?;
        let mut lp = LoadParams::default();
        load_init(&mut lp);
        lp.backend = backend_id;
        let mut model = ptr::null_mut();
        check(load(model_path.as_ptr(), &lp, &mut model))?;
        if model.is_null() {
            return Err("Qwen mengembalikan model kosong.".into());
        }
        // Always release session before model, including every error path.
        let result = (|| -> Result<WorkerOutput, String> {
            attest_backend(backend, &owned_string(resolved(model))?)?;
            let mut sp = SessionParams::default();
            session_init(&mut sp);
            sp.n_ctx = 2048;
            sp.threads = std::thread::available_parallelism()
                .map(|n| n.get().min(12) as i32)
                .unwrap_or(4);
            let mut session = ptr::null_mut();
            check(create(model, &sp, &mut session))?;
            if session.is_null() {
                return Err("Session Qwen kosong.".into());
            }
            let output = (|| -> Result<WorkerOutput, String> {
                let mut rp = RunParams::default();
                run_init(&mut rp);
                if language != "auto" {
                    rp.language = language_hint.as_ptr();
                }
                let mut output = WorkerOutput {
                    language: if language == "auto" {
                        "unknown".into()
                    } else {
                        language.into()
                    },
                    backend: backend.into(),
                    segments: Vec::new(),
                };
                // Bound the decoder cache and GPU working set. These are chunk
                // boundaries, explicitly disclosed as approximate in the UI.
                for (start, end) in chunk_spans(&pcm) {
                    let samples = &pcm[start..end];
                    check(execute(
                        session,
                        samples.as_ptr(),
                        samples.len() as i32,
                        &rp,
                    ))?;
                    attest_backend(backend, &owned_string(resolved(model))?)?;
                    let transcript = owned_string(text(session))?;
                    if transcript.len() > crate::types::MAX_TRANSCRIPT_TEXT_BYTES {
                        return Err("Teks Qwen terlalu besar.".into());
                    }
                    if output.language == "unknown" {
                        let lang = owned_string(detected(session))?;
                        if !lang.is_empty() {
                            output.language = lang;
                        }
                    }
                    if !transcript.trim().is_empty() {
                        output.segments.push(Segment {
                            from: timestamp(start),
                            to: timestamp(end),
                            text: transcript.trim().to_owned(),
                        });
                    }
                    eprintln!("QWEN_PROGRESS {}", end * 100 / pcm.len());
                }
                Ok(output)
            })();
            free_session(session);
            output
        })();
        free_model(model);
        let bytes = serde_json::to_vec(&result?).map_err(|e| format!("Qwen JSON: {e}"))?;
        if bytes.len() as u64 > crate::checkpoint::MAX_CHECKPOINT_BYTES {
            return Err("Hasil Qwen terlalu besar.".into());
        }
        fs::write(Path::new(&args[3]), bytes)
            .map_err(|e| format!("Gagal menyimpan hasil Qwen: {e}"))?;
    }
    Ok(())
}

fn attest_backend(requested: &str, actual: &str) -> Result<(), String> {
    let actual = actual.to_ascii_lowercase();
    // CUDA_VISIBLE_DEVICES is restricted by the parent to the selected UUID;
    // CUDA0 is its process-local ordinal, never the physical NVIDIA index.
    // transcribe.cpp v0.4.1 reports Metal as MTL<ordinal>. Unlike CUDA, the
    // Metal request selects a backend without specifying a device ordinal.
    if matches!(
        (requested, actual.as_str()),
        ("cpu", "cpu") | ("cuda", "cuda0")
    ) || (requested == "metal"
        && actual.strip_prefix("mtl").is_some_and(|ordinal| {
            !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
        }))
    {
        Ok(())
    } else {
        Err(format!(
            "Qwen meminta {requested}, tetapi runtime memakai {actual}."
        ))
    }
}

/// Returns false for normal application startup. This entry must precede all
/// Tauri initialization (especially the single-instance plugin).
pub fn worker_entry() -> bool {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--qwen-worker")) {
        return false;
    }
    if let Err(error) = run(&args.collect::<Vec<_>>()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn chunks_keep_all_audio_and_prefer_quiet_boundaries() {
        let mut pcm = vec![0.5; 44 * 16000];
        pcm[19 * 16000..20 * 16000].fill(0.0);
        let spans = super::chunk_spans(&pcm);
        assert_eq!(spans.first().unwrap().0, 0);
        assert_eq!(spans.last().unwrap().1, pcm.len());
        assert!(spans[0].1 >= 19 * 16000 && spans[0].1 < 20 * 16000);
        assert!(spans.windows(2).all(|pair| pair[0].1 == pair[1].0));
        assert!(spans
            .iter()
            .all(|(start, end)| end > start && end - start <= 21 * 16000));
        assert_eq!(
            super::chunk_spans(&vec![0.5; 20 * 16000 + 100]),
            vec![(0, 20 * 16000 + 100)]
        );
    }
    #[test]
    fn backend_attestation_rejects_fallback_and_wrong_device() {
        for (request, actual) in [
            ("cpu", "CPU"),
            ("cuda", "CUDA0"),
            ("metal", "MTL0"),
            ("metal", "mtl0"),
            ("metal", "MTL1"),
        ] {
            assert!(
                super::attest_backend(request, actual).is_ok(),
                "requested={request}, actual={actual}"
            );
        }
        for (request, actual) in [
            ("cuda", "CPU"),
            ("cuda", "CUDA1"),
            ("cuda", ""),
            ("metal", "CPU"),
            ("metal", "CUDA0"),
            ("metal", ""),
            ("metal", "Metal"),
            ("metal", "MTL"),
            ("metal", "MTL-1"),
            ("metal", "MTL0invalid"),
            ("cpu", "CUDA0"),
            ("cpu", "MTL0"),
            ("cuda", "MTL0"),
        ] {
            assert!(
                super::attest_backend(request, actual).is_err(),
                "requested={request}, actual={actual}"
            );
        }
    }
    #[test]
    fn approximate_timestamps_preserve_chunk_offset() {
        assert_eq!(super::timestamp(20 * 16000), "00:00:20,000");
        assert_eq!(super::timestamp(16001), "00:00:01,000");
    }
    #[test]
    fn rejects_noncanonical_or_oversized_audio() {
        let path = std::env::temp_dir().join(format!("qwen-wav-{}", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"not a wav").unwrap();
        assert!(super::read_pcm(&path).is_err());
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(302 * 16000 * 2 + 44).unwrap();
        drop(file);
        assert!(super::read_pcm(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn decodes_the_checkpoint_wav_contract() {
        let path = std::env::temp_dir().join(format!("qwen-pcm-{}", uuid::Uuid::new_v4()));
        let mut bytes = Vec::new();
        crate::checkpoint::write_wav_header(&mut bytes, 6).unwrap();
        for sample in [i16::MIN, 0, i16::MAX] {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(&path, &bytes).unwrap();
        let pcm = super::read_pcm(&path).unwrap();
        assert_eq!(pcm, vec![-1.0, 0.0, 32767.0 / 32768.0]);
        bytes[4] = 0;
        std::fs::write(&path, bytes).unwrap();
        assert!(super::read_pcm(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
