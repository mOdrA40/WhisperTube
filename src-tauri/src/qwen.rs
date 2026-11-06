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
    candidates: &[crate::system::GpuInfo],
    cancelled: &std::sync::atomic::AtomicBool,
    mut capability: impl FnMut(&crate::system::GpuInfo) -> Result<(u32, u32), String>,
) -> Result<Option<crate::system::GpuInfo>, String> {
    crate::io_cancel::check(cancelled)?;
    for gpu in candidates {
        crate::io_cancel::check(cancelled)?;
        let compatibility =
            crate::models::ensure_vram_available(MODEL_ID, gpu.available_memory_mb())
                .and_then(|()| capability(gpu))
                .and_then(|capability| {
                    // Pinned v0.4.1 bindings/python-native-cu12/pyproject.toml builds
                    // CMAKE_CUDA_ARCHITECTURES=75;80;86;89;90 (including forward-compatible
                    // PTX). Whisper CUDA support does not imply Qwen support.
                    if capability >= (7, 5) {
                        Ok(())
                    } else {
                        Err(format!(
                            "{} memiliki compute capability {}.{}, tetapi runtime Qwen 0.4.1 memerlukan minimal 7.5.",
                            gpu.name, capability.0, capability.1
                        ))
                    }
                });
        // Cancellation during a hardware probe must not become CPU fallback.
        crate::io_cancel::check(cancelled)?;
        match compatibility {
            Ok(()) => return Ok(Some(gpu.clone())),
            Err(error) if requested == "auto" => {
                eprintln!(
                    "WhisperTube: kandidat CUDA Qwen {} dilewati: {error}",
                    gpu.name
                );
            }
            Err(error) => {
                return Err(format!(
                    "CUDA Qwen tidak dapat digunakan: {error} Pilih CPU atau Auto."
                ));
            }
        }
    }
    Ok(None)
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
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        for requested in ["auto", "cuda"] {
            for capability in [(7, 5), (8, 0), (8, 6), (8, 9), (9, 0), (12, 0)] {
                let selected =
                    select_cuda_candidate(requested, &[gpu()], &cancelled, |candidate| {
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
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        for capability in [(5, 2), (6, 1), (7, 0), (7, 2)] {
            assert!(
                select_cuda_candidate("auto", &[gpu()], &cancelled, |_| Ok(capability))
                    .unwrap()
                    .is_none()
            );
            let error = select_cuda_candidate("cuda", &[gpu()], &cancelled, |_| Ok(capability))
                .unwrap_err();
            assert!(error.contains("minimal 7.5"));
            assert!(error.contains("Pilih CPU atau Auto"));
        }
        for error in ["probe timed out", "compute capability N/A", "UUID mismatch"] {
            assert!(
                select_cuda_candidate("auto", &[gpu()], &cancelled, |_| Err(error.into()))
                    .unwrap()
                    .is_none()
            );
            assert!(
                select_cuda_candidate("cuda", &[gpu()], &cancelled, |_| Err(error.into()))
                    .unwrap_err()
                    .contains(error)
            );
        }
        assert!(
            select_cuda_candidate("auto", &[], &cancelled, |_| panic!("no GPU to query"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn auto_tries_other_visible_gpus_but_explicit_cuda_stays_on_its_target() {
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let mut old = gpu();
        old.name = "Legacy GPU".into();
        old.total_memory_mb = Some(11264);
        old.free_memory_mb = Some(10240);
        old.uuid = Some("GPU-bbbbbbbb".into());
        old.device_index = Some(7);
        let devices = [old, gpu()];
        let mut probed = Vec::new();
        let selected = select_cuda_candidate("auto", &devices, &cancelled, |gpu| {
            probed.push(gpu.device_index);
            Ok(if gpu.device_index == Some(7) {
                (6, 1)
            } else {
                (8, 6)
            })
        })
        .unwrap()
        .unwrap();
        assert_eq!(probed, [Some(7), Some(3)]);
        assert_eq!(selected.uuid.as_deref(), Some("GPU-aaaaaaaa"));
        let mut command = std::process::Command::new("qwen-worker");
        crate::system::configure_cuda_command(&mut command, &selected).unwrap();
        assert!(command
            .get_envs()
            .any(|(key, value)| key == "CUDA_VISIBLE_DEVICES"
                && value == Some(std::ffi::OsStr::new("GPU-aaaaaaaa"))));
        assert!(
            select_cuda_candidate("cuda", &devices[..1], &cancelled, |_| Ok((6, 1)))
                .unwrap_err()
                .contains("minimal 7.5")
        );
        assert_eq!(
            select_cuda_candidate("cuda", &devices[1..], &cancelled, |_| Ok((8, 6)))
                .unwrap()
                .unwrap()
                .device_index,
            Some(3)
        );
        // The caller supplies the already visibility-filtered inventory.
        assert!(
            select_cuda_candidate("auto", &devices[..1], &cancelled, |_| Ok((6, 1)))
                .unwrap()
                .is_none()
        );
        let mut attempts = 0;
        assert!(select_cuda_candidate("auto", &devices, &cancelled, |_| {
            attempts += 1;
            Ok((6, 1))
        })
        .unwrap()
        .is_none());
        assert_eq!(attempts, devices.len());
    }

    #[test]
    fn auto_skips_insufficient_or_unknown_vram_and_failed_capability_probes() {
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        for free in [None, Some(1024)] {
            let mut unavailable = gpu();
            unavailable.free_memory_mb = free;
            unavailable.device_index = Some(9);
            let selected =
                select_cuda_candidate("auto", &[unavailable.clone(), gpu()], &cancelled, |gpu| {
                    assert_ne!(
                        gpu.device_index,
                        Some(9),
                        "VRAM must be checked before probing"
                    );
                    Ok((8, 6))
                })
                .unwrap()
                .unwrap();
            assert_eq!(selected.device_index, Some(3));
            assert!(
                select_cuda_candidate("cuda", &[unavailable], &cancelled, |_| panic!("VRAM guard"))
                    .is_err()
            );
        }
        let mut first = gpu();
        first.device_index = Some(9);
        let selected = select_cuda_candidate("auto", &[first, gpu()], &cancelled, |gpu| {
            if gpu.device_index == Some(9) {
                Err("probe unavailable".into())
            } else {
                Ok((8, 6))
            }
        })
        .unwrap()
        .unwrap();
        assert_eq!(selected.device_index, Some(3));
    }

    #[test]
    fn cancellation_before_or_during_selection_never_becomes_fallback() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let cancelled = AtomicBool::new(true);
        assert!(
            select_cuda_candidate("auto", &[gpu()], &cancelled, |_| panic!("cancelled"))
                .unwrap_err()
                .contains("dibatalkan")
        );
        cancelled.store(false, Ordering::SeqCst);
        let error = select_cuda_candidate("auto", &[gpu(), gpu()], &cancelled, |_| {
            cancelled.store(true, Ordering::SeqCst);
            Err("probe interrupted".into())
        })
        .unwrap_err();
        assert!(error.contains("dibatalkan"));
    }
}
