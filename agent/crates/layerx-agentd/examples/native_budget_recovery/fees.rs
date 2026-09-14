use super::{checked, Result};
use layerx_client::evidence::{verify_account_evidence, AccountEvidencePolicy, RootSelector};
use layerx_client::Client;
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_types::{ids::Did, verify::VerificationLevel};
use std::path::Path;

pub struct Authority {
    sequencer: SequencerAuthorization,
    epoch: u64,
}

pub struct Funding {
    pub balance: u128,
    pub sequence: u64,
    pub root: [u8; 32],
}

/// # Errors
/// Returns a refusal if actual native fixture inputs, transport, or evidence are invalid.
/// # Panics
/// Panics when real native results contradict the fixture contract.
pub fn authority(path: &Path, key: [u8; 32], epoch: u64) -> Result<Authority> {
    let source = std::fs::read_to_string(path)?;
    assert!(source.len() <= 4096);
    let lines: Vec<_> = source.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "layerx-sequencer-authority-v1");
    let fields: Vec<_> = lines[1].split(',').collect();
    assert_eq!(fields.len(), 6);
    assert_eq!(fields[5], "active");
    let identity = checked(layerx_programs::hex::decode(fields[0]))?
        .try_into()
        .map_err(|_| "sequencer identity length")?;
    assert_eq!(fields[1], layerx_programs::hex::encode(&key));
    assert_eq!(fields[2].parse::<u64>()?, epoch);
    let first = fields[3].parse::<u64>()?;
    let last = fields[4].parse::<u64>()?;
    assert!(first > 0 && last >= first);
    Ok(Authority {
        sequencer: SequencerAuthorization::new(identity, key, first, last),
        epoch,
    })
}

/// # Errors
/// Returns a refusal if actual native fixture inputs, transport, or evidence are invalid.
/// # Panics
/// Panics when real native results contradict the fixture contract.
pub fn funding(
    client: &mut Client,
    authority: &Authority,
    did: &Did,
    owner: [u8; 32],
) -> Result<Funding> {
    let name = format!("agent:{}:main", std::str::from_utf8(did.as_bytes())?);
    let id = super::fixture::account(&name)?;
    let asset = checked(client.native_fee_policy(8197))?
        .value
        .asset
        .asset_id;
    let account = checked(client.account(
        id,
        VerificationLevel::BATCH_INCLUDED,
        8198,
        authority.sequencer,
    ))?;
    let verified = checked(verify_account_evidence(
        account.canonical_bytes(),
        account.proof_material(),
        id,
        Some(asset),
        AccountEvidencePolicy {
            expected_protocol_version: 3,
            expected_network_id: 77,
            handshake_sequencer_key: authority.sequencer.public_key(),
            root_selector: RootSelector::Latest,
        },
    ))?;
    assert_eq!(
        checked(layerx_wire::receipt::decode_batch_header(
            &verified.signed_header().canonical_bytes
        ))?
        .epoch(),
        authority.epoch
    );
    let value = verified.account();
    assert_eq!(value.name, name.as_bytes());
    assert_eq!(value.kind, 1);
    assert_eq!(value.authority_key, Some(owner));
    assert!(!value.frozen);
    let balance = value.balance();
    assert!(balance > 0);
    let preparation = checked(client.preparation_state(did, 8199))?;
    assert_eq!(
        preparation.observed_head_sequence,
        verified.observed_sequence()
    );
    assert_eq!(preparation.observed_state_root, verified.state_root());
    Ok(Funding {
        balance,
        sequence: verified.observed_sequence(),
        root: verified.state_root(),
    })
}
