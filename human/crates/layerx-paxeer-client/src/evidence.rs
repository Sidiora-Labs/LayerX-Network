use crate::{EndpointConfig, EndpointFailure, EndpointFault};
use layerx_types::intent::EvmAddress;
use sha2::{Digest as _, Sha256};

pub(crate) const LIMIT: usize = 1_048_576;
pub(crate) const MAX_ITEMS: usize = 4096;

pub(crate) fn invalid() -> EndpointFault {
    EndpointFault::InconsistentObservation
}
pub(crate) fn failure(endpoint: &EndpointConfig, fault: EndpointFault) -> EndpointFailure {
    EndpointFailure {
        url: endpoint.url.clone(),
        fault,
    }
}
pub(crate) fn word(value: usize) -> Result<[u8; 32], EndpointFault> {
    let mut out = [0; 32];
    out[24..].copy_from_slice(&u64::try_from(value).map_err(|_| invalid())?.to_be_bytes());
    Ok(out)
}
pub(crate) fn word_number(value: &[u8]) -> Result<usize, EndpointFault> {
    if value.len() != 32 || value[..24] != [0; 24] {
        return Err(invalid());
    }
    usize::try_from(u64::from_be_bytes(
        value[24..].try_into().map_err(|_| invalid())?,
    ))
    .map_err(|_| invalid())
}
pub(crate) fn dynamic(value: &[u8]) -> Result<Vec<u8>, EndpointFault> {
    let mut out = word(value.len())?.to_vec();
    out.extend_from_slice(value);
    out.resize(out.len().div_ceil(32) * 32, 0);
    Ok(out)
}
pub(crate) fn abi(head: &[[u8; 32]], tails: &[Vec<u8>]) -> Result<Vec<u8>, EndpointFault> {
    let mut out = Vec::new();
    let mut offset = (head.len() + tails.len()) * 32;
    for value in head {
        out.extend_from_slice(value);
    }
    for tail in tails {
        out.extend_from_slice(&word(offset)?);
        offset = offset.checked_add(tail.len()).ok_or_else(invalid)?;
    }
    for tail in tails {
        out.extend_from_slice(tail);
    }
    if out.len() > LIMIT {
        return Err(invalid());
    }
    Ok(out)
}
pub(crate) fn split_dynamic(
    input: &[u8],
    head_words: usize,
    index: usize,
) -> Result<&[u8], EndpointFault> {
    let offset = word_number(
        input
            .get(index * 32..(index + 1) * 32)
            .ok_or_else(invalid)?,
    )?;
    if offset < head_words * 32 || offset % 32 != 0 {
        return Err(invalid());
    }
    let length_end = offset.checked_add(32).ok_or_else(invalid)?;
    let size = word_number(input.get(offset..length_end).ok_or_else(invalid)?)?;
    let start = offset.checked_add(32).ok_or_else(invalid)?;
    let end = start.checked_add(size).ok_or_else(invalid)?;
    input.get(start..end).ok_or_else(invalid)
}

pub(crate) use layerx_paxeer_verifier::{publication, publication_at};

pub(crate) fn digest(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

pub(crate) struct Registered {
    pub state_root: [u8; 32],
    pub sender: [u8; 20],
}
pub(crate) fn registered(
    endpoint: &EndpointConfig,
    registry: EvmAddress,
    checkpoint: [u8; 32],
    confirmations: u64,
) -> Result<Registered, EndpointFailure> {
    // layerxAnchor `CheckpointSubmitted(uint64 indexed batchNumber, bytes32
    // indexed checkpointId, bytes32 stateRoot, bytes32 receiptRoot, uint8
    // signers)`: its transaction sender is the checkpoint's proposer, the only
    // account layerxCustody lets register the deposit root.
    let value = publication_at(
        endpoint,
        registry,
        SUBMITTED_TOPIC,
        2,
        checkpoint,
        confirmations,
    )?;
    let run = || {
        if value.topics.len() != 3 || value.data.len() != 96 {
            return Err(invalid());
        }
        // The batch is indexed as a canonical quantity; a publication that
        // encodes it noncanonically is not this checkpoint's submission.
        u64::try_from(word_number(&value.topics[1])?).map_err(|_| invalid())?;
        Ok(Registered {
            state_root: value.data[..32].try_into().map_err(|_| invalid())?,
            sender: value.sender,
        })
    };
    run().map_err(|e| failure(endpoint, e))
}
pub(crate) struct Reader<'a>(pub &'a [u8]);
impl<'a> Reader<'a> {
    pub fn take(&mut self, len: usize) -> Result<&'a [u8], EndpointFault> {
        let out = self.0.get(..len).ok_or_else(invalid)?;
        self.0 = &self.0[len..];
        Ok(out)
    }
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], EndpointFault> {
        self.take(N)?.try_into().map_err(|_| invalid())
    }
    pub fn finish(self) -> Result<(), EndpointFault> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}
pub(crate) const DEPOSIT_SELECTOR: [u8; 4] = [0x8f, 0xf1, 0xfa, 0xc9];
pub(crate) const DEPOSIT_TOPIC: [u8; 32] = [
    0xdc, 0x7b, 0x7d, 0xbc, 0xfc, 0x1d, 0xc6, 0x57, 0xc, 0x57, 0xd3, 0xd6, 0x41, 0x3b, 0x4a, 0x7f,
    0xd3, 0xc1, 0xaa, 0x6, 0x8b, 0x1a, 0x8a, 0x45, 0x91, 0xd, 0x76, 0xe6, 0x5a, 0x35, 0xb0, 0xbc,
];
pub(crate) const SUBMITTED_TOPIC: [u8; 32] = [
    0xf7, 0x32, 0xef, 0xc9, 0xdf, 0x2e, 0x75, 0x89, 0x89, 0x88, 0x99, 0xf8, 0x5c, 0xa5, 0xe6, 0xcb,
    0x25, 0xfa, 0x1c, 0x61, 0x94, 0x61, 0xd9, 0x6e, 0x81, 0xe8, 0x2b, 0xe9, 0xcc, 0xf1, 0x44, 0x16,
];
