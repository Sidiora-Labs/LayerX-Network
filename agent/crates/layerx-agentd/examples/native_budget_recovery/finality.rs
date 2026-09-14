use super::{checked, Result};
use layerx_client::{evidence::FinalityEvidenceCandidate, Client};
use std::io::Read as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;

const MAXIMUM: u64 = 1_048_576 + 96 * 1024;

fn material(directory: &Path, value: &serde_json::Value) -> Result<Vec<u8>> {
    let path = Path::new(value.as_str().ok_or("finality material path missing")?);
    let allowed = directory.join("proofs").canonicalize()?;
    let path = path.canonicalize()?;
    if !path.starts_with(&allowed) {
        return Err("finality material outside proof directory".into());
    }
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != 4021
        || metadata.nlink() != 1
        || metadata.mode() & 0o777 != 0o600
        || metadata.len() == 0
        || metadata.len() > MAXIMUM
    {
        return Err("finality material bounds or ownership refused".into());
    }
    let mut bytes = Vec::new();
    file.take(MAXIMUM + 1).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len())? != metadata.len() {
        return Err("finality material changed while reading".into());
    }
    Ok(bytes)
}

/// # Errors
/// Returns a refusal if actual native fixture inputs, transport, or evidence are invalid.
/// # Panics
/// Panics when real native results contradict the fixture contract.
pub fn register(
    client: &mut Client,
    directory: &Path,
    response: &serde_json::Value,
    previous: &mut u64,
    target: u64,
) -> Result<()> {
    let records = response["registrations"]
        .as_array()
        .ok_or("native finality registrations missing")?;
    if records.is_empty()
        || records.len() > 128
        || target <= *previous
        || target > 128
        || u64::try_from(records.len())? != target - *previous
    {
        return Err("native finality registration range refused".into());
    }
    for record in records {
        if record
            .as_object()
            .ok_or("registration object missing")?
            .len()
            != 4
        {
            return Err("unexpected registration fields".into());
        }
        let expected = previous.checked_add(1).ok_or("registration overflow")?;
        assert_eq!(record["batch"], expected);
        let candidate = checked(FinalityEvidenceCandidate::from_exact_bytes(
            material(directory, &record["checkpoint"])?,
            material(directory, &record["finality"])?,
            3,
            77,
        ))?;
        assert_eq!(
            record["checkpoint_id"]
                .as_str()
                .ok_or("checkpoint identity missing")?,
            layerx_programs::hex::encode(&candidate.checkpoint_id())
        );
        let result = checked(client.register_finality_evidence(&candidate, 8500 + expected))?;
        assert_eq!(result.batch_number, expected);
        assert_eq!(result.checkpoint_id, candidate.checkpoint_id());
        *previous = expected;
    }
    assert_eq!(*previous, target);
    Ok(())
}
