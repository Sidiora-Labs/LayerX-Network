use std::{env, fs, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf, sync::Arc, time::Duration};
use layerx_human_service::{journeys::{DepositAdmission, DepositRuntime, MovementExecutionIdentity,
    WalletCustodyRequest}, server::movement_provider::{MovementProviderConfig, NativeMovementCodec,
    UnixMovementProvider}, store::{AgentTenantId, PrincipalId}};
use layerx_paxeer_client::{EndpointConfig, EndpointTransport, TransactionHash, WithdrawalBoundary,
    WithdrawalConfig};
use layerx_types::{account::AccountId, amount::Amount, ids::AssetId, intent::EvmAddress};
use serde::Deserialize;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    recipient: String,
    transaction: String,
    wallet: String,
    asset: String,
    amount: String,
    principal: String,
    tenant: String,
    action_key: String,
    plan_id: String,
}

fn checked<T, E: std::fmt::Debug>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|error| format!("{error:?}").into())
}
fn setting(name: &str) -> Result<String> {
    Ok(env::var(format!("LAYERX_HUMAN_MOVEMENT_PROVIDER_{name}"))?)
}
fn bytes<const N: usize>(text: &str) -> Result<[u8; N]> {
    let text = text.strip_prefix("0x").ok_or("missing hex prefix")?;
    if text.len() != 2 * N || !text.is_ascii() { return Err("hex length".into()); }
    let mut result = [0; N];
    for (i, value) in result.iter_mut().enumerate() {
        *value = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)?;
    }
    Ok(result)
}
fn client() -> Result<UnixMovementProvider> {
    let ca = fs::read(setting("PAXEER_CA_DER")?)?;
    let urls: Vec<String> = serde_json::from_str(&setting("PAXEER_RPC_URLS")?)?;
    let chain = setting("PAXEER_CHAIN_ID")?.parse()?;
    let deadline = Duration::from_secs(setting("DEADLINE_SECONDS")?.parse()?);
    let endpoints = urls.into_iter().map(|url| EndpointConfig { url, request_timeout: deadline,
        transport: EndpointTransport::PinnedTls { trust_anchor_der: ca.clone() },
        expected_chain_id: chain }).collect();
    checked(UnixMovementProvider::new(MovementProviderConfig {
        socket: PathBuf::from(setting("SOCKET")?), peer_uid: setting("ALLOWED_UID")?.parse()?,
        peer_gid: setting("ALLOWED_GID")?.parse()?, maximum_frame_bytes: 1_048_576,
        deadline: Duration::from_secs(setting("DEADLINE_SECONDS")?.parse()?),
    }, Arc::new(checked(NativeMovementCodec::for_protocol(3))?),
    checked(WithdrawalBoundary::new_for_protocol(WithdrawalConfig {
        endpoints, minimum_endpoint_agreement: setting("PAXEER_MINIMUM_AGREEMENT")?.parse()?,
        claims_contract: EvmAddress::new(bytes(&setting("PAXEER_CLAIMS_CONTRACT")?)?),
        required_confirmations: setting("PAXEER_CONFIRMATIONS")?.parse()?,
        poll_cadence: Duration::from_secs(1), delayed_after_polls: 2,
    }, 3))?))
}
fn request(input: &Input, recipient: &AccountId) -> Result<WalletCustodyRequest> {
    let beneficiary = checked(layerx_paxeer_client::account_address_for_protocol(recipient, 3))?;
    let wallet = EvmAddress::new(bytes(&input.wallet)?);
    Ok(WalletCustodyRequest { identity: MovementExecutionIdentity {
        principal: checked(PrincipalId::new(&input.principal))?,
        tenant: checked(AgentTenantId::new(&input.tenant))?,
        account: beneficiary, wallet, plan_id: bytes(&input.plan_id)?,
    }, action_key: bytes(&input.action_key)?, wallet,
        chain_id: setting("PAXEER_CHAIN_ID")?.parse()?,
        vault: EvmAddress::new(bytes(&setting("PAXEER_VAULT")?)?),
        asset: AssetId::new(bytes(&input.asset)?), beneficiary,
        amount: Amount::from_u128(input.amount.parse()?),
    })
}
fn run() -> Result {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 { return Err("expected request and output paths".into()); }
    let input: Input = serde_json::from_slice(&fs::read(&args[0])?)?;
    let recipient = checked(AccountId::parse(&input.recipient))?;
    let transaction = checked(TransactionHash::from_hex(&input.transaction))?;
    let request = request(&input, &recipient)?;
    let mut client = client()?;
    let DepositAdmission::Native(admission) = checked(client.admit_credit(&request, transaction, &recipient, 3))?
    else { return Err("expected native admission".into()); };
    let reserve = checked(AccountId::parse("system:paxeer-reserve"))?;
    checked(admission.credit_intent(&reserve, &recipient))?;
    assert_eq!(admission.transaction(), transaction);
    assert_eq!(admission.custody().amount, request.amount);
    assert_eq!(admission.custody().beneficiary, request.beneficiary);
    let encoded = checked(layerx_paxeer_client::wire::encode_native_deposit_admission(&admission, 786))?;
    assert_eq!(checked(layerx_paxeer_client::wire::decode_native_deposit_admission(&encoded, 786))?, *admission);
    for offset in encoded.len() - 427..encoded.len() {
        let mut corrupt = encoded.clone(); corrupt[offset] ^= 1;
        assert!(layerx_paxeer_client::wire::decode_native_deposit_admission(&corrupt, 786).is_err());
    }
    for end in 0..encoded.len() {
        assert!(layerx_paxeer_client::wire::decode_native_deposit_admission(&encoded[..end], 786).is_err());
    }
    let mut wrong = request.clone();
    wrong.amount = Amount::from_u128(request.amount.value().checked_add(1).ok_or("amount overflow")?);
    assert!(client.admit_credit(&wrong, transaction, &recipient, 3).is_err());
    let foreign = checked(AccountId::parse("agent:did:layerx:unrelated-recipient:main"))?;
    assert!(client.admit_credit(&request, transaction, &foreign, 3).is_err());
    let mut file = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&args[1])?;
    file.write_all(admission.native_credit().canonical_bytes())?;
    file.sync_all()?;
    Ok(())
}
fn main() {
    if let Err(error) = run() { eprintln!("native custody admission check failed: {error}"); std::process::exit(1); }
}
