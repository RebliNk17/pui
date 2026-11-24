//! Lowercase hexadecimal encoding for digests (sha2 0.11 outputs no longer implement `LowerHex`).

use std::fmt::Write as _;

pub fn lower(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    #[test]
    fn matches_previous_lower_hex_digest_format() {
        assert_eq!(super::lower([0x00, 0x0f, 0xab]), "000fab");
        assert_eq!(
            super::lower(Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
