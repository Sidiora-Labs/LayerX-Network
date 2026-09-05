use layerx_programs_runtime::abi::{Capability, CapabilitySet};
use layerx_programs_runtime::accounts::derive_program_account;
use layerx_programs_runtime::storage::ProgramId;
use serde_json::{json, Value};

fn grant_json(grant: &Capability) -> Value {
    match grant {
        Capability::StorageRead => json!({"tag": 1, "kind": "StorageRead"}),
        Capability::StorageWrite => json!({"tag": 2, "kind": "StorageWrite"}),
        Capability::EmitEvent => json!({"tag": 3, "kind": "EmitEvent"}),
        Capability::Call { program } => {
            json!({"tag": 4, "kind": "Call", "program": hex::encode(program.bytes())})
        }
        Capability::Transfer402 {
            asset,
            to,
            maximum_amount,
        } => {
            json!({"tag": 5, "kind": "Transfer402", "asset": hex::encode(asset), "to": hex::encode(to), "maximum_amount": maximum_amount.to_string()})
        }
        Capability::ProgramSpend {
            owner_program,
            seed,
            source_account,
            asset,
            to,
            maximum_amount,
        } => {
            json!({"tag": 9, "kind": "ProgramSpend", "owner_program": hex::encode(owner_program.bytes()), "seed": hex::encode(seed), "source_account": hex::encode(source_account), "asset": hex::encode(asset), "to": hex::encode(to), "maximum_amount": maximum_amount.to_string()})
        }
        Capability::ReceiptRead { receipt_digest } => {
            json!({"tag": 6, "kind": "ReceiptRead", "receipt_digest": hex::encode(receipt_digest)})
        }
        Capability::BalanceView {
            account,
            asset,
            receipt_digest,
        } => {
            json!({"tag": 10, "kind": "BalanceView", "account": hex::encode(account), "asset": hex::encode(asset), "receipt_digest": hex::encode(receipt_digest)})
        }
        Capability::SharedStorageRead => json!({"tag": 7, "kind": "SharedStorageRead"}),
        Capability::SharedStorageWrite => json!({"tag": 8, "kind": "SharedStorageWrite"}),
    }
}

fn canonical_grants(set: &CapabilitySet) -> Result<Vec<Capability>, String> {
    CapabilitySet::decode_v2_canonical(&set.canonical_encoding())
        .map_err(|error| format!("{error:?}"))
}

fn fixture() -> Result<Value, String> {
    let owner = ProgramId::new([0x11; 32]).map_err(|error| format!("{error:?}"))?;
    let seed = b"fixture-seed".to_vec();
    let source = derive_program_account(owner, &seed)
        .map_err(|error| format!("{error:?}"))?
        .bytes();
    let grants = vec![
        Capability::StorageRead,
        Capability::StorageWrite,
        Capability::EmitEvent,
        Capability::Call { program: owner },
        Capability::Transfer402 {
            asset: [0x22; 32],
            to: [0x33; 32],
            maximum_amount: u128::MAX,
        },
        Capability::ProgramSpend {
            owner_program: owner,
            seed,
            source_account: source,
            asset: [0x22; 32],
            to: [0x33; 32],
            maximum_amount: 1000,
        },
        Capability::ReceiptRead {
            receipt_digest: [0x44; 32],
        },
        Capability::BalanceView {
            account: source,
            asset: [0x22; 32],
            receipt_digest: [0x44; 32],
        },
        Capability::SharedStorageRead,
        Capability::SharedStorageWrite,
    ];
    let parent = CapabilitySet::new(grants).map_err(|error| format!("{error:?}"))?;
    let ordered = canonical_grants(&parent)?;
    let mut requests = ordered.clone();
    for grant in &mut requests {
        match grant {
            Capability::Transfer402 { maximum_amount, .. } => *maximum_amount = 500,
            Capability::ProgramSpend { maximum_amount, .. } => *maximum_amount = 500,
            _ => {}
        }
    }
    let narrowed = parent
        .narrow(requests)
        .map_err(|error| format!("{error:?}"))?;
    let narrowed_grants = canonical_grants(&narrowed)?;
    let mut escalation_cases = Vec::new();
    for (name, tag) in [
        ("transfer_amount_increase", 5),
        ("program_spend_amount_increase", 9),
        ("balance_receipt_substitution", 10),
    ] {
        let mut requested = narrowed_grants.clone();
        for grant in &mut requested {
            match grant {
                Capability::Transfer402 { maximum_amount, .. } if tag == 5 => *maximum_amount = 501,
                Capability::ProgramSpend { maximum_amount, .. } if tag == 9 => {
                    *maximum_amount = 501
                }
                Capability::BalanceView { receipt_digest, .. } if tag == 10 => {
                    *receipt_digest = [0x55; 32]
                }
                _ => {}
            }
        }
        if narrowed.narrow(requested.clone()).is_ok() {
            return Err(format!("escalation accepted: {name}"));
        }
        let requested_set =
            CapabilitySet::new(requested.clone()).map_err(|error| format!("{error:?}"))?;
        escalation_cases.push(json!({"name": name, "parent": "narrowed", "capabilities": requested.iter().map(grant_json).collect::<Vec<_>>(), "canonical_hex": hex::encode(requested_set.canonical_encoding()), "accepted": false}));
    }
    let equal = parent
        .narrow(ordered.clone())
        .map_err(|error| format!("{error:?}"))?;
    if equal.canonical_encoding() != parent.canonical_encoding() {
        return Err("equal narrowing changed canonical bytes".into());
    }
    Ok(json!({
        "name": "native-program-capabilities-v2",
        "provenance": {"producer": "programs/crates/layerx-programs-runtime/examples/capability_fixture.rs", "encoder": "layerx_programs_runtime::abi::CapabilitySet::canonical_encoding", "abi_version": 2, "seed_length_encoding": "u16be"},
        "capabilities": ordered.iter().map(grant_json).collect::<Vec<_>>(),
        "canonical_hex": hex::encode(parent.canonical_encoding()),
        "narrowed_capabilities": narrowed_grants.iter().map(grant_json).collect::<Vec<_>>(),
        "narrowed_hex": hex::encode(narrowed.canonical_encoding()),
        "equal_narrowing_accepted": true,
        "escalation_cases": escalation_cases
    }))
}

fn main() -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(&fixture()?).map_err(|error| error.to_string())?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::fixture;

    #[test]
    fn fixture_is_reproducible_and_covers_canonical_order_and_escalations() {
        let value = fixture().unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(value, fixture().unwrap_or_else(|error| panic!("{error}")));
        let tags: Vec<_> = value["capabilities"]
            .as_array()
            .unwrap_or_else(|| panic!("grants"))
            .iter()
            .map(|grant| grant["tag"].clone())
            .collect();
        assert_eq!(
            serde_json::json!(tags),
            serde_json::json!([1, 2, 3, 4, 5, 9, 6, 10, 7, 8])
        );
        assert_eq!(value["escalation_cases"].as_array().map(Vec::len), Some(3));
        assert_eq!(value["equal_narrowing_accepted"], true);
    }
}
