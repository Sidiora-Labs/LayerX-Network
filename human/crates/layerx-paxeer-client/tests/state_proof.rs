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

#[test]
fn native_recipient_authority_and_signed_domain_refusals() -> TestResult {
    use ed25519_dalek::{Signer as _, SigningKey};
    use layerx_paxeer_client::{ExitError, ExitRefusal};
    use layerx_types::intent::EvmAddress;

    let document = parse_json(include_str!("vectors/native-state-proofs.json"))
        .map_err(|e| format!("{e:?}"))?;
    let Some(Json::Array(vectors)) = document.member("vectors") else {
        return Err("missing vectors".into());
    };
    let mut seed = [0; 32];
    seed[0] = 1;
    let key = SigningKey::from_bytes(&seed);
    for (index, vector) in vectors.iter().enumerate().skip(7) {
        let witness = StateWitness::decode(&bytes(vector, "proof")?)?;
        let root: [u8; 32] = bytes(vector, "root")?.try_into().map_err(|_| "root")?;
        let evidence = native_exit_evidence(&witness, index)?;
        let anchor = [3; 32];
        let mut message = b"LX:SETTLE:RECIPIENT:v1\x00".to_vec();
        message.extend_from_slice(&7_u32.to_be_bytes());
        message.extend_from_slice(&evidence.account);
        message.extend_from_slice(&evidence.asset_id);
        message.extend_from_slice(&evidence.recipient.bytes());
        message.extend_from_slice(&anchor);
        let signature = key.sign(&message).to_bytes();
        if index == 9 {
            assert_eq!(
                evidence.verify_native_balance(&witness, root, 7, anchor, &signature),
                Err(ExitError::Refused(ExitRefusal::RecipientNotAuthorized))
            );
            continue;
        }
        evidence
            .verify_native_balance(&witness, root, 7, anchor, &signature)
            .map_err(|e| format!("{e:?}"))?;
        for position in 0..signature.len() {
            let mut changed = signature;
            changed[position] ^= 1;
            assert!(evidence
                .verify_native_balance(&witness, root, 7, anchor, &changed)
                .is_err());
        }
        for network in [0, 8] {
            assert!(evidence
                .verify_native_balance(&witness, root, network, anchor, &signature)
                .is_err());
        }
        for anchor in [[0; 32], [4; 32]] {
            assert!(evidence
                .verify_native_balance(&witness, root, 7, anchor, &signature)
                .is_err());
        }
        let mut changed = evidence.clone();
        changed.recipient = EvmAddress::new([8; 20]);
        assert!(changed
            .verify_native_balance(&witness, root, 7, anchor, &signature)
            .is_err());
        changed = evidence.clone();
        changed.account[0] ^= 1;
        assert!(changed
            .verify_native_balance(&witness, root, 7, anchor, &signature)
            .is_err());
        changed = evidence.clone();
        changed.asset_id[0] ^= 1;
        assert!(changed
            .verify_native_balance(&witness, root, 7, anchor, &signature)
            .is_err());
        changed = evidence.clone();
        changed.finalised_balance += 1;
        assert!(changed
            .verify_native_balance(&witness, root, 7, anchor, &signature)
            .is_err());
        let mut wrong_root = root;
        wrong_root[0] ^= 1;
        assert!(evidence
            .verify_native_balance(&witness, wrong_root, 7, anchor, &signature)
            .is_err());
        let other_key = SigningKey::from_bytes(&[2; 32]);
        assert!(evidence
            .verify_native_balance(
                &witness,
                root,
                7,
                anchor,
                &other_key.sign(&message).to_bytes()
            )
            .is_err());
        message.remove(b"LX:SETTLE:RECIPIENT:v1".len());
        assert!(evidence
            .verify_native_balance(&witness, root, 7, anchor, &key.sign(&message).to_bytes())
            .is_err());
    }
    Ok(())
}

fn native_exit_evidence(
    witness: &StateWitness,
    index: usize,
) -> Result<layerx_paxeer_client::ExitEvidence, Box<dyn std::error::Error>> {
    Ok(layerx_paxeer_client::ExitEvidence {
        native: None,
        account: witness.key[1..].try_into().map_err(|_| "account")?,
        asset_id: {
            let mut id = [0; 32];
            id[0] = 1;
            id
        },
        finalised_balance: 100 + u128::try_from(index - 7)?,
        recipient: layerx_types::intent::EvmAddress::new([9; 20]),
        leaf_index: 0,
        siblings: Vec::new(),
        attestations: Vec::new(),
    })
}

