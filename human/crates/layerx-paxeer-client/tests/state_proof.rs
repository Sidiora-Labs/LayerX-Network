use layerx_paxeer_client::state_proof::{StateProofError, StateWitness};
use layerx_paxeer_client::{parse_json, Json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn bytes(json: &Json, key: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let text = json
        .member(key)
        .and_then(Json::as_text)
        .and_then(|value| value.strip_prefix("0x"))
        .ok_or("missing hex field")?;
    (0..text.len())
        .step_by(2)
        .map(|i| {
            Ok(u8::from_str_radix(
                text.get(i..i + 2).ok_or("odd hex")?,
                16,
            )?)
        })
        .collect()
}

#[test]
fn native_generated_vectors_and_refusals() -> TestResult {
    let source = include_str!("vectors/native-state-proofs.json");
    assert_eq!(
        source,
        include_str!("../../../../contracts/config/native-state-proofs.json")
    );
    let document = parse_json(source).map_err(|e| format!("{e:?}"))?;
    let Some(Json::Array(vectors)) = document.member("vectors") else {
        return Err("missing vectors".into());
    };
    assert_eq!(vectors.len(), 10);
    for vector in vectors {
        let wire = bytes(vector, "proof")?;
        let root: [u8; 32] = bytes(vector, "root")?
            .try_into()
            .map_err(|_| "root length")?;
        let proof = StateWitness::decode(&wire)?;
        proof.verify(root)?;
        assert_eq!(proof.encode()?, wire);
        for length in 0..wire.len() {
            assert!(StateWitness::decode(&wire[..length]).is_err());
        }
        let mut changed = wire.clone();
        changed.push(0);
        assert!(StateWitness::decode(&changed).is_err());
        changed = wire.clone();
        changed[1] = 1;
        assert_eq!(
            StateWitness::decode(&changed),
            Err(StateProofError::Version)
        );
        changed = wire.clone();
        changed[3] = 10;
        assert_eq!(StateWitness::decode(&changed), Err(StateProofError::Module));
        let mut changed = proof.clone();
        changed.leaf_index_a = changed.leaf_count_a;
        assert_eq!(changed.root(), Err(StateProofError::Path));
        changed = proof.clone();
        changed.siblings_a.push([0; 32]);
        assert_eq!(changed.root(), Err(StateProofError::Path));
        changed = proof.clone();
        changed.leaf_count_a = 0;
        assert_eq!(changed.root(), Err(StateProofError::Path));
        changed = proof.clone();
        changed.key[0] ^= 1;
        assert!(changed.verify(root).is_err());
        changed = proof.clone();
        changed.value[0] ^= 1;
        assert!(changed.verify(root).is_err());
        changed = proof.clone();
        changed.siblings_b[0][0] ^= 1;
        assert!(changed.verify(root).is_err());
        if proof.leaf_count_a == 3 && proof.leaf_index_a == 2 {
            changed = proof.clone();
            changed.siblings_a[0][0] ^= 1;
            assert_eq!(changed.root(), Err(StateProofError::Path));
        }
        let mut wrong_root = root;
        wrong_root[0] ^= 1;
        assert_eq!(proof.verify(wrong_root), Err(StateProofError::Root));
    }
    Ok(())
}

/// The three account vectors (7, 8, 9) are the accounts a forced exit is
/// proven over. Vector 9 carries no account authority key and can therefore
/// never authorise a recipient.
const FIRST_ACCOUNT_VECTOR: usize = 7;
const EXIT_NETWORK: u32 = 7;
const EXIT_ASSET: [u8; 32] = {
    let mut asset = [0_u8; 32];
    asset[0] = 1;
    asset
};

fn exit_evidence(
    witness: &StateWitness,
    wire: Vec<u8>,
    index: usize,
) -> Result<layerx_paxeer_client::ExitEvidence, Box<dyn std::error::Error>> {
    Ok(layerx_paxeer_client::ExitEvidence {
        material: layerx_paxeer_client::ForcedExitMaterial {
            witness: wire,
            batch_number: 1,
            account: witness
                .key
                .get(1..)
                .ok_or("account key")?
                .try_into()
                .map_err(|_| "account")?,
            asset_id: EXIT_ASSET,
            recipient: layerx_types::intent::EvmAddress::new([9; 20]),
            recipient_signature: [0; 64],
        },
        finalised_balance: 100 + u128::try_from(index - FIRST_ACCOUNT_VECTOR)?,
    })
}

#[test]
fn native_recipient_authority_and_signed_domain_refusals() -> TestResult {
    use ed25519_dalek::{Signer as _, SigningKey};
    use layerx_paxeer_client::custody::exit_recipient_message;
    use layerx_paxeer_client::{verify_exit_balance, ExitError, ExitRefusal};
    use layerx_types::intent::EvmAddress;

    let document = parse_json(include_str!("vectors/native-state-proofs.json"))
        .map_err(|e| format!("{e:?}"))?;
    let Some(Json::Array(vectors)) = document.member("vectors") else {
        return Err("missing vectors".into());
    };
    let mut seed = [0; 32];
    seed[0] = 1;
    let key = SigningKey::from_bytes(&seed);
    for (index, vector) in vectors.iter().enumerate().skip(FIRST_ACCOUNT_VECTOR) {
        let wire = bytes(vector, "proof")?;
        let witness = StateWitness::decode(&wire)?;
        let root: [u8; 32] = bytes(vector, "root")?.try_into().map_err(|_| "root")?;
        let mut evidence = exit_evidence(&witness, wire, index)?;
        // The anchor of a forced exit is the finalized state root itself.
        let mut message = exit_recipient_message(
            EXIT_NETWORK,
            &evidence.material.account,
            &evidence.material.asset_id,
            evidence.material.recipient,
            &root,
        );
        assert_eq!(message.get(..22), Some(&b"LX:SETTLE:RECIPIENT:v1"[..]));
        let signature = key.sign(&message).to_bytes();
        evidence.material.recipient_signature = signature;

        if index == 9 {
            // No authority key in the account record: nobody can name a
            // recipient for it, however well formed the signature is.
            assert_eq!(
                verify_exit_balance(&evidence, EXIT_NETWORK, root),
                Err(ExitError::Refused(ExitRefusal::RecipientNotAuthorized))
            );
            continue;
        }
        verify_exit_balance(&evidence, EXIT_NETWORK, root).map_err(|e| format!("{e:?}"))?;

        for position in 0..signature.len() {
            let mut changed = evidence.clone();
            changed.material.recipient_signature[position] ^= 1;
            assert_eq!(
                verify_exit_balance(&changed, EXIT_NETWORK, root),
                Err(ExitError::Refused(ExitRefusal::RecipientNotAuthorized))
            );
        }
        for network in [0, 8] {
            assert_eq!(
                verify_exit_balance(&evidence, network, root),
                Err(ExitError::Refused(ExitRefusal::RecipientNotAuthorized))
            );
        }
        let mut changed = evidence.clone();
        changed.material.recipient = EvmAddress::new([8; 20]);
        assert_eq!(
            verify_exit_balance(&changed, EXIT_NETWORK, root),
            Err(ExitError::Refused(ExitRefusal::RecipientNotAuthorized))
        );
        let other_key = SigningKey::from_bytes(&[2; 32]);
        changed = evidence.clone();
        changed.material.recipient_signature = other_key.sign(&message).to_bytes();
        assert_eq!(
            verify_exit_balance(&changed, EXIT_NETWORK, root),
            Err(ExitError::Refused(ExitRefusal::RecipientNotAuthorized))
        );
        // The domain separator is load bearing: the same fields signed
        // without it authorise nothing.
        message.remove(b"LX:SETTLE:RECIPIENT:v1".len());
        changed = evidence.clone();
        changed.material.recipient_signature = key.sign(&message).to_bytes();
        assert_eq!(
            verify_exit_balance(&changed, EXIT_NETWORK, root),
            Err(ExitError::Refused(ExitRefusal::RecipientNotAuthorized))
        );

        // The witness, not the caller, is the authority on the account, the
        // asset and the whole balance.
        changed = evidence.clone();
        changed.material.account[0] ^= 1;
        assert_eq!(
            verify_exit_balance(&changed, EXIT_NETWORK, root),
            Err(ExitError::Refused(ExitRefusal::NativeBalanceNotProven))
        );
        changed = evidence.clone();
        changed.material.asset_id[31] ^= 1;
        assert_eq!(
            verify_exit_balance(&changed, EXIT_NETWORK, root),
            Err(ExitError::Refused(ExitRefusal::NativeBalanceNotProven))
        );
        changed = evidence.clone();
        changed.finalised_balance += 1;
        assert_eq!(
            verify_exit_balance(&changed, EXIT_NETWORK, root),
            Err(ExitError::Refused(ExitRefusal::NativeBalanceNotProven))
        );
        let mut wrong_root = root;
        wrong_root[0] ^= 1;
        assert_eq!(
            verify_exit_balance(&evidence, EXIT_NETWORK, wrong_root),
            Err(ExitError::Refused(ExitRefusal::NativeBalanceNotProven))
        );
        // Empty consensus bindings never reach the proof at all.
        changed = evidence.clone();
        changed.material.asset_id = [0; 32];
        assert_eq!(
            verify_exit_balance(&changed, EXIT_NETWORK, root),
            Err(ExitError::Refused(ExitRefusal::EmptyAsset))
        );
        changed = evidence.clone();
        changed.material.witness = wrong_root.to_vec();
        assert_eq!(
            verify_exit_balance(&changed, EXIT_NETWORK, root),
            Err(ExitError::Refused(ExitRefusal::Material("witness")))
        );
    }
    Ok(())
}

#[test]
fn forced_exit_material_wire_retains_every_binding() -> TestResult {
    use ed25519_dalek::{Signer as _, SigningKey};
    use layerx_paxeer_client::custody::{exit_recipient_message, MAX_EVIDENCE_BYTES};
    use layerx_paxeer_client::{verify_exit_balance, wire, ForcedExitMaterial};

    let document = parse_json(include_str!("vectors/native-state-proofs.json"))
        .map_err(|e| format!("{e:?}"))?;
    let Some(Json::Array(vectors)) = document.member("vectors") else {
        return Err("vectors".into());
    };
    let vector = vectors.get(7).ok_or("account vector")?;
    let encoded = bytes(vector, "proof")?;
    let root: [u8; 32] = bytes(vector, "root")?.try_into().map_err(|_| "root")?;
    let witness = StateWitness::decode(&encoded)?;
    let mut evidence = exit_evidence(&witness, encoded, 7)?;
    let mut seed = [0; 32];
    seed[0] = 1;
    let key = SigningKey::from_bytes(&seed);
    let message = exit_recipient_message(
        EXIT_NETWORK,
        &evidence.material.account,
        &evidence.material.asset_id,
        evidence.material.recipient,
        &root,
    );
    evidence.material.recipient_signature = key.sign(&message).to_bytes();
    let material = evidence
        .material
        .clone()
        .validated()
        .map_err(|e| format!("{e:?}"))?;

    let wire_bytes = wire::encode_forced_exit_material(&material, MAX_EVIDENCE_BYTES)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        wire::decode_forced_exit_material(&wire_bytes, MAX_EVIDENCE_BYTES)
            .map_err(|e| format!("{e:?}"))?,
        material
    );
    // Every prefix and every trailing byte is refused: the wire is exact.
    for length in 0..wire_bytes.len() {
        assert!(
            wire::decode_forced_exit_material(&wire_bytes[..length], MAX_EVIDENCE_BYTES).is_err()
        );
    }
    let mut trailing = wire_bytes.clone();
    trailing.push(0);
    assert!(wire::decode_forced_exit_material(&trailing, MAX_EVIDENCE_BYTES).is_err());
    assert!(wire::decode_forced_exit_material(&wire_bytes, wire_bytes.len() - 1).is_err());
    assert!(wire::encode_forced_exit_material(&material, wire_bytes.len() - 1).is_err());

    // The codec refuses material that is not a complete consensus binding.
    for field in 0..6 {
        let mut changed: ForcedExitMaterial = material.clone();
        match field {
            0 => changed.batch_number = 0,
            1 => changed.account = [0; 32],
            2 => changed.asset_id = [0; 32],
            3 => changed.recipient = layerx_types::intent::EvmAddress::new([0; 20]),
            4 => changed.recipient_signature = [0; 64],
            _ => changed.witness.clear(),
        }
        assert!(wire::encode_forced_exit_material(&changed, MAX_EVIDENCE_BYTES).is_err());
    }

    // What survives the wire still proves the balance and the recipient.
    let restored = wire::decode_forced_exit_material(&wire_bytes, MAX_EVIDENCE_BYTES)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(restored.witness, witness.encode()?);
    let mut carried = evidence.clone();
    carried.material = restored;
    verify_exit_balance(&carried, EXIT_NETWORK, root).map_err(|e| format!("{e:?}"))?;

    // The two calls the precompile accepts differ only in their selector.
    let request = material.request_calldata();
    let execute = material.execute_calldata();
    assert_ne!(request.get(..4), execute.get(..4));
    assert_eq!(request.get(4..), execute.get(4..));
    Ok(())
}

