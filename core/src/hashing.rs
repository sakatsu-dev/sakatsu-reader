//! The two conveniences `digest` 0.11 dropped, in one place.
//!
//! Up to `sha2`/`md-5` 0.10 a digest formatted itself as hex (`{:x}`) and a hasher was an
//! `io::Write`, so a file could be `io::copy`-ed into it. 0.11 removed both. Every checksum this
//! project records or compares — catalog pins, content addresses, audio placeholders, evaluation
//! artifacts — is lowercase hex, so the replacement must produce exactly what `{:x}` did.

use std::fmt::Write as _;
use std::io;

/// A digest as lowercase hex, two digits per byte: what `format!("{:x}", digest)` gave.
pub fn hex(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// A borrowed hasher as an `io::Write`, so a reader can be `io::copy`-ed into it.
pub struct Hashing<'a, D>(pub &'a mut D);

impl<D: sha2::Digest> io::Write for Hashing<'_, D> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    #[test]
    fn hex_is_the_familiar_lowercase_checksum() {
        // The SHA-256 of the empty input, as every checksum tool prints it.
        assert_eq!(
            hex(Sha256::digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(hex([0x00, 0x0f, 0xa0]), "000fa0");
    }

    #[test]
    fn copying_into_the_writer_hashes_what_was_read() {
        let mut hash = Sha256::new();
        let copied = io::copy(&mut &b"abc"[..], &mut Hashing(&mut hash)).expect("copy");
        assert_eq!(copied, 3);
        assert_eq!(hex(hash.finalize()), hex(Sha256::digest(b"abc")));
    }
}
