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
