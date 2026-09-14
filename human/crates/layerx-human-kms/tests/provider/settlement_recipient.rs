use super::{checked, Host, Result};
use layerx_crypto::settlement_recipient::RecipientAuthorization;
use layerx_human_service::custody::{KeyClass, KeyId, Keystore, KmsProvider};
use layerx_human_service::store::PrincipalId;
use layerx_types::ids::Did;

#[test]
fn recipient_binding_uses_real_custody_and_survives_restart() -> Result<()> {
    let mut host = Host::new()?;
    let principal = PrincipalId::new("recipient-alice")?;
    let key = KeyId::new("human-primary")?;
    let store = Keystore::open_production(
        host.root.join("recipient-client-state"),
        77,
        host.remote("client", "beta-kms")?,
    )?;
    let public = store.create(&principal, &key, KeyClass::HumanPrimary)?;
    let binding = store.evm_binding(&principal, &key)?;
    let reference = store.evm_provider_reference(&principal, &key)?;
    let authorization = RecipientAuthorization {
        network_id: 77,
        binding_digest: binding.digest(),
        did: checked(Did::new(b"did:layerx:recipient-alice"))?,
        public_key: public,
        asset: [3; 32],
        checkpoint: [4; 32],
        recipient: store.evm_wallet(&principal, &key)?,
    };
    let payload = checked(authorization.encode())?;
    assert_eq!(
        checked(RecipientAuthorization::decode(&payload))?,
        authorization
    );
    let sign = |value: &RecipientAuthorization| -> Result<[u8; 64]> {
        checked(host.remote("client", "beta-kms")?.evm_operation(
            13,
            &binding,
            &reference,
            &checked(value.encode())?,
        ))?
        .try_into()
        .map_err(|_| "recipient signature length".into())
    };
    let signature = sign(&authorization)?;
    checked(authorization.verify_signature(&signature))?;
    let changes: [fn(&mut RecipientAuthorization); 4] = [
        |value| value.network_id += 1,
        |value| value.binding_digest[0] ^= 1,
        |value| value.public_key[0] ^= 1,
        |value| value.recipient[0] ^= 1,
    ];
    for change in changes {
        let mut changed = authorization.clone();
        change(&mut changed);
        assert!(sign(&changed).is_err());
    }
    for length in 0..payload.len() {
        assert!(RecipientAuthorization::decode(&payload[..length]).is_err());
    }
    let mut trailing = payload.clone();
    trailing.push(0);
    assert!(RecipientAuthorization::decode(&trailing).is_err());
    assert!(host
        .remote("foreign", "beta-kms")?
        .evm_operation(13, &binding, &reference, &payload)
        .is_err());
    let mut changed = authorization.clone();
    changed.checkpoint[0] ^= 1;
    assert!(changed.verify_signature(&signature).is_err());
    let next = sign(&changed)?;
    checked(changed.verify_signature(&next))?;
    host.stop();
    host.start()?;
    let repeated: [u8; 64] = checked(
        host.remote("client", "beta-kms")?
            .evm_operation(13, &binding, &reference, &payload),
    )?
    .try_into()
    .map_err(|_| "recipient signature length")?;
    assert_eq!(repeated, signature);
    changed = authorization.clone();
    changed.did = checked(Did::new(b"did:layerx:recipient-bob"))?;
    assert!(host
        .remote("client", "beta-kms")?
        .evm_operation(13, &binding, &reference, &checked(changed.encode())?,)
        .is_err());
    store.destroy(&principal, &key)?;
    assert!(host
        .remote("client", "beta-kms")?
        .evm_operation(13, &binding, &reference, &payload)
        .is_err());
    Ok(())
}
