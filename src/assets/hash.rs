//! Content fingerprints for asset URLs, shared with `build.rs`.

/// FNV-1a (64-bit): a stable, dependency-free fingerprint for cache busting, not for security.
#[must_use]
pub const fn fingerprint(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    hash
}

/// Inserts the content fingerprint before the extension: `app.css` becomes `app.<hash>.css`.
#[must_use]
pub fn hashed_file_name(name: &str, bytes: &[u8]) -> String {
    let hash = format!("{:016x}", fingerprint(bytes));
    match name.rsplit_once('.') {
        Some((stem, extension)) => format!("{stem}.{hash}.{extension}"),
        None => format!("{name}.{hash}"),
    }
}
