use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

pub fn check(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::SeqCst) {
        Err("Proses dibatalkan.".into())
    } else {
        Ok(())
    }
}

/// Match only cancellation errors emitted by our workers, never arbitrary logs.
pub fn is_cancellation_error(error: &str) -> bool {
    matches!(
        error.trim(),
        "Job dibatalkan." | "Transkripsi dibatalkan." | "Proses dibatalkan."
    )
}

pub fn copy(
    input: &mut impl Read,
    output: &mut impl Write,
    cancelled: &AtomicBool,
) -> Result<u64, String> {
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0;
    loop {
        check(cancelled)?;
        let count = match input.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result.map_err(|e| format!("Gagal membaca file: {e}"))?,
        };
        check(cancelled)?;
        if count == 0 {
            return Ok(total);
        }
        output
            .write_all(&buffer[..count])
            .map_err(|e| format!("Gagal menulis file: {e}"))?;
        total += count as u64;
    }
}

pub fn file_sha256(path: &Path, cancelled: &AtomicBool) -> Result<String, String> {
    check(cancelled)?;
    let mut file =
        File::open(path).map_err(|e| format!("Gagal membuka file untuk verifikasi: {e}"))?;
    hash_reader(&mut file, cancelled)
}

fn hash_reader(input: &mut impl Read, cancelled: &AtomicBool) -> Result<String, String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        check(cancelled)?;
        let count = match input.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result.map_err(|e| format!("Gagal membaca file untuk verifikasi: {e}"))?,
        };
        check(cancelled)?;
        if count == 0 {
            return Ok(format!("{:x}", hasher.finalize()));
        }
        hasher.update(&buffer[..count]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct CancelOnRead<'a>(&'a AtomicBool);
    impl Read for CancelOnRead<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            buffer[0] = b'x';
            self.0.store(true, Ordering::SeqCst);
            Ok(1)
        }
    }
    #[test]
    fn copy_and_hash_stop_when_cancelled_inside_a_read() {
        let cancelled = AtomicBool::new(false);
        let mut output = Vec::new();
        assert!(copy(&mut CancelOnRead(&cancelled), &mut output, &cancelled)
            .unwrap_err()
            .contains("dibatalkan"));
        assert!(output.is_empty());
        cancelled.store(false, Ordering::SeqCst);
        assert!(hash_reader(&mut CancelOnRead(&cancelled), &cancelled)
            .unwrap_err()
            .contains("dibatalkan"));
    }
    #[test]
    fn ordinary_copy_and_hash_keep_exact_bytes_and_checksum() {
        let cancelled = AtomicBool::new(false);
        let mut output = Vec::new();
        assert_eq!(copy(&mut &b"abc"[..], &mut output, &cancelled).unwrap(), 3);
        assert_eq!(output, b"abc");
        assert_eq!(
            hash_reader(&mut &b"abc"[..], &cancelled).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
