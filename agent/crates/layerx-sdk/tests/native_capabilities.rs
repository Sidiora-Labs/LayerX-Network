use layerx_sdk::native_capabilities::{
    derive_native_program_account, NativeCapability, NativeCapabilitySet,
};

fn bytes(value: &serde_json::Value) -> Result<Vec<u8>, String> {
    let encoded = value.as_str().ok_or("hex string absent")?;
    if encoded.len() % 2 != 0
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("fixture hex".into());
    }
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(
                std::str::from_utf8(pair).map_err(|error| error.to_string())?,
                16,
            )
            .map_err(|error| error.to_string())
        })
        .collect()
}

fn fixed(value: &serde_json::Value) -> Result<[u8; 32], String> {
    bytes(value)?
        .try_into()
        .map_err(|_| "fixture identifier length".into())
}

fn logical(value: &serde_json::Value) -> Result<NativeCapabilitySet, String> {
    let mut grants = Vec::new();
    for grant in value.as_array().ok_or("logical grants absent")? {
        let amount = || {
            grant["maximum_amount"]
                .as_str()
                .ok_or("amount absent")?
                .parse::<u128>()
                .map_err(|error| error.to_string())
        };
        grants.push(match grant["kind"].as_str().ok_or("kind absent")? {
            "StorageRead" => NativeCapability::StorageRead,
            "StorageWrite" => NativeCapability::StorageWrite,
            "EmitEvent" => NativeCapability::EmitEvent,
            "Call" => NativeCapability::Call {
                program: fixed(&grant["program"])?,
            },
            "Transfer402" => NativeCapability::Transfer402 {
                asset: fixed(&grant["asset"])?,
                to: fixed(&grant["to"])?,
                maximum_amount: amount()?,
            },
            "ProgramSpend" => NativeCapability::ProgramSpend {
                owner_program: fixed(&grant["owner_program"])?,
                seed: bytes(&grant["seed"])?,
                source_account: fixed(&grant["source_account"])?,
                asset: fixed(&grant["asset"])?,
                to: fixed(&grant["to"])?,
                maximum_amount: amount()?,
            },
            "ReceiptRead" => NativeCapability::ReceiptRead {
                receipt_digest: fixed(&grant["receipt_digest"])?,
            },
            "BalanceView" => NativeCapability::BalanceView {
                account: fixed(&grant["account"])?,
                asset: fixed(&grant["asset"])?,
                receipt_digest: fixed(&grant["receipt_digest"])?,
            },
            "SharedStorageRead" => NativeCapability::SharedStorageRead,
            "SharedStorageWrite" => NativeCapability::SharedStorageWrite,
            _ => return Err("unknown logical grant".into()),
        });
    }
    NativeCapabilitySet::new(grants).map_err(|error| format!("{error:?}"))
}

fn fixture() -> Result<serde_json::Value, String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../platform/sdk/conformance/fixtures/native-program-capabilities-v2.json");
    serde_json::from_slice(&std::fs::read(path).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}