#[test]
fn native_evidence_wire_retains_two_checkpoints_and_signed_binding() -> TestResult {
    use ed25519_dalek::{Signer as _, SigningKey};
    use layerx_paxeer_client::{state_proof::NativeEvidence, wire};
    let document = parse_json(include_str!("vectors/native-state-proofs.json"))
        .map_err(|e| format!("{e:?}"))?;
    let Some(Json::Array(vectors)) = document.member("vectors") else {
        return Err("vectors".into());
    };
    let encoded = bytes(&vectors[7], "proof")?;
    let witness = StateWitness::decode(&encoded)?;
    let mut evidence = native_exit_evidence(&witness, 7)?;
    let mut seed = [0; 32];
    seed[0] = 1;
    let key = SigningKey::from_bytes(&seed);
    let anchor = [3; 32];
    let mut message = b"LX:SETTLE:RECIPIENT:v1\0".to_vec();
    message.extend_from_slice(&7_u32.to_be_bytes());
    message.extend_from_slice(&evidence.account);
    message.extend_from_slice(&evidence.asset_id);
    message.extend_from_slice(&evidence.recipient.bytes());
    message.extend_from_slice(&anchor);
    let native = NativeEvidence {
        request_anchor: anchor,
        inclusion_checkpoint: [4; 32],
        network_id: 7,
        witness: encoded,
        recipient_signature: key.sign(&message).to_bytes().to_vec(),
    };
    evidence.native = Some(native.clone());
    let encoded = wire::encode_native_evidence(&native, 1_048_576).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        wire::decode_native_evidence(&encoded, 1_048_576).map_err(|e| format!("{e:?}"))?,
        native
    );
    assert_ne!(native.request_anchor, native.inclusion_checkpoint);
    for length in 0..encoded.len() {
        assert!(wire::decode_native_evidence(&encoded[..length], 1_048_576).is_err());
    }
    let mut trailing = encoded.clone();
    trailing.push(0);
    assert!(wire::decode_native_evidence(&trailing, 1_048_576).is_err());
    let mut wrong = native.clone();
    wrong.recipient_signature.pop();
    assert!(wire::encode_native_evidence(&wrong, 1_048_576).is_err());
    wrong = native.clone();
    wrong.inclusion_checkpoint = [0; 32];
    assert!(wire::encode_native_evidence(&wrong, 1_048_576).is_err());
    assert!(wire::decode_native_evidence(&encoded, encoded.len() - 1).is_err());
    evidence
        .verify_native_balance(
            &witness,
            witness.root()?,
            native.network_id,
            native.request_anchor,
            native.recipient_signature.as_slice().try_into()?,
        )
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

#[test]
fn native_withdrawal_record_matches_replayed_module_fact() -> TestResult {
    use layerx_paxeer_client::{state_proof::NativeEvidence, CheckpointProof, DebitExpectation};
    use layerx_types::intent::EvmAddress;
    let document = parse_json(include_str!("vectors/native-withdrawal-proof.json"))
        .map_err(|e| format!("{e:?}"))?;
    let encoded = bytes(&document, "proof")?;
    let root = bytes(&document, "root")?.try_into().map_err(|_| "root")?;
    let witness = StateWitness::decode(&encoded)?;
    let value = &witness.value;
    let debit = DebitExpectation {
        activity_id: value[6..38].try_into()?,
        network_id: u32::from_be_bytes(value[2..6].try_into()?),
        withdrawal_id: value[6..38].try_into()?,
        account: value[38..70].try_into()?,
        withdrawals_account: bytes(&document, "withdrawals_account")?
            .try_into()
            .map_err(|_| "withdrawals account")?,
        asset_id: value[70..102].try_into()?,
        amount: u128::from_be_bytes(value[102..118].try_into()?),
        recipient: EvmAddress::new(value[130..150].try_into()?),
    };
    let proof = CheckpointProof {
        native: Some(NativeEvidence {
            request_anchor: value[150..182].try_into()?,
            inclusion_checkpoint: [4; 32],
            network_id: debit.network_id,
            witness: encoded,
            recipient_signature: Vec::new(),
        }),
        checkpoint_hash: [4; 32],
        state_root: root,
        epoch: 1,
        batch_number: 1,
        data_availability_root: [5; 32],
        leaf_index: 0,
        siblings: Vec::new(),
        attestations: Vec::new(),
    };
    proof
        .verify_native_withdrawal(&debit)
        .map_err(|e| format!("{e:?}"))?;
    for field in 0..6 {
        let mut altered = debit;
        match field {
            0 => altered.withdrawal_id[0] ^= 1,
            1 => altered.account[0] ^= 1,
            2 => altered.asset_id[0] ^= 1,
            3 => altered.amount += 1,
            4 => altered.network_id += 1,
            _ => altered.recipient = EvmAddress::new([8; 20]),
        }
        assert!(proof.verify_native_withdrawal(&altered).is_err());
    }
    let mut altered = proof.clone();
    altered.native.as_mut().ok_or("native")?.request_anchor[0] ^= 1;
    assert!(altered.verify_native_withdrawal(&debit).is_err());
    altered = proof.clone();
    altered
        .native
        .as_mut()
        .ok_or("native")?
        .inclusion_checkpoint[0] ^= 1;
    assert!(altered.verify_native_withdrawal(&debit).is_err());
    altered = proof.clone();
    altered.state_root[0] ^= 1;
    assert!(altered.verify_native_withdrawal(&debit).is_err());
    altered = proof.clone();
    altered.native.as_mut().ok_or("native")?.recipient_signature = vec![1; 64];
    assert!(altered.verify_native_withdrawal(&debit).is_err());
    Ok(())
}
