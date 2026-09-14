use crate::{raw_call, CheckpointProof, EndpointConfig, EndpointFailure, EndpointFault, Json};
use layerx_types::intent::EvmAddress;
use sha2::{Digest as _, Sha256};

pub(crate) const LIMIT: usize = 1_048_576;
pub(crate) const MAX_ITEMS: usize = 4096;
pub(crate) const WITHDRAWALS: &[u8] = b"LXP/Paxeer/withdrawal-witnesses/v1\0";
pub(crate) const WITHDRAWALS_V2: &[u8] = b"LXP/Paxeer/withdrawal-witnesses/v2\0";
pub(crate) const BALANCES_V2: &[u8] = b"LXP/Paxeer/balance-witnesses/v2\0";
pub(crate) const BALANCES: &[u8] = b"LXP/Paxeer/balance-witnesses/v1\0";

pub(crate) fn invalid() -> EndpointFault {
    EndpointFault::InconsistentObservation
}
pub(crate) fn failure(endpoint: &EndpointConfig, fault: EndpointFault) -> EndpointFailure {
    EndpointFailure {
        url: endpoint.url.clone(),
        fault,
    }
}
pub(crate) fn required<'a>(value: &'a Json, name: &str) -> Result<&'a Json, EndpointFault> {
    value.member(name).ok_or_else(invalid)
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("0x");
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}
pub(crate) fn bytes(value: &Json) -> Result<Vec<u8>, EndpointFault> {
    let text = value
        .as_text()
        .and_then(|s| s.strip_prefix("0x"))
        .ok_or_else(invalid)?;
    if text.len() % 2 != 0 || text.len() > LIMIT * 2 {
        return Err(invalid());
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| invalid())?;
            u8::from_str_radix(text, 16).map_err(|_| invalid())
        })
        .collect()
}
pub(crate) fn fixed<const N: usize>(value: &Json) -> Result<[u8; N], EndpointFault> {
    bytes(value)?.try_into().map_err(|_| invalid())
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

pub(crate) use layerx_paxeer_verifier::publication;

pub(crate) fn digest(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

pub(crate) struct Registered {
    pub state_root: [u8; 32],
    pub epoch: u64,
    pub batch: u64,
    pub da: [u8; 32],
    pub sender: [u8; 20],
}
pub(crate) fn registered(
    endpoint: &EndpointConfig,
    registry: EvmAddress,
    checkpoint: [u8; 32],
    confirmations: u64,
) -> Result<Registered, EndpointFailure> {
    let value = publication(
        endpoint,
        registry,
        REGISTERED_TOPIC,
        checkpoint,
        confirmations,
    )?;
    let run = || {
        if value.topics.len() != 4 || value.data.len() != 192 {
            return Err(invalid());
        }
        Ok(Registered {
            state_root: value.data[96..128].try_into().map_err(|_| invalid())?,
            epoch: u64::try_from(word_number(&value.topics[2])?).map_err(|_| invalid())?,
            batch: u64::try_from(word_number(&value.topics[3])?).map_err(|_| invalid())?,
            da: value.data[128..160].try_into().map_err(|_| invalid())?,
            sender: value.sender,
        })
    };
    run().map_err(|e| failure(endpoint, e))
}
pub(crate) fn witnesses(
    endpoint: &EndpointConfig,
    registry: EvmAddress,
    checkpoint: [u8; 32],
    confirmations: u64,
) -> Result<(Vec<u8>, Vec<u8>, Registered), EndpointFailure> {
    let registered = registered(endpoint, registry, checkpoint, confirmations)?;
    let published = publication(
        endpoint,
        registry,
        WITNESSES_TOPIC,
        checkpoint,
        confirmations,
    )?;
    let run = || {
        if published.sender != registered.sender
            || published.topics.len() != 2
            || published.data.len() != 64
            || !matches!(word_number(&published.data[..32])?, 1 | 2)
            || !published.input.starts_with(&WITNESSES_SELECTOR)
        {
            return Err(invalid());
        }
        let args = &published.input[4..];
        if args.get(..32) != Some(&checkpoint) {
            return Err(invalid());
        }
        let withdrawals = split_dynamic(args, 3, 1)?;
        let balances = split_dynamic(args, 3, 2)?;
        let version = word_number(&published.data[..32])?;
        let tags = if version == 2 {
            (WITHDRAWALS_V2, BALANCES_V2)
        } else {
            (WITHDRAWALS, BALANCES)
        };
        items(tags.0, withdrawals)?;
        items(tags.1, balances)?;
        let tails = [dynamic(withdrawals)?, dynamic(balances)?];
        if abi(&[checkpoint], &tails)? != args
            || digest(&abi(&[checkpoint, word(version)?], &tails)?) != published.data[32..]
        {
            return Err(invalid());
        }
        Ok((withdrawals.to_vec(), balances.to_vec(), registered))
    };
    run().map_err(|e| failure(endpoint, e))
}
pub(crate) fn bind(
    proof: &CheckpointProof,
    checkpoint: [u8; 32],
    registered: &Registered,
) -> Result<(), EndpointFault> {
    if proof.checkpoint_hash != checkpoint
        || proof.state_root != registered.state_root
        || proof.epoch != registered.epoch
        || proof.batch_number != registered.batch
        || proof.data_availability_root != registered.da
    {
        return Err(invalid());
    }
    Ok(())
}
pub(crate) fn vector(tag: &[u8], items: &[Vec<u8>]) -> Result<Vec<u8>, EndpointFault> {
    if items.len() > MAX_ITEMS {
        return Err(invalid());
    }
    let mut out = tag.to_vec();
    out.extend_from_slice(
        &u32::try_from(items.len())
            .map_err(|_| invalid())?
            .to_be_bytes(),
    );
    for item in items {
        out.extend_from_slice(
            &u32::try_from(item.len())
                .map_err(|_| invalid())?
                .to_be_bytes(),
        );
        out.extend_from_slice(item);
        if out.len() > LIMIT {
            return Err(invalid());
        }
    }
    Ok(out)
}
pub(crate) fn items<'a>(tag: &[u8], bytes: &'a [u8]) -> Result<Vec<&'a [u8]>, EndpointFault> {
    if bytes.len() > LIMIT {
        return Err(invalid());
    }
    let mut r = Reader(bytes.strip_prefix(tag).ok_or_else(invalid)?);
    let count = r.number()?;
    if count > MAX_ITEMS {
        return Err(invalid());
    }
    let mut out = Vec::new();
    for _ in 0..count {
        let len = r.number()?;
        out.push(r.take(len)?);
    }
    r.finish()?;
    Ok(out)
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
    pub fn number(&mut self) -> Result<usize, EndpointFault> {
        usize::try_from(u32::from_be_bytes(self.array()?)).map_err(|_| invalid())
    }
    pub fn finish(self) -> Result<(), EndpointFault> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}
