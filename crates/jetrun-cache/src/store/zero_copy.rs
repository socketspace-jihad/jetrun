use std::path::Path;

use bytes::Bytes;
use memmap2::Mmap;

/// Memory-map a file and return it as zero-copy Bytes.
///
/// Uses `Bytes::from_owner(Mmap)` so the mmap is held alive by the Bytes
/// reference count. No data is copied — reads go directly through the
/// kernel page cache.
///
/// # Safety
/// The file must not be modified while the Bytes is alive.
/// This is safe for our CAS store since blobs are immutable.
pub fn mmap_to_bytes(path: &Path) -> anyhow::Result<Bytes> {
    let file = std::fs::File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    Ok(Bytes::from_owner(mmap))
}

/// Compute blake3 hash of a file using mmap + SIMD parallel hashing.
/// This is significantly faster than reading chunks through async I/O
/// because blake3's `update_mmap` uses:
/// - Memory mapping (no read() syscalls)
/// - SIMD instructions (AVX2/AVX-512/NEON)
/// - Rayon parallel hashing for large files (>128KB)
pub fn hash_file_mmap(path: &Path) -> anyhow::Result<String> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_mmap(path)?;
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_mmap_to_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bin");
        let data = b"hello mmap world";
        std::fs::write(&path, data).unwrap();

        let bytes = mmap_to_bytes(&path).unwrap();
        assert_eq!(&bytes[..], data);
    }

    #[test]
    fn test_mmap_to_bytes_large() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.bin");
        let data: Vec<u8> = (0..1_000_000).map(|i| (i % 256) as u8).collect();
        std::fs::write(&path, &data).unwrap();

        let bytes = mmap_to_bytes(&path).unwrap();
        assert_eq!(bytes.len(), 1_000_000);
        assert_eq!(&bytes[..100], &data[..100]);
    }

    #[test]
    fn test_hash_file_mmap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hashme.bin");
        let data = b"content to hash";
        std::fs::write(&path, data).unwrap();

        let hash = hash_file_mmap(&path).unwrap();
        let expected = blake3::hash(data).to_hex().to_string();
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_hash_file_mmap_large() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        let chunk = vec![42u8; 65536];
        for _ in 0..16 {
            f.write_all(&chunk).unwrap();
        }
        drop(f);

        // Should not panic and should produce a valid hash
        let hash = hash_file_mmap(&path).unwrap();
        assert_eq!(hash.len(), 64); // blake3 hex hash is 64 chars
    }
}
