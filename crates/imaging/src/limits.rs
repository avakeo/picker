/// Safety limits applied before and while decoding untrusted image files.
#[derive(Debug, Clone, Copy)]
pub struct DecodeLimits {
    /// Maximum compressed file size in bytes.
    pub max_file_bytes: u64,
    /// Maximum width or height in pixels.
    pub max_edge: u32,
    /// Maximum total pixels after decoding.
    pub max_pixels: u64,
    /// Maximum decoded allocation in bytes.
    pub max_alloc_bytes: u64,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 200 * 1024 * 1024,
            max_edge: 32_768,
            max_pixels: 100_000_000,
            max_alloc_bytes: 512 * 1024 * 1024,
        }
    }
}
