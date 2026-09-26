//! Burns to Solana released through the custody program, driven by recorded
//! Paxeer and Solana JSON-RPC exchanges (`tests/fixtures/solana_outbound.json`),
//! a real secp256k1 attestor socket, a real ed25519 fee payer socket and a
//! real journal on disk. The recording names the release transactions by
//! placeholder; each test fills them with the bytes the pinned vector must
//! produce before replaying it.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey, Verifier as _};
use k256::sha2::{Digest as _, Sha256};
use layerx_bridge_relayer::attestation::{
    to_attestor_signature, uint256_from_u64, OutboundAttestation,
};
use layerx_bridge_relayer::hex;
use layerx_bridge_relayer::journal::{
    outbound_key, Completion, Entry, Journal, Recipient, SubmissionStatus,
};
use layerx_bridge_relayer::relayer::{
    ChainLink, ChainSettings, GasPolicy, PaxeerLink, PaxeerSettings, Relayer, RelayerAssembly,
    RelayerError, RelayerParts, RelayerSetup, SolanaLink, SolanaRelease, StepReport,
    OUTBOUND_STREAM,
};
use layerx_bridge_relayer::signer::{
    Attestor, FeePayer, KeyError, Submitter, ATTEST_OUTBOUND_DOMAIN, ETHEREUM_TRANSACTION_DOMAIN,
    FEE_PAYER_ALGORITHM, PAXEER_TRANSACTION_DOMAIN, SOLANA_TRANSACTION_DOMAIN,
};
use layerx_bridge_relayer::solana::observe::SolanaSettings;
use layerx_bridge_relayer::solana::release::{
    asset_address, associated_token_address, build_release_transaction, config_address,
    nullifier_address, recipient_address, vault_authority, wire_transaction, Release,
    ReleaseAccounts, INSTRUCTIONS_SYSVAR, OP_RELEASE, SECP256K1_PROGRAM, SYSTEM_PROGRAM,
    TOKEN_PROGRAM,
};
use layerx_bridge_relayer::solana::rpc::{Commitment, SolanaRpc};
use layerx_bridge_relayer::solana::{
    base58_encode, base58_fixed, base64_encode, handle, SOLANA_CHAIN_ID,
};
use layerx_mirror::signer::{RemoteChainSigner, RemoteSignerConfig, SignerEndpoint};
use support::{key, work_directory, Recording, SignerServer};

const FIXTURE: &str = "solana_outbound.json";
const ATTESTOR: u8 = 0xa1;
const PAXEER_FEES: u8 = 0xb1;
const ETHEREUM_FEES: u8 = 0xb2;
const FEE_PAYER_SECRET: [u8; 32] = [0xf1; 32];
const FEE_PAYER_HANDLE: &str = "solana-fees";

const PROGRAM_ID: &str = "A7SZbByPYuHpunZ9pyMDMrhMYvK44ANT1AVqb8U1FpM9";
const SIDIORA_MINT: &str = "5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump";
const RECIPIENT_KEY: &str = "59TLtNdRpCZysEHkGDMPFHkHiHXkBqQVqAVxAupzNoQb";
const BLOCKHASH_FIRST: &str = "FrLFSoxDaT2FroN4k7ZE7uettrVRLzn3DQN8p6hBcjCT";
const BLOCKHASH_SECOND: &str = "79cu2QNvp92chEmBhBgcUfERDT9KHnUcr8eiJb5WzKML";

/// The addresses the custody program derives for the pinned vector.
const VAULT_AUTHORITY: &str = "GxxA9Cs9v5pAGVsaCe2jjDrtmieeBijcY4S5HHTY8Vq6";
const CONFIG: &str = "6REcAK7m9b1v6qZV5wV3aFPawCStHvrTGYNGbGqXF4Q7";
const ASSET: &str = "5XvCgGUysuU85ut4iTLDjiShTJWLzhqwJ3azMjPrknQ1";
const RECIPIENT_PDA: &str = "DukbFHcKu14W4ntjxws5sAG4NWgHWgTT7VuJrkGNif7r";
const NULLIFIER: &str = "FSACiQWvDWbuuarWkWmWik9diuERNadDv6EpasgWYrUk";
const VAULT_TOKEN: &str = "J8Rf9GPpeneA3AcgVTSvCEDCKyztYqrzD6S5NqWHU2nw";
const RECIPIENT_TOKEN: &str = "5Yc2Ahn1FPMtG4fhXzPi9LbfhJADa3YRZkibiGmS1kNt";

