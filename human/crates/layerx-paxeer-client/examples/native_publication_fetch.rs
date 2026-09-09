use std::time::Duration;

use ed25519_dalek::{Signature, VerifyingKey};
use layerx_paxeer_client::{
    deposit_root_registration_message, CheckpointProof, CustodyDeposit, DebitExpectation,
    EndpointConfig, EndpointTransport, ExitEvidence, PublishedDepositProof,
};
use layerx_types::{amount::Amount, ids::AssetId, intent::EvmAddress};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn bytes<const N: usize>(value: &str) -> Result<[u8; N]> {
    let value = value.strip_prefix("0x").ok_or("hex prefix")?;
    if value.len() != N * 2 {
        return Err("hex length".into());
    }
    let mut result = [0; N];
    for (i, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16)?;
    }
    Ok(result)
}

fn checked<T, E: std::fmt::Debug>(result: std::result::Result<T, E>) -> Result<T> {
    result.map_err(|error| format!("{error:?}").into())
}

fn endpoint(url: &str) -> EndpointConfig {
    EndpointConfig {
        url: url.to_owned(),
        request_timeout: Duration::from_secs(30),
        transport: EndpointTransport::LocalEmulator,
        expected_chain_id: 31_337,
    }
}

fn fetch_deposit_balance(args: &[String]) -> Result<[u8; 32]> {
    if args.len() != 13 {
        return Err("expected 13 deposit and balance inputs".into());
    }
    let endpoint = endpoint(&args[0]);
    let registry = EvmAddress::new(bytes(&args[1])?);
    let vault = EvmAddress::new(bytes(&args[2])?);
    let checkpoint = bytes(&args[3])?;
    let account = bytes(&args[4])?;
    let asset = bytes(&args[5])?;
    let recipient = EvmAddress::new(bytes(&args[6])?);
    let custody = CustodyDeposit {
        deposit_id: bytes(&args[7])?,
        asset: AssetId::new(asset),
        payer: EvmAddress::new(bytes(&args[8])?),
        beneficiary: account,
        amount: Amount::from_u128(args[10].parse()?),
        nonce: args[9].parse()?,
    };
    let deposit = checked(PublishedDepositProof::fetch_published(
        &endpoint, vault, registry, checkpoint, custody, 1,
    ))?;
    assert_eq!(deposit.registration.checkpoint_id, checkpoint);
    let authority = VerifyingKey::from_bytes(&bytes(&args[12])?)?;
    authority.verify_strict(
        &checked(deposit_root_registration_message(&deposit.registration))?,
        &Signature::from_bytes(&deposit.registration.signature),
    )?;
    let root = deposit.registration.checkpoint_state_root;
    let exit = checked(ExitEvidence::fetch_published(
        &endpoint,
        registry,
        checkpoint,
        (account, asset, recipient),
        3,
        1,
    ))?;
    assert_eq!(exit.finalised_balance, args[11].parse::<u128>()?);
    let native = exit.native.as_ref().ok_or("native exit absent")?;
    let witness = checked(native.decoded(root))?;
    let signature: [u8; 64] = native.recipient_signature.as_slice().try_into()?;
    checked(exit.verify_native_balance(&witness, root, 77, native.request_anchor, &signature))?;
    assert!(ExitEvidence::fetch_published(
        &endpoint,
        registry,
        checkpoint,
        (account, asset, EvmAddress::new([0x32; 20])),
        3,
        1,
    )
    .is_err());
    println!("native v2 deposit and signed balance fetched and verified; wrong recipient refused");
    Ok(root)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|v| v == "--deposit-balance-only") {
        fetch_deposit_balance(&args[1..])?;
        return Ok(());
    }
    if args.len() != 17 {
        return Err("expected 17 public fixture inputs".into());
    }
    let common = [0, 1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 14, 16].map(|i| args[i].clone());
    let root = fetch_deposit_balance(&common)?;
    let endpoint = endpoint(&args[0]);
    let registry = EvmAddress::new(bytes(&args[1])?);
    let checkpoint = bytes(&args[3])?;
    let withdrawal = bytes(&args[7])?;
    let debit = checked(
        DebitExpectation {
            activity_id: withdrawal,
            network_id: 77,
            withdrawal_id: withdrawal,
            account: bytes(&args[4])?,
            withdrawals_account: bytes(&args[8])?,
            asset_id: bytes(&args[5])?,
            amount: args[13].parse()?,
            recipient: EvmAddress::new(bytes(&args[6])?),
        }
        .validated(),
    )?;
    let proof = checked(CheckpointProof::fetch_published(
        &endpoint, registry, checkpoint, &debit, 3, 1,
    ))?;
    assert_eq!(proof.state_root, root);
    checked(proof.verify_native_withdrawal(&debit))?;
    let native = proof.native.as_ref().ok_or("native withdrawal absent")?;
    assert_eq!(native.request_anchor, bytes::<32>(&args[15])?);
    assert_eq!(native.inclusion_checkpoint, checkpoint);
    assert_ne!(native.request_anchor, native.inclusion_checkpoint);
    assert!(
        CheckpointProof::fetch_published(&endpoint, registry, bytes(&args[15])?, &debit, 3, 1,)
            .is_err()
    );
    println!("native v2 withdrawal fetched and verified; missing withdrawal refused");
    Ok(())
}
