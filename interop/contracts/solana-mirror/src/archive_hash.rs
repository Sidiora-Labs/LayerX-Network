use solana_program::program_error::ProgramError;

use crate::MirrorError;

const INITIAL: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

pub(crate) struct ArchiveHash {
    state: [u32; 8],
    tail: [u8; 64],
    length: usize,
}

impl ArchiveHash {
    pub(crate) const fn new() -> Self {
        Self {
            state: INITIAL,
            tail: [0; 64],
            length: 0,
        }
    }

    pub(crate) fn load(bytes: &[u8], received: u64) -> Result<Self, ProgramError> {
        if bytes.len() != 97 {
            return Err(MirrorError::Conflict.into());
        }
        let length = usize::from(bytes[96]);
        if length >= 64
            || received % 64 != length as u64
            || bytes[32 + length..96].iter().any(|byte| *byte != 0)
        {
            return Err(MirrorError::Conflict.into());
        }
        let mut state = [0; 8];
        for (word, encoded) in state.iter_mut().zip(bytes[..32].chunks_exact(4)) {
            *word = u32::from_be_bytes(encoded.try_into().map_err(|_| MirrorError::Conflict)?);
        }
        if received < 64 && state != INITIAL {
            return Err(MirrorError::Conflict.into());
        }
        Ok(Self {
            state,
            tail: bytes[32..96]
                .try_into()
                .map_err(|_| MirrorError::Conflict)?,
            length,
        })
    }

    pub(crate) fn store(&self, bytes: &mut [u8]) -> Result<(), ProgramError> {
        if bytes.len() != 97 || self.length >= 64 {
            return Err(MirrorError::Conflict.into());
        }
        for (word, encoded) in self.state.iter().zip(bytes[..32].chunks_exact_mut(4)) {
            encoded.copy_from_slice(&word.to_be_bytes());
        }
        bytes[32..96].copy_from_slice(&self.tail);
        bytes[96] = u8::try_from(self.length).map_err(|_| MirrorError::Bounds)?;
        Ok(())
    }

    pub(crate) fn update(&mut self, mut bytes: &[u8]) -> Result<(), ProgramError> {
        if self.length > 0 {
            let copied = (64 - self.length).min(bytes.len());
            self.tail[self.length..self.length + copied].copy_from_slice(&bytes[..copied]);
            self.length += copied;
            bytes = &bytes[copied..];
            if self.length < 64 {
                return Ok(());
            }
            sha2::compress256(&mut self.state, &[self.tail.into()]);
            self.tail.fill(0);
            self.length = 0;
        }
        let mut blocks = bytes.chunks_exact(64);
        for block in &mut blocks {
            let block: [u8; 64] = block.try_into().map_err(|_| MirrorError::Bounds)?;
            sha2::compress256(&mut self.state, &[block.into()]);
        }
        let remaining = blocks.remainder();
        self.tail[..remaining.len()].copy_from_slice(remaining);
        self.length = remaining.len();
        Ok(())
    }

    pub(crate) fn finish(mut self, received: u64) -> Result<[u8; 32], ProgramError> {
        if received % 64 != self.length as u64 || self.length >= 64 {
            return Err(MirrorError::Conflict.into());
        }
        let bits = received.checked_mul(8).ok_or(MirrorError::Bounds)?;
        self.tail[self.length] = 0x80;
        if self.length >= 56 {
            sha2::compress256(&mut self.state, &[self.tail.into()]);
            self.tail.fill(0);
        }
        self.tail[56..].copy_from_slice(&bits.to_be_bytes());
        sha2::compress256(&mut self.state, &[self.tail.into()]);
        let mut digest = [0; 32];
        for (word, encoded) in self.state.iter().zip(digest.chunks_exact_mut(4)) {
            encoded.copy_from_slice(&word.to_be_bytes());
        }
        Ok(digest)
    }
}

#[cfg(test)]
mod tests {
    use super::ArchiveHash;
    use sha2::{Digest, Sha256};

    #[test]
    fn resumed_partitioned_hash_matches_sha256_at_every_padding_boundary() {
        let bytes: Vec<_> = (0..2161).map(|index| (index % 251) as u8).collect();
        for length in [
            0, 1, 55, 56, 63, 64, 65, 119, 120, 127, 128, 719, 720, 721, 2161,
        ] {
            for chunk_size in [1, 17, 55, 56, 63, 64, 65, 128, 719, 720] {
                let mut state = ArchiveHash::new();
                let mut received = 0;
                for chunk in bytes[..length].chunks(chunk_size) {
                    state.update(chunk).expect("bounded input hash");
                    received += chunk.len() as u64;
                    let mut persisted = [0; 97];
                    state.store(&mut persisted).expect("persist hash state");
                    state = ArchiveHash::load(&persisted, received).expect("resume hash state");
                }
                assert_eq!(
                    state.finish(received).expect("final hash"),
                    <[u8; 32]>::from(Sha256::digest(&bytes[..length]))
                );
            }
        }
    }

    #[test]
    fn malformed_tail_and_initial_state_are_refused() {
        let mut encoded = [0; 97];
        ArchiveHash::new()
            .store(&mut encoded)
            .expect("initial state");
        let original = encoded;
        encoded[96] = 64;
        assert!(ArchiveHash::load(&encoded, 64).is_err());
        encoded = original;
        encoded[32] = 1;
        assert!(ArchiveHash::load(&encoded, 0).is_err());
        encoded = original;
        encoded[0] ^= 1;
        assert!(ArchiveHash::load(&encoded, 0).is_err());
        assert!(ArchiveHash::load(&original, 1).is_err());
    }
}