/// The pinned outbound vector of `bridge/vectors` and
/// `bridge/ATTESTATION-SOLANA.md`.
const VECTOR_TX_HASH: &str = "0x6f79d9a61a77030bbeba5f435907f53b09321779310326b9faaebb391b3b5d5f";
const VECTOR_NONCE: u64 = 11;
const VECTOR_AMOUNT: u64 = 4_200_000;
const VECTOR_RECIPIENT: &str = "0xfb02125a3275d53a9f6538626b49894d2aae80cc";
const VECTOR_DIGEST: &str = "0xc583652dd9b59e0fcef102cfc8866a52beadb77b82de25445d86900baf6d1c4e";
const VECTOR_NULLIFIER: &str = "0xd653f4968eb9b70e1eaef15fb134f2f7da8c15c38335af0069c94585e521ea3a";
const VAULT_HANDLE: &str = "0x334121a65b47bd45c3f6381537d9180e98e445bc";
const SIDIORA_ASSET: &str = "0x21f7b20a555199fa73a238b1a91fd0f549068fee";
const ATTESTOR_ADDRESS: &str = "0xd2431ca38735c2fd438e2caa23f094191d89675b";

const PAXEER: PaxeerSettings = PaxeerSettings {
    chain_id: 229,
    finality_depth: 2,
    start_block: 25,
    max_block_range: 10,
    gas: GasPolicy {
        gas_limit: 1_000_000,
        max_fee_per_gas: 100_000_000_000,
        max_priority_fee_per_gas: 2_000_000_000,
    },
};

const CHAIN: ChainSettings = ChainSettings {
    chain_id: 1,
    vault: [0x11; 20],
    finality_depth: 12,
    start_block: 95,
    max_block_range: 10,
    gas: GasPolicy {
        gas_limit: 400_000,
        max_fee_per_gas: 200_000_000_000,
        max_priority_fee_per_gas: 3_000_000_000,
    },
};

fn fixed<const N: usize>(text: &str) -> [u8; N] {
    hex::fixed::<N>(text).unwrap_or_else(|error| panic!("{text}: {error}"))
}

fn base58<const N: usize>(text: &str) -> [u8; N] {
    base58_fixed::<N>(text).unwrap_or_else(|error| panic!("{text}: {error}"))
}

fn settings() -> SolanaSettings {
    SolanaSettings {
        chain_id: SOLANA_CHAIN_ID,
        vault: fixed(VAULT_HANDLE),
        program_id: base58(PROGRAM_ID),
        finality_depth: 32,
        start_slot: 1000,
        max_slot_range: 500,
        commitment: Commitment::Finalized,
    }
}

fn vector() -> OutboundAttestation {
    OutboundAttestation {
        chain_id: SOLANA_CHAIN_ID,
        vault: fixed(VAULT_HANDLE),
        paxeer_tx_hash: fixed(VECTOR_TX_HASH),
        paxeer_nonce: VECTOR_NONCE,
        recipient: fixed(VECTOR_RECIPIENT),
        asset: fixed(SIDIORA_ASSET),
        amount: uint256_from_u64(VECTOR_AMOUNT),
    }
}

fn vector_key() -> String {
    outbound_key(SOLANA_CHAIN_ID, &fixed(VECTOR_TX_HASH), VECTOR_NONCE)
}

fn attest(digest: &[u8; 32]) -> [u8; 65] {
    let (signature, recovery) = key(ATTESTOR)
        .sign_prehash_recoverable(digest)
        .unwrap_or_else(|error| panic!("attest: {error}"));
    let mut recoverable = [0_u8; 65];
    recoverable[..64].copy_from_slice(&signature.to_bytes());
    recoverable[64] = recovery.to_byte();
    to_attestor_signature(recoverable).unwrap_or_else(|error| panic!("attest: {error}"))
}

fn fee_payer_key() -> SigningKey {
    SigningKey::from_bytes(&FEE_PAYER_SECRET)
}

fn vector_accounts() -> ReleaseAccounts {
    ReleaseAccounts {
        program_id: base58(PROGRAM_ID),
        fee_payer: fee_payer_key().verifying_key().to_bytes(),
        config: base58(CONFIG),
        asset: base58(ASSET),
        mint: base58(SIDIORA_MINT),
        vault_authority: base58(VAULT_AUTHORITY),
        vault_token: base58(VAULT_TOKEN),
        recipient_token: base58(RECIPIENT_TOKEN),
        nullifier: base58(NULLIFIER),
    }
}

fn vector_release() -> Release {
    Release {
        attestation: vector(),
        recipient: base58(RECIPIENT_KEY),
        signatures: vec![(fixed(ATTESTOR_ADDRESS), attest(&fixed(VECTOR_DIGEST)))],
        accounts: vector_accounts(),
    }
}

/// The signed wire bytes and fee payer signature of the vector's release
/// against `blockhash`.
fn signed_release(blockhash: &str) -> (Vec<u8>, [u8; 64]) {
    let message = build_release_transaction(&vector_release(), &base58(blockhash))
        .unwrap_or_else(|error| panic!("release: {error}"));
    let signature = fee_payer_key().sign(&message).to_bytes();
    (wire_transaction(&signature, &message), signature)
}

