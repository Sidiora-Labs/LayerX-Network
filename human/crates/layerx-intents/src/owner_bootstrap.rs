use super::*;
use layerx_types::activity::TimestampBound;
use layerx_types::intent::{ApprovalThreshold, PublicKey, RecoveryRoot};
use layerx_wire::decode::Decoder;

pub enum NativeOwnerBootstrap {
    Identity { did: Did, primary_key: PublicKey },
    RotationPolicy { did: Did, pending_key: PublicKey, challenge: TimestampBound, effective_sequence: u64 },
    RecoveryPolicy { did: Did, root: RecoveryRoot, threshold: ApprovalThreshold, minimum_delay: u64, maximum_delay: u64 },
}

impl NativeOwnerBootstrap {
    /// # Errors
    /// Refuses invalid native owner fields, unavailable ordinals, or non-canonical round trips.
    pub fn compile(&self, registry: &ModuleRegistry) -> Result<CompiledIntent, CompileError> {
        let (did, ordinal, count, key, numbers) = match self {
            Self::Identity { did, primary_key } => (did, 1, 2, primary_key.bytes(), Vec::new()),
            Self::RotationPolicy { did, pending_key, challenge, effective_sequence } => {
                if *effective_sequence == 0 { return Err(native_invalid()); }
                (did, 2, 4, pending_key.bytes(), vec![challenge.not_before(), challenge.not_after(), *effective_sequence])
            }
            Self::RecoveryPolicy { did, root, threshold, minimum_delay, maximum_delay } => {
                if root.is_zero() || *minimum_delay == 0 || maximum_delay < minimum_delay {
                    return Err(native_invalid());
                }
                (did, 3, 5, root.bytes(), vec![u64::from(threshold.value()), *minimum_delay, *maximum_delay])
            }
        };
        if key == [0; 32] { return Err(native_invalid()); }
        let did = hash::did_id_for_protocol(did, 3).map_err(|error| CompileError::wire(CompileField::Did, error))?;
        let tag = 0x7100 | ordinal;
        let mut encoder = Encoder::new(MAX_PAYLOAD_BYTES);
        header(&mut encoder, tag, count)?;
        fixed(&mut encoder, &did, CompileField::Did)?;
        fixed(&mut encoder, &key, CompileField::PrimaryKey)?;
        for (index, number) in numbers.iter().enumerate() {
            if ordinal == 3 && index == 0 {
                wire(CompileField::Threshold, encoder.u16(u16::try_from(*number).map_err(|_| native_invalid())?))?;
            } else {
                wire(CompileField::Sequence, encoder.u64(*number))?;
            }
        }
        let compiled = finish(registry, ModuleId::Governance, ordinal, encoder)?;
        let mut decoder = Decoder::new(compiled.payload().as_bytes());
        let invalid = |_| native_invalid();
        if decoder.u16().map_err(invalid)? != tag || decoder.u16().map_err(invalid)? != count
            || decoder.fixed(32).map_err(invalid)? != did || decoder.fixed(32).map_err(invalid)? != key
        { return Err(native_invalid()); }
        for (index, number) in numbers.iter().enumerate() {
            let actual = if ordinal == 3 && index == 0 {
                u64::from(decoder.u16().map_err(invalid)?)
            } else { decoder.u64().map_err(invalid)? };
            if actual != *number { return Err(native_invalid()); }
        }
        decoder.finish().map_err(invalid)?;
        Ok(compiled)
    }
}