#[test]
fn runtime_logical_grants_pin_bytes_and_narrowing() -> Result<(), String> {
    let fixture = fixture()?;
    let parent = logical(&fixture["capabilities"])?;
    let canonical = bytes(&fixture["canonical_hex"])?;
    assert_eq!(parent.grants().len(), 10);
    assert_eq!(
        parent.encode().map_err(|error| format!("{error:?}"))?,
        canonical
    );
    assert_eq!(
        NativeCapabilitySet::decode(&canonical).map_err(|error| format!("{error:?}"))?,
        parent
    );
    let narrowed = logical(&fixture["narrowed_capabilities"])?;
    assert_eq!(
        narrowed.encode().map_err(|error| format!("{error:?}"))?,
        bytes(&fixture["narrowed_hex"])?
    );
    assert_eq!(
        parent
            .narrow(narrowed.clone())
            .map_err(|error| format!("{error:?}"))?,
        narrowed
    );
    assert_eq!(fixture["equal_narrowing_accepted"], true);
    assert_eq!(
        parent
            .narrow(parent.clone())
            .map_err(|error| format!("{error:?}"))?,
        parent
    );
    let cases = fixture["escalation_cases"]
        .as_array()
        .ok_or("escalation cases absent")?;
    assert_eq!(cases.len(), 3);
    for case in cases {
        assert_eq!(case["parent"], "narrowed");
        assert_eq!(case["accepted"], false);
        let child = logical(&case["capabilities"])?;
        assert_eq!(
            child.encode().map_err(|error| format!("{error:?}"))?,
            bytes(&case["canonical_hex"])?
        );
        assert!(narrowed.narrow(child).is_err());
    }
    for length in 0..canonical.len() {
        assert!(NativeCapabilitySet::decode(&canonical[..length]).is_err());
    }
    let mut trailing = canonical.clone();
    trailing.push(0);
    assert!(NativeCapabilitySet::decode(&trailing).is_err());
    let mut unknown = canonical.clone();
    unknown[2] = 11;
    assert!(NativeCapabilitySet::decode(&unknown).is_err());
    let mut reordered = canonical;
    reordered.swap(2, 3);
    assert!(NativeCapabilitySet::decode(&reordered).is_err());
    let mut duplicate = parent.grants().to_vec();
    duplicate.push(duplicate[0].clone());
    assert!(NativeCapabilitySet::new(duplicate).is_err());
    Ok(())
}

#[test]
fn fixture_grant_mutations_pin_count_seed_and_view_bounds() -> Result<(), String> {
    let fixture = fixture()?;
    let parent = logical(&fixture["capabilities"])?;
    let spend = parent
        .grants()
        .iter()
        .find(|grant| matches!(grant, NativeCapability::ProgramSpend { .. }))
        .ok_or("spend absent")?;
    let mut grants = Vec::new();
    for index in 0..238u16 {
        let mut grant = spend.clone();
        if let NativeCapability::ProgramSpend {
            owner_program,
            seed,
            source_account,
            ..
        } = &mut grant
        {
            *seed = vec![0; 128];
            seed[..2].copy_from_slice(&index.to_be_bytes());
            *source_account = derive_native_program_account(*owner_program, seed)
                .map_err(|error| format!("{error:?}"))?;
        }
        grants.push(grant);
    }
    let maximum = NativeCapabilitySet::new(grants.clone()).map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        maximum
            .encode()
            .map_err(|error| format!("{error:?}"))?
            .len(),
        65_452
    );
    grants.push(NativeCapability::Call { program: [1; 32] });
    assert!(NativeCapabilitySet::new(grants).is_err());
    let view = parent
        .grants()
        .iter()
        .find(|grant| matches!(grant, NativeCapability::BalanceView { .. }))
        .ok_or("view absent")?;
    let mut views = Vec::new();
    for index in 1..=33u8 {
        let mut grant = view.clone();
        if let NativeCapability::BalanceView { account, .. } = &mut grant {
            *account = [index; 32];
        }
        views.push(grant);
        if index == 32 {
            assert!(NativeCapabilitySet::new(views.clone()).is_ok());
        }
    }
    assert!(NativeCapabilitySet::new(views).is_err());
    for original in parent.grants() {
        let mut grant = original.clone();
        match &mut grant {
            NativeCapability::ProgramSpend { source_account, .. } => source_account[0] ^= 1,
            NativeCapability::Transfer402 { maximum_amount, .. } => *maximum_amount = 0,
            NativeCapability::BalanceView { receipt_digest, .. } => *receipt_digest = [0; 32],
            _ => continue,
        }
        assert!(NativeCapabilitySet::new(vec![grant]).is_err());
    }
    let mut long_seed = spend.clone();
    if let NativeCapability::ProgramSpend { seed, .. } = &mut long_seed {
        *seed = vec![0; 129];
    }
    assert!(NativeCapabilitySet::new(vec![long_seed]).is_err());
    Ok(())
}
