//! Shared bounded-memory hashing for release archives and frozen executables.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    fs::File,
    io::{self, Read},
    path::Path,
};

const BUFFER_BYTES: usize = 32 * 1024;

#[derive(Debug)]
pub struct InputTooLarge {
    pub limit: u64,
}

impl fmt::Display for InputTooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "hash input exceeds {} bytes", self.limit)
    }
}

impl std::error::Error for InputTooLarge {}

pub fn reader(mut input: impl Read, limit: Option<u64>) -> Result<String> {
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; BUFFER_BYTES];
    let mut total = 0_u64;
    loop {
        let length = match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(length) => length,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error).context("read hash input"),
        };
        total = total
            .checked_add(length as u64)
            .context("hash input size overflow")?;
        if let Some(limit) = limit
            && total > limit
        {
            return Err(InputTooLarge { limit }.into());
        }
        digest.update(&buffer[..length]);
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in digest.finalize() {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 15) as usize] as char);
    }
    Ok(output)
}

pub fn file(path: &Path) -> Result<String> {
    reader(
        File::open(path).with_context(|| format!("open hash input {}", path.display()))?,
        None,
    )
}

#[cfg(test)]
pub fn bytes(input: &[u8]) -> String {
    reader(input, None).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_hash_covers_empty_known_and_partial_chunks() -> Result<()> {
        assert_eq!(
            bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let data = vec![0x5A; BUFFER_BYTES * 3 + 17];
        let reference: String = Sha256::digest(&data)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(reader(data.as_slice(), Some(data.len() as u64))?, reference);
        assert!(
            reader(data.as_slice(), Some(data.len() as u64 - 1))
                .unwrap_err()
                .is::<InputTooLarge>()
        );
        Ok(())
    }

    #[test]
    fn hash_reads_are_bounded_and_interrupted_reads_are_retried() -> Result<()> {
        struct Probe {
            remaining: usize,
            largest: usize,
            interrupted: bool,
        }
        impl Read for Probe {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                self.largest = self.largest.max(buffer.len());
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(io::ErrorKind::Interrupted.into());
                }
                let count = self.remaining.min(buffer.len());
                buffer[..count].fill(42);
                self.remaining -= count;
                Ok(count)
            }
        }
        let mut probe = Probe {
            remaining: BUFFER_BYTES * 128 + 7,
            largest: 0,
            interrupted: false,
        };
        assert_eq!(reader(&mut probe, None)?.len(), 64);
        assert_eq!(probe.remaining, 0);
        assert_eq!(probe.largest, BUFFER_BYTES);
        Ok(())
    }
}
