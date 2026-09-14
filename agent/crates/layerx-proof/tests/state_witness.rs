use layerx_proof::state_witness::{StateProofError, StateWitness};
use layerx_types::json::parse;

#[test]
fn exact_native_vectors_and_negative_paths() -> Result<(), Box<dyn std::error::Error>> {
    let document = parse(include_str!(
        "../../../../contracts/config/native-state-proofs.json"
    ))?;
    let vectors = document.array_at("vectors")?;
    assert_eq!(vectors.len(), 10);
    for vector in vectors {
        let encoded = vector.hex_at("proof")?;
        let root = vector.hex_array_at("root")?;
        let witness = StateWitness::decode(&encoded)?;
        witness.verify(root)?;
        assert_eq!(witness.encode()?, encoded);
        for length in 0..encoded.len() {
            assert!(StateWitness::decode(&encoded[..length]).is_err());
        }
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(StateWitness::decode(&trailing).is_err());
        let mut bad = witness.clone();
        bad.leaf_index_a = bad.leaf_count_a;
        assert_eq!(bad.root(), Err(StateProofError::Path));
        bad = witness.clone();
        bad.siblings_a.push([0; 32]);
        assert_eq!(bad.root(), Err(StateProofError::Path));
        bad = witness.clone();
        bad.key[0] ^= 1;
        assert!(bad.verify(root).is_err());
        bad = witness.clone();
        bad.value[0] ^= 1;
        assert!(bad.verify(root).is_err());
        bad = witness.clone();
        bad.siblings_b[0][0] ^= 1;
        assert!(bad.verify(root).is_err());
        bad = witness.clone();
        bad.module_id = 10;
        assert_eq!(bad.root(), Err(StateProofError::Module));
        if witness.leaf_count_a == 3 && witness.leaf_index_a == 2 {
            bad = witness.clone();
            bad.siblings_a[0][0] ^= 1;
            assert_eq!(bad.root(), Err(StateProofError::Path));
        }
        let mut other_root = root;
        other_root[0] ^= 1;
        assert_eq!(witness.verify(other_root), Err(StateProofError::Root));
    }
    Ok(())
}