/// Loads the recording with its release placeholders filled in.
fn recording(name: &str) -> Recording {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(FIXTURE);
    let mut text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture {}: {error}", path.display()));
    for (which, blockhash) in [("first", BLOCKHASH_FIRST), ("second", BLOCKHASH_SECOND)] {
        let (raw, signature) = signed_release(blockhash);
        text = text
            .replace(
                &format!("$release:{which}:transaction"),
                &base64_encode(&raw),
            )
            .replace(
                &format!("$release:{which}:signature"),
                &base58_encode(&signature),
            );
    }
    let filled = work_directory(&format!("recording-{name}")).join(FIXTURE);
    fs::write(&filled, text).unwrap_or_else(|error| panic!("{}: {error}", filled.display()));
    Recording::load(
        filled
            .to_str()
            .unwrap_or_else(|| panic!("{} is not text", filled.display())),
    )
}

/// One request the fee-payer signer served: (handle, domain, message).
type FeePayerRequest = (String, Vec<u8>, Vec<u8>);

/// An ed25519 signer daemon for tests: each handle owns one key and may sign
/// messages under its listed policy domains only; anything else is refused.
struct FeePayerServer {
    socket: PathBuf,
    requests: Arc<Mutex<Vec<FeePayerRequest>>>,
}

fn read_frame(stream: &mut UnixStream) -> Option<Vec<u8>> {
    let mut length = [0_u8; 4];
    stream.read_exact(&mut length).ok()?;
    let length = usize::try_from(u32::from_be_bytes(length)).ok()?;
    if length > 8192 {
        return None;
    }
    let mut frame = vec![0_u8; length];
    stream.read_exact(&mut frame).ok()?;
    Some(frame)
}

fn take(cursor: &mut &[u8], count: usize) -> Option<Vec<u8>> {
    let (head, tail) = cursor.split_at_checked(count)?;
    *cursor = tail;
    Some(head.to_vec())
}

fn take_sized(cursor: &mut &[u8], width: usize) -> Option<Vec<u8>> {
    let length = take(cursor, width)?
        .iter()
        .fold(0_usize, |length, byte| (length << 8) | usize::from(*byte));
    take(cursor, length)
}

/// (handle, domain, message) of an ed25519 request whose digest is the
/// sha256 of its message.
fn parse_request(frame: &[u8]) -> Option<FeePayerRequest> {
    let mut cursor = frame.strip_prefix(b"LXCS")?;
    if take(&mut cursor, 3)? != [0, 1, 2] {
        return None;
    }
    let handle = String::from_utf8(take_sized(&mut cursor, 2)?).ok()?;
    let domain = take_sized(&mut cursor, 2)?;
    let digest = take(&mut cursor, 32)?;
    let message = take_sized(&mut cursor, 4)?;
    if !cursor.is_empty() || digest[..] != Sha256::digest(&message)[..] {
        return None;
    }
    Some((handle, domain, message))
}

impl FeePayerServer {
    fn start(name: &str, keys: Vec<(&str, SigningKey, Vec<&[u8]>)>) -> Self {
        let socket = work_directory(&format!("fee-payer-{name}")).join("s.sock");
        let listener = UnixListener::bind(&socket)
            .unwrap_or_else(|error| panic!("fee payer socket {}: {error}", socket.display()));
        let keys: BTreeMap<String, (SigningKey, Vec<Vec<u8>>)> = keys
            .into_iter()
            .map(|(handle, key, domains)| {
                let domains = domains.into_iter().map(<[u8]>::to_vec).collect();
                (handle.to_owned(), (key, domains))
            })
            .collect();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let Some(frame) = read_frame(&mut stream) else {
                    continue;
                };
                let response = parse_request(&frame).map_or_else(
                    || vec![1],
                    |(handle, domain, message)| {
                        let response = match keys.get(&handle) {
                            Some((key, domains)) if domains.contains(&domain) => {
                                let mut response = vec![0_u8];
                                response.extend_from_slice(&key.sign(&message).to_bytes());
                                response
                            }
                            _ => vec![1],
                        };
                        log.lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push((handle, domain, message));
                        response
                    },
                );
                let length = u32::try_from(response.len()).unwrap_or_default();
                let _ = stream
                    .write_all(&length.to_be_bytes())
                    .and_then(|()| stream.write_all(&response));
            }
        });
        Self { socket, requests }
    }

    fn requests(&self) -> Vec<FeePayerRequest> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn remote(&self, handle: &str, key: &SigningKey) -> RemoteChainSigner {
        RemoteChainSigner::new(RemoteSignerConfig {
            endpoint: SignerEndpoint::Uds {
                socket: self.socket.clone(),
            },
            algorithm: FEE_PAYER_ALGORITHM,
            key_handle: handle.to_owned(),
            public_key: key.verifying_key().to_bytes().to_vec(),
            timeout: Duration::from_secs(5),
        })
        .unwrap_or_else(|error| panic!("fee payer signer {handle}: {error:?}"))
    }
}

