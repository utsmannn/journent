//! Agent key generation + verification.
//!
//! Format key: `<prefix>_<64 hex>` (32 random bytes). Store sha256(key).
//! Verify by hashing incoming key and comparing.

use rand::RngCore;
use sha2::{Digest, Sha256};

pub fn hash_key(key: &str) -> String {
    let mut h = Sha256::new();
    h.update(key.as_bytes());
    hex::encode(h.finalize())
}

/// Generate a fresh agent key. Returns (full_key, key_prefix_for_display).
pub fn generate_full_key(prefix: &str) -> (String, String) {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let hex_part = hex::encode(bytes);
    let full = format!("{}_{}", prefix, hex_part);
    let display_prefix = format!("{}_{}…", prefix, &hex_part[..8]);
    (full, display_prefix)
}

/// Constant-time-ish compare of two hex sha256 hashes.
pub fn verify(key_plain: &str, key_hash: &str) -> bool {
    let computed = hash_key(key_plain);
    computed == key_hash
}

/// Extract bearer token from "Authorization: Bearer <key>".
pub fn extract_bearer(headers: &axum::http::HeaderMap) -> Option<String> {
    let hv = headers.get(axum::http::header::AUTHORIZATION)?;
    let s = hv.to_str().ok()?;
    let s = s.trim();
    let token = s.strip_prefix("Bearer ").or_else(|| s.strip_prefix("bearer "))?;
    let t = token.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}
