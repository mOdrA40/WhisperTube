//! Qwen runs in a child of this executable, before Tauri initializes. A native
//! driver crash cannot take down the UI, and the existing process owner handles
//! cancellation. The ABI below is pinned to transcribe.cpp v0.4.1/include/transcribe.h.
mod runtime;
mod worker;

pub use runtime::{ensure_runtime, installed, library_path, snapshot, supported_backends};
pub use worker::worker_entry;

pub const MODEL_ID: &str = "qwen3-asr-1.7b-q8_0";
pub const MODEL_FILE: &str = "Qwen3-ASR-1.7B-Q8_0.gguf";
pub const MODEL_URL: &str = "https://huggingface.co/handy-computer/Qwen3-ASR-1.7B-gguf/resolve/0e0a596e3959c3f3f2ab7d443a5b6f0f4a4bba6f/Qwen3-ASR-1.7B-Q8_0.gguf";

pub fn is_qwen(id: &str) -> bool {
    id == MODEL_ID
}

pub fn select_cuda_candidate(
    requested: &str,
    candidate: Option<crate::system::GpuInfo>,
    capability: impl FnOnce(&crate::system::GpuInfo) -> Result<(u32, u32), String>,
) -> Result<Option<crate::system::GpuInfo>, String> {
    let Some(gpu) = candidate else {
        return Ok(None);
    };
    let compatibility = capability(&gpu).and_then(|capability| {
        // Pinned v0.4.1 bindings/python-native-cu12/pyproject.toml builds
        // CMAKE_CUDA_ARCHITECTURES=75;80;86;89;90 (including forward-compatible
        // PTX). A compatible Whisper CUDA engine does not imply Qwen support.
        if capability >= (7, 5) {
            Ok(())
        } else {
            Err(format!(
                "{} memiliki compute capability {}.{}, tetapi runtime Qwen 0.4.1 memerlukan minimal 7.5.",
                gpu.name, capability.0, capability.1
            ))
        }
    });
    match compatibility {
        Ok(()) => Ok(Some(gpu)),
        Err(error) if requested == "auto" => {
            eprintln!("WhisperTube: CUDA Qwen dilewati; Auto memakai CPU: {error}");
            Ok(None)
        }
        Err(error) => Err(format!(
            "CUDA Qwen tidak dapat digunakan: {error} Pilih CPU atau Auto."
        )),
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct WorkerOutput {
    pub language: String,
    pub backend: String,
    pub segments: Vec<crate::types::Segment>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu() -> crate::system::GpuInfo {
        crate::system::GpuInfo {
            name: "Selected GPU".into(),
            total_memory_mb: Some(8192),
            free_memory_mb: Some(6144),
            uuid: Some("GPU-aaaaaaaa".into()),
            device_index: Some(3),
        }
    }

    #[test]
    fn cuda_selection_preserves_the_requested_gpu_for_supported_architectures() {
        for requested in ["auto", "cuda"] {
            for capability in [(7, 5), (8, 0), (8, 6), (8, 9), (9, 0), (12, 0)] {
                let selected = select_cuda_candidate(requested, Some(gpu()), |candidate| {
                    assert_eq!(candidate.uuid.as_deref(), Some("GPU-aaaaaaaa"));
                    Ok(capability)
                })
                .unwrap()
                .unwrap();
                assert_eq!(selected.device_index, Some(3));
                assert_eq!(selected.uuid.as_deref(), Some("GPU-aaaaaaaa"));
                let mut command = std::process::Command::new("qwen-worker");
                crate::system::configure_cuda_command(&mut command, &selected).unwrap();
                assert!(command.get_envs().any(|(name, value)| {
                    name == "CUDA_VISIBLE_DEVICES"
                        && value == Some(std::ffi::OsStr::new("GPU-aaaaaaaa"))
                }));
            }
        }
    }

    #[test]
    fn auto_uses_cpu_for_unsupported_or_unverifiable_cuda_but_explicit_cuda_fails() {
        for capability in [(5, 2), (6, 1), (7, 0), (7, 2)] {
            assert!(
                select_cuda_candidate("auto", Some(gpu()), |_| Ok(capability))
                    .unwrap()
                    .is_none()
            );
            let error = select_cuda_candidate("cuda", Some(gpu()), |_| Ok(capability)).unwrap_err();
            assert!(error.contains("minimal 7.5"));
            assert!(error.contains("Pilih CPU atau Auto"));
        }
        for error in ["probe timed out", "compute capability N/A", "UUID mismatch"] {
            assert!(
                select_cuda_candidate("auto", Some(gpu()), |_| Err(error.into()))
                    .unwrap()
                    .is_none()
            );
            assert!(
                select_cuda_candidate("cuda", Some(gpu()), |_| Err(error.into()))
                    .unwrap_err()
                    .contains(error)
            );
        }
        assert!(
            select_cuda_candidate("auto", None, |_| panic!("no GPU to query"))
                .unwrap()
                .is_none()
        );
    }
}