struct Signers {
    attestor: SignerServer,
    fee_payer: FeePayerServer,
}

fn signers(name: &str) -> Signers {
    Signers {
        attestor: SignerServer::start(
            name,
            vec![
                ("attestor-1", key(ATTESTOR), vec![ATTEST_OUTBOUND_DOMAIN]),
                (
                    "paxeer-fees-1",
                    key(PAXEER_FEES),
                    vec![PAXEER_TRANSACTION_DOMAIN],
                ),
                (
                    "ethereum-fees",
                    key(ETHEREUM_FEES),
                    vec![ETHEREUM_TRANSACTION_DOMAIN],
                ),
            ],
        ),
        fee_payer: FeePayerServer::start(
            name,
            vec![(
                FEE_PAYER_HANDLE,
                fee_payer_key(),
                vec![SOLANA_TRANSACTION_DOMAIN],
            )],
        ),
    }
}

impl Signers {
    fn attestor_requests(&self) -> usize {
        self.attestor
            .requests()
            .iter()
            .filter(|(handle, _, _)| handle == "attestor-1")
            .count()
    }

    fn fee_payer_messages(&self) -> Vec<Vec<u8>> {
        self.fee_payer
            .requests()
            .into_iter()
            .map(|(handle, domain, message)| {
                assert_eq!(handle, FEE_PAYER_HANDLE);
                assert_eq!(domain, SOLANA_TRANSACTION_DOMAIN);
                message
            })
            .collect()
    }
}

fn start(recording: &Recording, signers: &Signers, journal: &Path) -> Relayer {
    let secp = &signers.attestor;
    let parts = RelayerParts {
        attestor: Attestor::new(secp.remote("attestor-1", &key(ATTESTOR)))
            .unwrap_or_else(|error| panic!("attestor: {error:?}")),
        paxeer: PaxeerLink {
            settings: PAXEER,
            rpc: recording.endpoint("paxeer"),
            submitter: Submitter::paxeer(secp.remote("paxeer-fees-1", &key(PAXEER_FEES)))
                .unwrap_or_else(|error| panic!("paxeer submitter: {error:?}")),
        },
        chains: vec![ChainLink {
            settings: CHAIN,
            rpc: recording.endpoint("ethereum-1"),
            submitter: Submitter::ethereum(secp.remote("ethereum-fees", &key(ETHEREUM_FEES)))
                .unwrap_or_else(|error| panic!("ethereum submitter: {error:?}")),
        }],
        journal: Journal::open(journal).unwrap_or_else(|error| panic!("journal: {error}")),
        cosign: None,
        max_submissions: 3,
    };
    let fee_payer = FeePayer::new(signers.fee_payer.remote(FEE_PAYER_HANDLE, &fee_payer_key()))
        .unwrap_or_else(|error| panic!("fee payer: {error:?}"));
    Relayer::new(RelayerSetup {
        assembly: RelayerAssembly {
            parts,
            solana: Some(SolanaLink {
                settings: settings(),
                rpc: SolanaRpc::new(recording.endpoint("solana")),
            }),
        },
        release: Some(SolanaRelease {
            fee_payer,
            mints: vec![base58(SIDIORA_MINT)],
        }),
    })
    .unwrap_or_else(|error| panic!("relayer startup: {error}"))
}

fn step(relayer: &mut Relayer) -> StepReport {
    relayer
        .outbound_step()
        .unwrap_or_else(|error| panic!("outbound step: {error}"))
}

const fn report(observed: usize, submitted: usize, completed: usize, waiting: usize) -> StepReport {
    StepReport {
        observed,
        submitted,
        completed,
        waiting,
        refused: 0,
    }
}

fn journal_lines(path: &Path) -> Vec<Entry> {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("journal {}: {error}", path.display()))
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("{line}: {error}")))
        .collect()
}

fn assert_replays(relayer: &Relayer, path: &Path) {
    let replayed = Journal::open(path).unwrap_or_else(|error| panic!("journal: {error}"));
    assert_eq!(replayed.state(), relayer.journal().state());
}

fn short(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]))
}

/// The message after its signature section, split into header, account keys,
/// blockhash and (program index, account indices, data) per instruction.
struct Message {
    header: [u8; 3],
    keys: Vec<[u8; 32]>,
    blockhash: [u8; 32],
    instructions: Vec<(usize, Vec<usize>, Vec<u8>)>,
}

