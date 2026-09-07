use jetrun_common::models::Compression;

/// Threshold for switching from LZ4 to Zstd (64KB)
const ZSTD_THRESHOLD: usize = 64 * 1024;

/// Auto-select compression algorithm based on data size.
/// - <256 bytes: no compression (overhead not worth it)
/// - <64KB: LZ4 (very fast, moderate ratio)
/// - >=64KB: Zstd level 3 (good ratio, still fast)
pub fn auto_select(data_len: usize) -> Compression {
    if data_len < 256 {
        Compression::None
    } else if data_len < ZSTD_THRESHOLD {
        Compression::Lz4
    } else {
        Compression::Zstd
    }
}

/// Compress data using the specified algorithm.
pub fn compress(data: &[u8], method: Compression) -> anyhow::Result<Vec<u8>> {
    match method {
        Compression::None => Ok(data.to_vec()),
        Compression::Zstd => {
            // Level 3: good balance of speed and compression ratio
            Ok(zstd::encode_all(std::io::Cursor::new(data), 3)?)
        }
        Compression::Lz4 => Ok(lz4_flex::compress_prepend_size(data)),
    }
}

/// Decompress data.
pub fn decompress(data: &[u8], method: Compression) -> anyhow::Result<Vec<u8>> {
    match method {
        Compression::None => Ok(data.to_vec()),
        Compression::Zstd => Ok(zstd::decode_all(std::io::Cursor::new(data))?),
        Compression::Lz4 => lz4_flex::decompress_size_prepended(data)
            .map_err(|e| anyhow::anyhow!("LZ4 decompress error: {}", e)),
    }
}

/// Get the compression ratio (original_size / compressed_size).
pub fn ratio(original_len: usize, compressed_len: usize) -> f64 {
    if compressed_len == 0 {
        return 0.0;
    }
    original_len as f64 / compressed_len as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_select() {
        assert_eq!(auto_select(100) as u8, Compression::None as u8);
        assert_eq!(auto_select(1000) as u8, Compression::Lz4 as u8);
        assert_eq!(auto_select(100_000) as u8, Compression::Zstd as u8);
    }

    #[test]
    fn test_compress_decompress_zstd() {
        let data = b"hello world ".repeat(1000);
        let compressed = compress(&data, Compression::Zstd).unwrap();
        assert!(compressed.len() < data.len());

        let decompressed = decompress(&compressed, Compression::Zstd).unwrap();
        assert_eq!(decompressed, data);

        let r = ratio(data.len(), compressed.len());
        assert!(r > 1.0, "ratio should be > 1.0, got {}", r);
    }

    #[test]
    fn test_compress_decompress_lz4() {
        let data = b"repeated data! ".repeat(500);
        let compressed = compress(&data, Compression::Lz4).unwrap();
        assert!(compressed.len() < data.len());

        let decompressed = decompress(&compressed, Compression::Lz4).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_compress_decompress_none() {
        let data = b"raw data";
        let compressed = compress(data, Compression::None).unwrap();
        assert_eq!(compressed, data);

        let decompressed = decompress(&compressed, Compression::None).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_zstd_better_than_lz4_on_large_data() {
        let data = b"The quick brown fox jumps over the lazy dog. ".repeat(10000);
        let zstd_compressed = compress(&data, Compression::Zstd).unwrap();
        let lz4_compressed = compress(&data, Compression::Lz4).unwrap();

        // Zstd should compress better than LZ4 on larger data
        assert!(
            zstd_compressed.len() <= lz4_compressed.len(),
            "zstd={} lz4={}",
            zstd_compressed.len(),
            lz4_compressed.len()
        );
    }
}
