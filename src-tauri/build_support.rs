use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

pub fn verify_ffmpeg_receipt(binary: &Path, receipt: &Path, version: &str) -> std::io::Result<()> {
    let mut raw = String::new();
    File::open(receipt)?.take(129).read_to_string(&mut raw)?;
    let mut fields = raw.split_whitespace();
    let invalid = || std::io::Error::other("FFmpeg build pin is invalid");
    if raw.len() > 128 || fields.next() != Some(version) {
        return Err(invalid());
    }
    let digest = fields.next().ok_or_else(invalid)?;
    if fields.next().is_some()
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    verify_digest(binary, digest)
}

pub fn verify_digest(path: &Path, expected: &str) -> std::io::Result<()> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 128 * 1024 * 1024 {
        return Err(std::io::Error::other("Invalid downloader file/size"));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > 128 * 1024 * 1024 {
            return Err(std::io::Error::other("Downloader grew beyond limit"));
        }
        hash.update(&buffer[..count]);
    }
    if total != metadata.len() || format!("{:x}", hash.finalize()) != expected {
        return Err(std::io::Error::other("Pinned downloader checksum mismatch"));
    }
    Ok(())
}