fn parse_message(message: &[u8]) -> Message {
    // Every length in a release message is below 128: one shortvec byte each.
    let mut at = 3;
    let header = [message[0], message[1], message[2]];
    let count = usize::from(message[at]);
    at += 1;
    let keys = (0..count)
        .map(|index| <[u8; 32]>::try_from(&message[at + 32 * index..][..32]))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| panic!("keys: {error}"));
    at += 32 * count;
    let blockhash: [u8; 32] = message[at..at + 32]
        .try_into()
        .unwrap_or_else(|error| panic!("blockhash: {error}"));
    at += 32;
    let instructions = (0..message[at])
        .map(|_| {
            at += 1;
            let program = usize::from(message[at]);
            let accounts = usize::from(message[at + 1]);
            let indices = message[at + 2..at + 2 + accounts]
                .iter()
                .map(|index| usize::from(*index))
                .collect();
            at += 2 + accounts;
            let (length, width) = if message[at] < 0x80 {
                (usize::from(message[at]), 1)
            } else {
                (
                    usize::from(message[at] & 0x7f) | (usize::from(message[at + 1]) << 7),
                    2,
                )
            };
            let data = message[at + width..at + width + length].to_vec();
            at += width + length - 1;
            (program, indices, data)
        })
        .collect();
    assert_eq!(
        at + 1,
        message.len(),
        "the message ends after its instructions"
    );
    Message {
        header,
        keys,
        blockhash,
        instructions,
    }
}

#[test]
fn the_vector_release_derives_the_program_addresses_and_verifies_under_the_fee_payer() {
    let program = base58::<32>(PROGRAM_ID);
    let mint = base58::<32>(SIDIORA_MINT);
    let recipient = base58::<32>(RECIPIENT_KEY);
    let vault = vault_authority(&program, 255).unwrap_or_else(|| panic!("vault authority"));
    assert_eq!(vault, base58(VAULT_AUTHORITY));
    assert_eq!(handle(&vault), fixed(VAULT_HANDLE));
    assert_eq!(handle(&recipient), fixed(VECTOR_RECIPIENT));
    assert_eq!(vector().digest(), fixed(VECTOR_DIGEST));
    assert_eq!(vector().nullifier(), fixed(VECTOR_NULLIFIER));
    let expected = [
        (config_address(&program), CONFIG),
        (asset_address(&program, &mint), ASSET),
        (
            recipient_address(&program, &fixed(VECTOR_RECIPIENT)),
            RECIPIENT_PDA,
        ),
        (
            nullifier_address(&program, &fixed(VECTOR_NULLIFIER)),
            NULLIFIER,
        ),
        (associated_token_address(&vault, &mint), VAULT_TOKEN),
        (associated_token_address(&recipient, &mint), RECIPIENT_TOKEN),
    ];
    for (derived, pinned) in expected {
        assert_eq!(derived, Some(base58(pinned)), "{pinned}");
    }
    let (raw, signature) = signed_release(BLOCKHASH_FIRST);
    assert_eq!(raw[0], 1, "the fee payer is the only signer");
    assert_eq!(raw[1..65], signature);
    let message = &raw[65..];
    fee_payer_key()
        .verifying_key()
        .verify(message, &ed25519_dalek::Signature::from_bytes(&signature))
        .unwrap_or_else(|error| panic!("fee payer signature: {error}"));
    assert!(raw.len() <= 1232);
    let parsed = parse_message(message);
    assert_eq!(parsed.blockhash, base58::<32>(BLOCKHASH_FIRST));
    // The fee payer signs and pays; eight programs and read-only accounts
    // follow the four accounts the release writes.
    assert_eq!(parsed.header, [1, 0, 8]);
    assert_eq!(parsed.keys.len(), 13);
    assert_eq!(parsed.keys[0], vector_accounts().fee_payer);
    assert_eq!(parsed.instructions.len(), 2);
}

