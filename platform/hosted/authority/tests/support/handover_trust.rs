use super::Trust;
use layerx_platform_authority::hex;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};

#[test]
fn protected_native_genesis_and_finality_policy_refuse_substitution() -> Result<(), String> {
    let artifact = include_bytes!("../fixtures/real-handover-genesis.lxt");
    let pins = layerx_wire::handover::decode_genesis_trust(artifact)
        .map_err(|error| format!("{error:?}"))?;
    let id = layerx_wire::handover::sequencer_id(&pins.initial_sequencer_key)
        .map_err(|error| format!("{error:?}"))?;
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce).map_err(|error| format!("{error:?}"))?;
    let directory = std::env::temp_dir().join(format!(
        "authority-trust-{}-{}",
        std::process::id(),
        hex::encode(&nonce)
    ));
    fs::create_dir(&directory).map_err(|error| error.to_string())?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let directory = fs::canonicalize(directory).map_err(|error| error.to_string())?;
    let genesis = directory.join("genesis.lxt");
    let finality = directory.join("finality.conf");
    let policy = format!("version=1\nurl=http://127.0.0.1:18546\ntransport=local-emulator\ntrust_anchor_der=\nchain_id=125\nrequest_timeout_ms=8000\nregistry={}\nguarantor_bond={}\nprotocol_version=3\nnetwork_id={}\ncanonical_genesis_root={}\nconfirmations=1\n", "12".repeat(20), "34".repeat(20), pins.network_id, hex::encode(&pins.canonical_state_root));
    for (path, bytes) in [
        (&genesis, artifact.as_slice()),
        (&finality, policy.as_bytes()),
    ] {
        fs::write(path, bytes).map_err(|error| error.to_string())?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    let load = || {
        Trust::from_paths(
            &genesis,
            &finality,
            pins.network_id,
            id,
            pins.initial_sequencer_key,
        )
    };
    let trust = load().map_err(|()| "native trust refused")?;
    let history = trust.history.lock().map_err(|_| "history lock")?;
    assert_eq!(history.network_id(), pins.network_id);
    assert!(history.verified_head().is_none());
    assert!(history.authorization_for_batch(1).is_err());
    assert!(history.authorization_for_sequence(1).is_err());
    drop(history);
    assert!(Trust::from_paths(
        &genesis,
        &finality,
        pins.network_id + 1,
        id,
        pins.initial_sequencer_key
    )
    .is_err());
    assert!(Trust::from_paths(
        &genesis,
        &finality,
        pins.network_id,
        [0; 32],
        pins.initial_sequencer_key
    )
    .is_err());
    assert!(Trust::from_paths(&genesis, &finality, pins.network_id, id, [0; 32]).is_err());
    for ending in [0, 1, 29, artifact.len() - 1] {
        fs::write(&genesis, &artifact[..ending]).map_err(|error| error.to_string())?;
        assert!(load().is_err());
    }
    let mut trailing = artifact.to_vec();
    trailing.push(0);
    fs::write(&genesis, trailing).map_err(|error| error.to_string())?;
    assert!(load().is_err());
    fs::write(&genesis, artifact).map_err(|error| error.to_string())?;
    for changed in [
        policy.replace(&hex::encode(&pins.canonical_state_root), &"00".repeat(32)),
        policy.replace("protocol_version=3", "protocol_version=2"),
        format!("{policy}network_id={}\n", pins.network_id),
    ] {
        fs::write(&finality, changed).map_err(|error| error.to_string())?;
        assert!(load().is_err());
    }
    fs::write(&finality, policy).map_err(|error| error.to_string())?;
    for path in [&genesis, &finality] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o644))
            .map_err(|error| error.to_string())?;
        assert!(load().is_err());
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    let link = directory.join("genesis-link.lxt");
    symlink(&genesis, &link).map_err(|error| error.to_string())?;
    assert!(Trust::from_paths(
        &link,
        &finality,
        pins.network_id,
        id,
        pins.initial_sequencer_key
    )
    .is_err());
    assert!(load().is_ok());
    Ok(())
}