#[test]
fn native_withdrawal_record_key_is_the_module_nullifier() -> TestResult {
    use layerx_paxeer_client::custody::{exit_withdrawal_id, withdrawal_nullifier};
    use layerx_paxeer_client::{DebitExpectation, DebitFault};
    use layerx_types::intent::EvmAddress;

    let document = parse_json(include_str!("vectors/native-withdrawal-proof.json"))
        .map_err(|e| format!("{e:?}"))?;
    let encoded = bytes(&document, "proof")?;
    let root: [u8; 32] = bytes(&document, "root")?.try_into().map_err(|_| "root")?;
    let witness = StateWitness::decode(&encoded)?;
    witness.verify(root)?;
    let mut wrong_root = root;
    wrong_root[31] ^= 1;
    assert_eq!(witness.verify(wrong_root), Err(StateProofError::Root));

    // The module stores the withdrawal record under its own nullifier.
    let key = &witness.key;
    assert_eq!(key.get(..11), Some(&b"withdrawal:"[..]));
    let recorded_nullifier: [u8; 32] = key.get(11..).ok_or("key")?.try_into()?;

    let value = &witness.value;
    let debit = DebitExpectation {
        activity_id: value.get(6..38).ok_or("record")?.try_into()?,
        network_id: u32::from_be_bytes(value.get(2..6).ok_or("record")?.try_into()?),
        withdrawal_id: value.get(6..38).ok_or("record")?.try_into()?,
        account: value.get(38..70).ok_or("record")?.try_into()?,
        withdrawals_account: bytes(&document, "withdrawals_account")?
            .try_into()
            .map_err(|_| "withdrawals account")?,
        asset_id: value.get(70..102).ok_or("record")?.try_into()?,
        amount: u128::from_be_bytes(value.get(102..118).ok_or("record")?.try_into()?),
        recipient: EvmAddress::new(value.get(130..150).ok_or("record")?.try_into()?),
    }
    .validated()
    .map_err(|e| format!("{e:?}"))?;
    let anchor: [u8; 32] = value.get(150..182).ok_or("record")?.try_into()?;
    assert_eq!(value.get(118..130), Some(&[0_u8; 12][..]));

    // The client's nullifier formula reproduces the module's own state key.
    assert_eq!(
        withdrawal_nullifier(
            debit.network_id,
            &debit.withdrawal_id,
            &debit.account,
            &debit.asset_id,
            debit.amount,
            &anchor,
        ),
        recorded_nullifier
    );

    // Every input to that identifier is load bearing.
    for field in 0..6 {
        let mut altered = debit;
        let mut altered_anchor = anchor;
        match field {
            0 => altered.withdrawal_id[0] ^= 1,
            1 => altered.account[0] ^= 1,
            2 => altered.asset_id[31] ^= 1,
            3 => altered.amount += 1,
            4 => altered.network_id += 1,
            _ => altered_anchor[0] ^= 1,
        }
        assert_ne!(
            withdrawal_nullifier(
                altered.network_id,
                &altered.withdrawal_id,
                &altered.account,
                &altered.asset_id,
                altered.amount,
                &altered_anchor,
            ),
            recorded_nullifier
        );
    }

    // A recorded withdrawal is not a forced exit: the emergency identifier of
    // the same account and anchor is a different withdrawal.
    assert_ne!(
        exit_withdrawal_id(debit.network_id, &debit.account, &debit.asset_id, &anchor),
        debit.withdrawal_id
    );

    // The record's own bindings are the ones the boundary refuses when empty.
    for field in 0..7 {
        let mut altered = debit;
        let name = match field {
            0 => {
                altered.withdrawal_id = [0; 32];
                "withdrawal_id"
            }
            1 => {
                altered.account = [0; 32];
                "account"
            }
            2 => {
                altered.withdrawals_account = [0; 32];
                "withdrawals_account"
            }
            3 => {
                altered.asset_id = [0; 32];
                "asset_id"
            }
            4 => {
                altered.network_id = 0;
                "network_id"
            }
            5 => {
                altered.amount = 0;
                "amount"
            }
            _ => {
                altered.recipient = EvmAddress::new([0; 20]);
                "recipient"
            }
        };
        assert_eq!(altered.validated(), Err(DebitFault::EmptyField(name)));
    }
    Ok(())
}