#[test]
fn the_secp256k1_instruction_directly_precedes_the_release_and_carries_the_attestor() {
    let (raw, _) = signed_release(BLOCKHASH_FIRST);
    let parsed = parse_message(&raw[65..]);
    let (secp_program, secp_accounts, secp) = &parsed.instructions[0];
    assert_eq!(parsed.keys[*secp_program], SECP256K1_PROGRAM);
    assert!(secp_accounts.is_empty());
    assert_eq!(secp[0], 1, "one attestor signature");
    let signature_at = short(secp, 1);
    let address_at = short(secp, 4);
    let message_at = short(secp, 7);
    assert_eq!(
        [secp[3], secp[6], secp[11]],
        [0, 0, 0],
        "every offset points into instruction 0 itself"
    );
    assert_eq!(short(secp, 9), 185);
    let attestation = attest(&fixed(VECTOR_DIGEST));
    assert_eq!(secp[signature_at..signature_at + 64], attestation[..64]);
    assert_eq!(secp[signature_at + 64], attestation[64] - 27);
    assert_eq!(
        secp[address_at..address_at + 20],
        fixed::<20>(ATTESTOR_ADDRESS)
    );
    assert_eq!(secp[message_at..message_at + 185], vector().preimage());

    let (program, accounts, data) = &parsed.instructions[1];
    assert_eq!(parsed.keys[*program], base58::<32>(PROGRAM_ID));
    let named = vector_accounts();
    let expected = [
        (named.fee_payer, true, true),
        (named.config, false, false),
        (named.asset, false, true),
        (named.mint, false, false),
        (named.vault_authority, false, false),
        (named.vault_token, false, true),
        (named.recipient_token, false, true),
        (named.nullifier, false, true),
        (INSTRUCTIONS_SYSVAR, false, false),
        (TOKEN_PROGRAM, false, false),
        (SYSTEM_PROGRAM, false, false),
    ];
    assert_eq!(accounts.len(), expected.len());
    let [signers, readonly_signed, readonly_unsigned] = parsed.header.map(usize::from);
    let total = parsed.keys.len();
    for (index, (account, signs, writes)) in accounts.iter().zip(expected) {
        assert_eq!(parsed.keys[*index], account);
        assert_eq!(*index < signers, signs, "signer flag of {index}");
        let writable = if *index < signers {
            *index < signers - readonly_signed
        } else {
            *index < total - readonly_unsigned
        };
        assert_eq!(writable, writes, "writable flag of {index}");
    }
    let mut release = b"PXBR".to_vec();
    release.extend_from_slice(&1_u16.to_be_bytes());
    release.push(OP_RELEASE);
    release.extend_from_slice(&fixed::<32>(VECTOR_TX_HASH));
    release.extend_from_slice(&VECTOR_NONCE.to_be_bytes());
    release.extend_from_slice(&base58::<32>(RECIPIENT_KEY));
    release.extend_from_slice(&VECTOR_AMOUNT.to_be_bytes());
    assert_eq!(*data, release);
}

#[test]
fn the_fee_payer_signs_only_under_the_solana_transaction_domain() {
    let signers = signers("fee-payer-domain");
    let payer = FeePayer::new(signers.fee_payer.remote(FEE_PAYER_HANDLE, &fee_payer_key()))
        .unwrap_or_else(|error| panic!("fee payer: {error:?}"));
    assert_eq!(
        payer.public_key(),
        fee_payer_key().verifying_key().to_bytes()
    );
    let signature = payer
        .sign_message(b"release message")
        .unwrap_or_else(|error| panic!("sign: {error}"));
    assert_eq!(
        signature,
        fee_payer_key().sign(b"release message").to_bytes()
    );

    // A daemon holding the fee payer under the attestor's domain only refuses.
    let attestor_domain = FeePayerServer::start(
        "fee-payer-attestor-domain",
        vec![(
            FEE_PAYER_HANDLE,
            fee_payer_key(),
            vec![ATTEST_OUTBOUND_DOMAIN],
        )],
    );
    let refused = FeePayer::new(attestor_domain.remote(FEE_PAYER_HANDLE, &fee_payer_key()))
        .unwrap_or_else(|error| panic!("fee payer: {error:?}"));
    assert!(matches!(
        refused.sign_message(b"release message"),
        Err(KeyError::Signer(_))
    ));
    assert_eq!(attestor_domain.requests().len(), 1);
    assert_eq!(attestor_domain.requests()[0].1, SOLANA_TRANSACTION_DOMAIN);

    // The attestor key can never be the fee payer.
    assert!(FeePayer::new(signers.attestor.remote("attestor-1", &key(ATTESTOR))).is_err());
    assert_eq!(signers.attestor_requests(), 0);
}

#[test]
fn a_release_is_journaled_before_its_broadcast_and_replays_from_disk() {
    let recording = recording("journal-first");
    let signers = signers("journal-first");
    let journal = work_directory("journal-first").join("relayer.jsonl");
    recording.set_phase("down");
    let mut relayer = start(&recording, &signers, &journal);
    assert_eq!(step(&mut relayer), report(1, 1, 0, 0));
    assert_eq!(recording.count("solana", "sendTransaction"), 1);
    assert_eq!(recording.unmatched(), Vec::<String>::new());
    let (raw, signature) = signed_release(BLOCKHASH_FIRST);
    let item = relayer
        .journal()
        .state()
        .items
        .get(&vector_key())
        .unwrap_or_else(|| panic!("the burn is journaled"))
        .clone();
    assert_eq!(item.signature, Some(attest(&fixed(VECTOR_DIGEST))));
    assert_eq!(
        item.recipient,
        Some(Recipient::Resolved(base58(RECIPIENT_KEY)))
    );
    assert_eq!(item.releases.len(), 1);
    assert_eq!(item.releases[0].raw, raw);
    assert_eq!(item.releases[0].signature, signature);
    assert_eq!(item.releases[0].last_valid_block_height, 2000);
    assert_eq!(item.releases[0].status, SubmissionStatus::Pending);
    assert!(item.submissions.is_empty());
    assert_eq!(
        relayer.journal().state().cursors.get(OUTBOUND_STREAM),
        Some(&31)
    );
    let lines = journal_lines(&journal);
    assert!(matches!(
        lines.last(),
        Some(Entry::ReleaseSubmitted { raw: journaled, .. }) if *journaled == raw
    ));
    assert_replays(&relayer, &journal);
    assert_eq!(signers.fee_payer_messages(), vec![raw[65..].to_vec()]);
}