pub(crate) const WITNESSES_SELECTOR: [u8; 4] = [0x69, 0x92, 0x97, 0x38];
pub(crate) const DEPOSIT_SELECTOR: [u8; 4] = [0x8f, 0xf1, 0xfa, 0xc9];
pub(crate) const WITNESSES_TOPIC: [u8; 32] = [
    0x8e, 0x3f, 0xa1, 0x7f, 0xc3, 0x5, 0x84, 0x80, 0x5c, 0xf0, 0x62, 0x3e, 0xf3, 0xf, 0xbf, 0x1,
    0xef, 0x3, 0x7, 0x71, 0x6a, 0x79, 0x9, 0xcf, 0xd3, 0x7c, 0x6f, 0xef, 0xb4, 0x76, 0xb9, 0x80,
];
pub(crate) const DEPOSIT_TOPIC: [u8; 32] = [
    0xdc, 0x7b, 0x7d, 0xbc, 0xfc, 0x1d, 0xc6, 0x57, 0xc, 0x57, 0xd3, 0xd6, 0x41, 0x3b, 0x4a, 0x7f,
    0xd3, 0xc1, 0xaa, 0x6, 0x8b, 0x1a, 0x8a, 0x45, 0x91, 0xd, 0x76, 0xe6, 0x5a, 0x35, 0xb0, 0xbc,
];
pub(crate) const REGISTERED_TOPIC: [u8; 32] = [
    0x9, 0x4d, 0x6, 0x13, 0x2b, 0xe9, 0xf, 0x15, 0x44, 0xeb, 0xa6, 0x3f, 0xf4, 0xd5, 0xf, 0xf3,
    0x21, 0x69, 0x50, 0xfc, 0xa4, 0x91, 0x2b, 0x3d, 0x46, 0x9d, 0x48, 0x2f, 0xbf, 0x88, 0x26, 0x1c,
];
