use std::fs::File;
use std::io::Read;
use std::path::Path;
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256};

/// All four checksums computed in one pass (MD5, SHA1, SHA256).
/// SHA512 is omitted unless explicitly requested (matches Rufus behaviour).
pub struct ChecksumResult {
    pub md5: String,
    pub sha1: String,
    pub sha256: String,
}

pub fn compute_all_checksums(path: &Path) -> Result<ChecksumResult, String> {
    let mut file = File::open(path)
        .map_err(|e| format!("Failed to open file: {}", e))?;

    let mut md5_h   = Md5::new();
    let mut sha1_h  = Sha1::new();
    let mut sha256_h = Sha256::new();

    let mut buffer = vec![0u8; 1024 * 1024]; // 1 MB chunks

    loop {
        let n = file.read(&mut buffer)
            .map_err(|e| format!("Error reading file: {}", e))?;
        if n == 0 { break; }
        md5_h.update(&buffer[..n]);
        sha1_h.update(&buffer[..n]);
        sha256_h.update(&buffer[..n]);
    }

    Ok(ChecksumResult {
        md5:    format!("{:x}", md5_h.finalize()),
        sha1:   format!("{:x}", sha1_h.finalize()),
        sha256: format!("{:x}", sha256_h.finalize()),
    })
}

/// Legacy single-hash helper kept for backward compat.
pub fn compute_sha256(path: &Path) -> Result<String, String> {
    compute_all_checksums(path).map(|r| r.sha256)
}