#[test]
fn a_restarted_relayer_rebroadcasts_the_journaled_release_and_completes_it_once_final() {
    let recording = recording("restart");
    let signers = signers("restart");
    let journal = work_directory("restart").join("relayer.jsonl");
    let mut relayer = start(&recording, &signers, &journal);
    assert_eq!(step(&mut relayer), report(1, 1, 0, 0));
    drop(relayer);

    // Not yet seen and the blockhash still valid: the same bytes go out again.
    let mut relayer = start(&recording, &signers, &journal);
    assert_eq!(step(&mut relayer), report(0, 0, 0, 1));
    assert_eq!(recording.count("solana", "sendTransaction"), 2);
    assert_eq!(recording.count("paxeer", "eth_getLogs"), 1);
    drop(relayer);

    recording.set_phase("finalized");
    let mut relayer = start(&recording, &signers, &journal);
    assert_eq!(step(&mut relayer), report(0, 0, 1, 0));
    let (raw, signature) = signed_release(BLOCKHASH_FIRST);
    let item = relayer
        .journal()
        .state()
        .items
        .get(&vector_key())
        .unwrap_or_else(|| panic!("the burn is journaled"))
        .clone();
    assert_eq!(
        item.completion,
        Some(Completion::Released {
            signature,
            slot: 1850
        })
    );
    assert_eq!(item.releases.len(), 1);
    assert_eq!(item.releases[0].status, SubmissionStatus::Landed);
    assert_eq!(item.pending_release(), None);
    assert_eq!(step(&mut relayer), report(0, 0, 0, 0));
    assert_eq!(recording.count("solana", "sendTransaction"), 2);
    assert_eq!(signers.attestor_requests(), 1);
    assert_eq!(signers.fee_payer_messages(), vec![raw[65..].to_vec()]);
    assert_eq!(recording.unmatched(), Vec::<String>::new());
    assert!(
        recording.sent().is_empty(),
        "nothing is sent to an EVM chain"
    );
    assert_replays(&relayer, &journal);
}

#[test]
fn an_expired_release_is_resubmitted_with_the_journaled_attestation() {
    let recording = recording("expired");
    let signers = signers("expired");
    let journal = work_directory("expired").join("relayer.jsonl");
    let mut relayer = start(&recording, &signers, &journal);
    assert_eq!(step(&mut relayer), report(1, 1, 0, 0));
    drop(relayer);

    recording.set_phase("expired");
    let mut relayer = start(&recording, &signers, &journal);
    assert_eq!(step(&mut relayer), report(0, 1, 0, 0));
    let (first_raw, first) = signed_release(BLOCKHASH_FIRST);
    let (second_raw, second) = signed_release(BLOCKHASH_SECOND);
    let item = relayer
        .journal()
        .state()
        .items
        .get(&vector_key())
        .unwrap_or_else(|| panic!("the burn is journaled"))
        .clone();
    assert_eq!(item.releases.len(), 2);
    assert_eq!(item.releases[0].signature, first);
    assert_eq!(item.releases[0].status, SubmissionStatus::Dropped);
    assert_eq!(item.releases[1].signature, second);
    assert_eq!(item.releases[1].raw, second_raw);
    assert_eq!(item.releases[1].last_valid_block_height, 2450);
    assert_eq!(item.releases[1].status, SubmissionStatus::Pending);
    assert!(journal_lines(&journal).contains(&Entry::ReleaseExpired {
        item: vector_key(),
        signature: first,
    }));
    assert_eq!(signers.attestor_requests(), 1);
    assert_eq!(
        signers.fee_payer_messages(),
        vec![first_raw[65..].to_vec(), second_raw[65..].to_vec()]
    );
    assert_eq!(recording.count("solana", "sendTransaction"), 2);
    assert_eq!(recording.unmatched(), Vec::<String>::new());
    assert_replays(&relayer, &journal);
}

#[test]
fn a_burn_whose_nullifier_exists_completes_without_signing() {
    let recording = recording("consumed");
    let signers = signers("consumed");
    let journal = work_directory("consumed").join("relayer.jsonl");
    recording.set_phase("consumed");
    let mut relayer = start(&recording, &signers, &journal);
    assert_eq!(step(&mut relayer), report(1, 0, 1, 0));
    assert_eq!(
        relayer
            .journal()
            .state()
            .items
            .get(&vector_key())
            .and_then(|item| item.completion),
        Some(Completion::AlreadyBridged)
    );
    assert_eq!(signers.attestor_requests(), 0);
    assert!(signers.fee_payer_messages().is_empty());
    assert_eq!(recording.count("solana", "sendTransaction"), 0);
    assert_eq!(recording.count("solana", "getLatestBlockhash"), 0);
    assert_eq!(recording.unmatched(), Vec::<String>::new());
}

#[test]
fn a_burn_to_an_unregistered_recipient_waits_unsigned_until_its_pda_exists() {
    let recording = recording("unregistered");
    let signers = signers("unregistered");
    let journal = work_directory("unregistered").join("relayer.jsonl");
    recording.set_phase("unregistered");
    let mut relayer = start(&recording, &signers, &journal);
    assert_eq!(step(&mut relayer), report(1, 0, 0, 1));
    let key = vector_key();
    assert_eq!(
        relayer
            .journal()
            .state()
            .items
            .get(&key)
            .and_then(|item| item.recipient),
        Some(Recipient::Pending)
    );
    drop(relayer);
    let lines = journal_lines(&journal).len();

    // A restart keeps it pending without journaling it again.
    let mut relayer = start(&recording, &signers, &journal);
    assert_eq!(step(&mut relayer), report(0, 0, 0, 1));
    assert_eq!(journal_lines(&journal).len(), lines);
    assert_eq!(signers.attestor_requests(), 0);
    assert!(signers.fee_payer_messages().is_empty());
    assert_eq!(recording.count("solana", "sendTransaction"), 0);

    // The recipient registers: the release goes out.
    recording.set_phase("*");
    assert_eq!(step(&mut relayer), report(0, 1, 0, 0));
    let item = relayer
        .journal()
        .state()
        .items
        .get(&key)
        .unwrap_or_else(|| panic!("the burn is journaled"))
        .clone();
    assert_eq!(
        item.recipient,
        Some(Recipient::Resolved(base58(RECIPIENT_KEY)))
    );
    assert_eq!(item.releases.len(), 1);
    assert_eq!(signers.attestor_requests(), 1);
    assert_eq!(recording.count("solana", "sendTransaction"), 1);
    assert_eq!(recording.unmatched(), Vec::<String>::new());
    assert_replays(&relayer, &journal);
}

#[test]
fn releases_need_the_custody_program_and_distinct_mints() {
    let recording = recording("setup");
    let signers = signers("setup");
    let journal = work_directory("setup").join("relayer.jsonl");
    let fee_payer = || {
        FeePayer::new(signers.fee_payer.remote(FEE_PAYER_HANDLE, &fee_payer_key()))
            .unwrap_or_else(|error| panic!("fee payer: {error:?}"))
    };
    let parts = || RelayerParts {
        attestor: Attestor::new(signers.attestor.remote("attestor-1", &key(ATTESTOR)))
            .unwrap_or_else(|error| panic!("attestor: {error:?}")),
        paxeer: PaxeerLink {
            settings: PAXEER,
            rpc: recording.endpoint("paxeer"),
            submitter: Submitter::paxeer(
                signers.attestor.remote("paxeer-fees-1", &key(PAXEER_FEES)),
            )
            .unwrap_or_else(|error| panic!("paxeer submitter: {error:?}")),
        },
        chains: vec![ChainLink {
            settings: CHAIN,
            rpc: recording.endpoint("ethereum-1"),
            submitter: Submitter::ethereum(
                signers
                    .attestor
                    .remote("ethereum-fees", &key(ETHEREUM_FEES)),
            )
            .unwrap_or_else(|error| panic!("ethereum submitter: {error:?}")),
        }],
        journal: Journal::open(&journal).unwrap_or_else(|error| panic!("journal: {error}")),
        cosign: None,
        max_submissions: 3,
    };
    let mint = base58::<32>(SIDIORA_MINT);
    let refused = [
        (None, vec![mint]),
        (Some(()), vec![]),
        (Some(()), vec![mint, mint]),
        (Some(()), vec![[0; 32]]),
    ];
    for (solana, mints) in refused {
        let setup = RelayerSetup {
            assembly: RelayerAssembly {
                parts: parts(),
                solana: solana.map(|()| SolanaLink {
                    settings: settings(),
                    rpc: SolanaRpc::new(recording.endpoint("solana")),
                }),
            },
            release: Some(SolanaRelease {
                fee_payer: fee_payer(),
                mints,
            }),
        };
        assert!(matches!(
            Relayer::new(setup),
            Err(RelayerError::Configuration(_))
        ));
    }
    assert_eq!(signers.attestor_requests(), 0);
}
