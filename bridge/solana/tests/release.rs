//! The release against the real Solana runtime and the real native secp256k1
//! program.
//!
//! Every test here runs the program through `solana-program-test` at the
//! program id the pinned vectors are computed against, with Sidiora's mint at
//! its real address, real SPL token accounts and a real deposit in custody. The
//! attestors are real secp256k1 keys, their signatures are made over the real
//! outbound preimage, and the native secp256k1 program the runtime carries
//! verifies them in the instruction before the release. No account layout is
//! written by a test and no refusal is asserted through a stub: the codes these
//! tests match are the codes the program returns.

use std::collections::HashMap;

use libsecp256k1::{Message, PublicKey, SecretKey};
use paxeer_x_bridge_solana_program::attestation::{
    outbound_preimage, Outbound, HALF_ORDER, OUTBOUND_PREIMAGE_BYTES,
};
use paxeer_x_bridge_solana_program::identity::{
    find_vault_authority, hex, pubkey_handle, vault_handle, HANDLE_BYTES, SIDIORA_ASSET_ID,
    SIDIORA_MINT, SOLANA_CHAIN_ID,
};
use paxeer_x_bridge_solana_program::recipient::recipient_handle;
use paxeer_x_bridge_solana_program::release::{nullifier, release_record};
use paxeer_x_bridge_solana_program::state::{
    find_asset_address, find_config_address, find_nullifier_address, find_receipt_address,
    find_recipient_address, Asset, NullifierRecord, RecipientRecord,
};
use paxeer_x_bridge_solana_program::{
    process_instruction, BridgeError, INSTRUCTION_MAGIC, INSTRUCTION_VERSION, OP_DEPOSIT,
    OP_INITIALISE, OP_REGISTER_ASSET, OP_REGISTER_RECIPIENT, OP_RELEASE, OP_SET_ATTESTORS,
    OP_SET_CAP, OP_SET_PAUSE,
};
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_program::instruction::{AccountMeta, Instruction, InstructionError};
use solana_program::keccak;
use solana_program::program_option::COption;
use solana_program::program_pack::Pack;
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::sysvar::instructions as instructions_sysvar;
use solana_program_test::{processor, ProgramTest, ProgramTestContext};
use solana_sdk::account::{Account, AccountSharedData};
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::{Transaction, TransactionError};
use solana_sdk_ids::{bpf_loader_upgradeable, secp256k1_program, system_program};
use solana_system_interface::instruction as system_instruction;
use spl_token::state::{Account as TokenAccount, Mint};

/// The outbound preimage bridge/ATTESTATION-SOLANA.md pins: a release of 4.2
/// SID to the key `PAXEERX_BRIDGE_SOLANA_VECTOR_RECIPIENT` derives, answering
/// the burn `PAXEERX_BRIDGE_SOLANA_VECTOR_BURN` derives, at paxeerNonce 11.
const PINNED_PREIMAGE: &str = concat!(
    "504158454552585f4252494447455f4f55545f5631",
    "0000000000000000000000000000000000000000000000000000534f4c414e41",
    "334121a65b47bd45c3f6381537d9180e98e445bc",
    "6f79d9a61a77030bbeba5f435907f53b09321779310326b9faaebb391b3b5d5f",
    "000000000000000b",
    "fb02125a3275d53a9f6538626b49894d2aae80cc",
    "21f7b20a555199fa73a238b1a91fd0f549068fee",
    "0000000000000000000000000000000000000000000000000000000000401640",
);
const PINNED_DIGEST: &str = "c583652dd9b59e0fcef102cfc8866a52beadb77b82de25445d86900baf6d1c4e";
const PINNED_NULLIFIER: &str = "d653f4968eb9b70e1eaef15fb134f2f7da8c15c38335af0069c94585e521ea3a";
const VECTOR_NONCE: u64 = 11;
const VECTOR_AMOUNT: u64 = 4_200_000;
/// The Sidiora deposit the inbound vector pins, which puts the custody the
/// release pays out of in place.
const DEPOSIT_AMOUNT: u64 = 12_345_678;
const PER_TX_CAP: u64 = 20_000_000;
const TOTAL_CAP: u64 = 100_000_000;
const THRESHOLD: u8 = 2;
/// The index of the release in a transaction that places the secp256k1
/// instruction first.
const RELEASE_INDEX: u8 = 1;

/// The secp256k1 group order, big-endian.
const ORDER: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe,
    0xba, 0xae, 0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36, 0x41, 0x41,
];

fn label(text: &str) -> [u8; 32] {
    keccak::hash(text.as_bytes()).to_bytes()
}

fn unhex(text: &str) -> Vec<u8> {
    assert_eq!(text.len() % 2, 0, "{text} has an odd length");
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).expect("hexadecimal"))
        .collect()
}

fn vector_program() -> Pubkey {
    Pubkey::new_from_array(label("PAXEERX_BRIDGE_SOLANA_VECTOR_PROGRAM"))
}

fn vector_recipient() -> Pubkey {
    Pubkey::new_from_array(label("PAXEERX_BRIDGE_SOLANA_VECTOR_RECIPIENT"))
}

fn vector_burn() -> [u8; 32] {
    label("PAXEERX_BRIDGE_SOLANA_VECTOR_BURN")
}

fn payload(opcode: u8, tail: &[u8]) -> Vec<u8> {
    let mut data = INSTRUCTION_MAGIC.to_vec();
    data.extend_from_slice(&INSTRUCTION_VERSION.to_be_bytes());
    data.push(opcode);
    data.extend_from_slice(tail);
    data
}

/// A secp256k1 key and the ethereum address the native program recovers for
/// it.
struct Attestor {
    secret: SecretKey,
    address: [u8; HANDLE_BYTES],
}

impl Attestor {
    fn from_label(text: &str) -> Self {
        let secret = SecretKey::parse(&label(text)).expect("a keccak256 output is a valid key");
        let public = PublicKey::from_secret_key(&secret).serialize();
        let digest = keccak::hash(&public[1..]).to_bytes();
        let mut address = [0_u8; HANDLE_BYTES];
        address.copy_from_slice(&digest[32 - HANDLE_BYTES..]);
        Self { secret, address }
    }

    /// Sign keccak256 of `message`, as every attestor of the bridge signs.
    fn sign(&self, message: &[u8]) -> Entry {
        let digest = keccak::hash(message).to_bytes();
        let (signature, recovery) = libsecp256k1::sign(&Message::parse(&digest), &self.secret);
        Entry {
            address: self.address,
            signature: signature.serialize(),
            recovery: recovery.serialize(),
            message: message.to_vec(),
        }
    }
}

/// Keys sorted by the address they recover to.
fn keys(labels: &[&str]) -> Vec<Attestor> {
    let mut keys: Vec<Attestor> = labels
        .iter()
        .map(|text| Attestor::from_label(text))
        .collect();
    keys.sort_by_key(|key| key.address);
    keys
}

fn attestor_set() -> Vec<Attestor> {
    keys(&[
        "PAXEERX_BRIDGE_SOLANA_TEST_ATTESTOR_1",
        "PAXEERX_BRIDGE_SOLANA_TEST_ATTESTOR_2",
        "PAXEERX_BRIDGE_SOLANA_TEST_ATTESTOR_3",
    ])
}

fn outsiders() -> Vec<Attestor> {
    keys(&[
        "PAXEERX_BRIDGE_SOLANA_TEST_OUTSIDER_1",
        "PAXEERX_BRIDGE_SOLANA_TEST_OUTSIDER_2",
    ])
}

/// One signature entry of a native secp256k1 instruction.
#[derive(Clone)]
struct Entry {
    address: [u8; HANDLE_BYTES],
    signature: [u8; 64],
    recovery: u8,
    message: Vec<u8>,
}

impl Entry {
    /// The malleable twin of this signature: s replaced by the order minus s
    /// and the recovery id flipped, which recovers the same signer.
    fn high_s(&self) -> Self {
        let mut twin = self.clone();
        let mut borrow = 0_i16;
        for index in (0..32).rev() {
            let difference =
                i16::from(ORDER[index]) - i16::from(self.signature[32 + index]) - borrow;
            borrow = i16::from(difference < 0);
            twin.signature[32 + index] =
                u8::try_from(difference + (borrow << 8)).expect("a byte of the difference");
        }
        twin.recovery ^= 1;
        assert!(twin.signature[32..] > HALF_ORDER[..]);
        twin
    }
}

/// The data of a native secp256k1 instruction carrying `entries`, every offset
/// pointing into this instruction at transaction index `index`. A message
/// several entries share is written once.
fn secp_data(entries: &[Entry], index: u8) -> Vec<u8> {
    let table = 1 + entries.len() * 11;
    let mut body: Vec<u8> = Vec::new();
    let mut written: Vec<(Vec<u8>, usize)> = Vec::new();
    let mut rows = Vec::new();
    for entry in entries {
        let message_offset = match written.iter().find(|(bytes, _)| *bytes == entry.message) {
            Some((_, offset)) => *offset,
            None => {
                let offset = table + body.len();
                body.extend_from_slice(&entry.message);
                written.push((entry.message.clone(), offset));
                offset
            }
        };
        let address_offset = table + body.len();
        body.extend_from_slice(&entry.address);
        let signature_offset = table + body.len();
        body.extend_from_slice(&entry.signature);
        body.push(entry.recovery);
        rows.push((
            signature_offset,
            address_offset,
            message_offset,
            entry.message.len(),
        ));
    }
    let mut data = vec![u8::try_from(entries.len()).expect("few entries")];
    for (signature_offset, address_offset, message_offset, message_size) in rows {
        data.extend_from_slice(&(signature_offset as u16).to_le_bytes());
        data.push(index);
        data.extend_from_slice(&(address_offset as u16).to_le_bytes());
        data.push(index);
        data.extend_from_slice(&(message_offset as u16).to_le_bytes());
        data.extend_from_slice(&(message_size as u16).to_le_bytes());
        data.push(index);
    }
    data.extend_from_slice(&body);
    data
}

fn secp_instruction(data: Vec<u8>) -> Instruction {
    Instruction {
        program_id: secp256k1_program::id(),
        accounts: vec![],
        data,
    }
}

fn sign_all(signers: &[&Attestor], message: &[u8]) -> Instruction {
    let entries: Vec<Entry> = signers.iter().map(|key| key.sign(message)).collect();
    secp_instruction(secp_data(&entries, 0))
}

#[derive(Debug)]
enum Failure {
    Runtime(String),
    Transaction(TransactionError),
}

/// One release request: which burn, to whom, how much, of which mint, paid
/// into which token account.
#[derive(Clone)]
struct Request {
    mint: Pubkey,
    recipient: Pubkey,
    recipient_token: Pubkey,
    paxeer_tx_hash: [u8; 32],
    paxeer_nonce: u64,
    amount: u64,
}

struct Harness {
    context: ProgramTestContext,
    program: Pubkey,
    config: Pubkey,
    vault: Pubkey,
    mint_authority: Keypair,
    vault_token: Pubkey,
    recipient_token: Pubkey,
    other_vault_tokens: HashMap<Pubkey, Pubkey>,
    attestors: Vec<Attestor>,
}

impl Harness {
    /// The custody program at the vector program id, initialised by its
    /// upgrade authority, holding the shared attestor set with threshold two,
    /// with Sidiora registered under its fixed id and the pinned Sidiora
    /// deposit locked in custody, and a token account for the vector
    /// recipient.
    async fn start() -> Self {
        let program = vector_program();
        let mint_authority = Keypair::new();
        let mut test = ProgramTest::new(
            "paxeer_x_bridge_solana_program",
            program,
            processor!(process_instruction),
        );
        let mut data = vec![0_u8; Mint::LEN];
        Mint {
            mint_authority: COption::Some(mint_authority.pubkey()),
            supply: 0,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        }
        .pack_into_slice(&mut data);
        test.add_account(
            SIDIORA_MINT,
            Account {
                lamports: Rent::default().minimum_balance(Mint::LEN),
                data,
                owner: spl_token::id(),
                executable: false,
                rent_epoch: 0,
            },
        );
        let context = test.start_with_context().await;
        let owner = context.payer.pubkey();
        let mut harness = Self {
            context,
            program,
            config: find_config_address(&program).0,
            vault: find_vault_authority(&program).0,
            mint_authority,
            vault_token: Pubkey::default(),
            recipient_token: Pubkey::default(),
            other_vault_tokens: HashMap::new(),
            attestors: attestor_set(),
        };
        harness.set_program_data(owner);
        let payer = harness.payer();

        let initialise = harness.initialise(&owner);
        harness.accept(&[initialise], &[]).await;
        let addresses: Vec<[u8; HANDLE_BYTES]> =
            harness.attestors.iter().map(|key| key.address).collect();
        let set = harness.set_attestors(&owner, &addresses, THRESHOLD);
        harness.accept(&[set], &[]).await;
        let register = harness.register_asset(&owner, &SIDIORA_MINT, Some(SIDIORA_ASSET_ID));
        harness.accept(&[register], &[]).await;

        let vault = harness.vault;
        harness.vault_token = harness.create_token_account(&SIDIORA_MINT, &vault).await;
        let source = harness
            .create_token_account(&SIDIORA_MINT, &payer.pubkey())
            .await;
        let authority = harness.mint_authority.insecure_clone();
        harness
            .mint_to(&SIDIORA_MINT, &source, &authority, DEPOSIT_AMOUNT)
            .await;
        let mut paxeer_recipient = [0_u8; 32];
        paxeer_recipient[12..]
            .copy_from_slice(&label("PAXEERX_BRIDGE_SOLANA_VECTOR_PAXEER_RECIPIENT")[12..]);
        let deposit = harness.deposit(&source, DEPOSIT_AMOUNT, paxeer_recipient);
        harness.accept(&[deposit], &[]).await;
        harness.recipient_token = harness
            .create_token_account(&SIDIORA_MINT, &vector_recipient())
            .await;
        harness
    }

    fn payer(&self) -> Keypair {
        self.context.payer.insecure_clone()
    }

    /// Place this program's `ProgramData` account naming `authority` as the
    /// upgrade authority, serialised from the upgradeable loader's own type.
    /// The runtime runs the program as a builtin and keeps no `ProgramData` of
    /// its own; the runtime's payer is the authority a deployment would have.
    fn set_program_data(&mut self, authority: Pubkey) {
        let state = UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(authority),
        };
        let bytes = UpgradeableLoaderState::size_of_programdata_metadata();
        let account = Account::new_data_with_space(
            Rent::default().minimum_balance(bytes),
            &state,
            bytes,
            &bpf_loader_upgradeable::id(),
        )
        .expect("the loader state serialises");
        let address = self.program_data();
        self.context
            .set_account(&address, &AccountSharedData::from(account));
    }

    fn program_data(&self) -> Pubkey {
        Pubkey::find_program_address(&[self.program.as_ref()], &bpf_loader_upgradeable::id()).0
    }

    /// The vector release: 4.2 SID to the vector recipient for the vector
    /// burn.
    fn vector_request(&self) -> Request {
        Request {
            mint: SIDIORA_MINT,
            recipient: vector_recipient(),
            recipient_token: self.recipient_token,
            paxeer_tx_hash: vector_burn(),
            paxeer_nonce: VECTOR_NONCE,
            amount: VECTOR_AMOUNT,
        }
    }

    /// The outbound preimage the attestors sign for `request`, built by the
    /// program's own builder from the asset id the registry holds.
    fn preimage(&self, request: &Request, asset_id: [u8; HANDLE_BYTES]) -> Vec<u8> {
        outbound_preimage(&Outbound {
            chain_id: SOLANA_CHAIN_ID,
            vault: vault_handle(&self.program),
            paxeer_tx_hash: request.paxeer_tx_hash,
            paxeer_nonce: request.paxeer_nonce,
            recipient: recipient_handle(&request.recipient),
            asset: asset_id,
            amount: request.amount,
        })
        .to_vec()
    }

    /// The first two attestors' signatures over `request`, in the native
    /// secp256k1 instruction placed first.
    fn attest(&self, request: &Request) -> Instruction {
        let preimage = self.preimage(request, SIDIORA_ASSET_ID);
        sign_all(&[&self.attestors[0], &self.attestors[1]], &preimage)
    }

    fn release(&self, request: &Request) -> Instruction {
        let payer = self.context.payer.pubkey();
        let mut tail = request.paxeer_tx_hash.to_vec();
        tail.extend_from_slice(&request.paxeer_nonce.to_be_bytes());
        tail.extend_from_slice(request.recipient.as_ref());
        tail.extend_from_slice(&request.amount.to_be_bytes());
        let burn = nullifier(&request.paxeer_tx_hash, request.paxeer_nonce);
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new_readonly(self.config, false),
                AccountMeta::new(find_asset_address(&self.program, &request.mint).0, false),
                AccountMeta::new_readonly(request.mint, false),
                AccountMeta::new_readonly(self.vault, false),
                AccountMeta::new(self.vault_token_of(&request.mint), false),
                AccountMeta::new(request.recipient_token, false),
                AccountMeta::new(find_nullifier_address(&self.program, &burn).0, false),
                AccountMeta::new_readonly(instructions_sysvar::ID, false),
                AccountMeta::new_readonly(spl_token::id(), false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: payload(OP_RELEASE, &tail),
        }
    }

    /// The vault-authority token account of `mint`: Sidiora's, or one a
    /// test opened for another mint.
    fn vault_token_of(&self, mint: &Pubkey) -> Pubkey {
        if *mint == SIDIORA_MINT {
            self.vault_token
        } else {
            *self
                .other_vault_tokens
                .get(mint)
                .expect("a vault token account exists for the mint")
        }
    }

    async fn send(
        &mut self,
        instructions: &[Instruction],
        signers: &[&Keypair],
    ) -> Result<Vec<String>, Failure> {
        let blockhash = self
            .context
            .get_new_latest_blockhash()
            .await
            .map_err(|error| Failure::Runtime(error.to_string()))?;
        let payer = self.payer();
        let mut transaction = Transaction::new_with_payer(instructions, Some(&payer.pubkey()));
        transaction.partial_sign(&[&payer], blockhash);
        for signer in signers {
            transaction.partial_sign(&[*signer], blockhash);
        }
        let outcome = self
            .context
            .banks_client
            .process_transaction_with_metadata(transaction)
            .await
            .map_err(|error| Failure::Runtime(error.to_string()))?;
        let logs = match outcome.metadata {
            Some(metadata) => metadata.log_messages,
            None => Vec::new(),
        };
        match outcome.result {
            Ok(()) => Ok(logs),
            Err(error) => Err(Failure::Transaction(error)),
        }
    }

    async fn accept(&mut self, instructions: &[Instruction], signers: &[&Keypair]) -> Vec<String> {
        match self.send(instructions, signers).await {
            Ok(logs) => logs,
            Err(Failure::Transaction(error)) => {
                panic!("the runtime refused a valid transaction: {error}")
            }
            Err(Failure::Runtime(message)) => {
                panic!("a valid transaction never reached the runtime: {message}")
            }
        }
    }

    /// Require the program to refuse the instruction at `index` with
    /// `expected`.
    async fn refuse(
        &mut self,
        instructions: &[Instruction],
        signers: &[&Keypair],
        index: u8,
        expected: BridgeError,
        what: &str,
    ) {
        match self.send(instructions, signers).await {
            Ok(_) => panic!("{what} was admitted"),
            Err(Failure::Transaction(TransactionError::InstructionError(
                at,
                InstructionError::Custom(code),
            ))) => {
                assert_eq!(at, index, "{what} was refused by another instruction");
                assert_eq!(
                    code, expected as u32,
                    "{what} was refused by the wrong rule"
                );
            }
            Err(Failure::Transaction(error)) => {
                panic!("{what} was refused outside the program: {error}")
            }
            Err(Failure::Runtime(message)) => {
                panic!("{what} never reached the runtime: {message}")
            }
        }
    }

    /// Require the native secp256k1 program, at index 0, to refuse the
    /// transaction before the release runs.
    async fn refuse_natively(&mut self, instructions: &[Instruction], what: &str) {
        match self.send(instructions, &[]).await {
            Ok(_) => panic!("{what} was admitted"),
            Err(Failure::Transaction(TransactionError::InstructionError(at, _))) => {
                assert_eq!(at, 0, "{what} was not refused by the secp256k1 program");
            }
            Err(Failure::Transaction(error)) => {
                panic!("{what} was refused before any instruction ran: {error}")
            }
            Err(Failure::Runtime(message)) => {
                panic!("{what} never reached the runtime: {message}")
            }
        }
    }

    async fn data(&mut self, key: Pubkey) -> Option<Vec<u8>> {
        self.context
            .banks_client
            .get_account(key)
            .await
            .expect("the runtime answers an account lookup")
            .map(|account| account.data)
    }

    async fn asset_record(&mut self, mint: &Pubkey) -> Asset {
        let key = find_asset_address(&self.program, mint).0;
        let data = self.data(key).await.expect("the asset account exists");
        Asset::decode(&data).expect("the asset record is this layout")
    }

    async fn token_amount(&mut self, key: Pubkey) -> u64 {
        let data = self.data(key).await.expect("the token account exists");
        TokenAccount::unpack(&data)
            .expect("the token account is an SPL token account")
            .amount
    }

    async fn rent_exempt(&mut self, bytes: usize) -> u64 {
        self.context
            .banks_client
            .get_rent()
            .await
            .expect("the runtime answers a rent lookup")
            .minimum_balance(bytes)
    }

    async fn create_mint(&mut self, decimals: u8) -> Pubkey {
        let mint = Keypair::new();
        let payer = self.payer();
        let lamports = self.rent_exempt(Mint::LEN).await;
        let create = system_instruction::create_account(
            &payer.pubkey(),
            &mint.pubkey(),
            lamports,
            Mint::LEN as u64,
            &spl_token::id(),
        );
        let initialise = spl_token::instruction::initialize_mint(
            &spl_token::id(),
            &mint.pubkey(),
            &self.mint_authority.pubkey(),
            None,
            decimals,
        )
        .expect("the SPL mint initialiser is well formed");
        self.accept(&[create, initialise], &[&mint]).await;
        mint.pubkey()
    }

    async fn create_token_account(&mut self, mint: &Pubkey, owner: &Pubkey) -> Pubkey {
        let account = Keypair::new();
        let payer = self.payer();
        let lamports = self.rent_exempt(TokenAccount::LEN).await;
        let create = system_instruction::create_account(
            &payer.pubkey(),
            &account.pubkey(),
            lamports,
            TokenAccount::LEN as u64,
            &spl_token::id(),
        );
        let initialise = spl_token::instruction::initialize_account(
            &spl_token::id(),
            &account.pubkey(),
            mint,
            owner,
        )
        .expect("the SPL token account initialiser is well formed");
        self.accept(&[create, initialise], &[&account]).await;
        account.pubkey()
    }

    async fn mint_to(&mut self, mint: &Pubkey, account: &Pubkey, authority: &Keypair, amount: u64) {
        let instruction = spl_token::instruction::mint_to(
            &spl_token::id(),
            mint,
            account,
            &authority.pubkey(),
            &[],
            amount,
        )
        .expect("the SPL mint_to is well formed");
        self.accept(&[instruction], &[authority]).await;
    }

    fn initialise(&self, owner: &Pubkey) -> Instruction {
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new(self.context.payer.pubkey(), true),
                AccountMeta::new(self.config, false),
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new_readonly(self.program_data(), false),
            ],
            data: payload(OP_INITIALISE, owner.as_ref()),
        }
    }

    fn set_attestors(
        &self,
        owner: &Pubkey,
        attestors: &[[u8; HANDLE_BYTES]],
        threshold: u8,
    ) -> Instruction {
        let mut tail = vec![u8::try_from(attestors.len()).expect("few attestors")];
        for entry in attestors {
            tail.extend_from_slice(entry);
        }
        tail.push(threshold);
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new_readonly(*owner, true),
                AccountMeta::new(self.config, false),
            ],
            data: payload(OP_SET_ATTESTORS, &tail),
        }
    }

    fn register_asset(
        &self,
        owner: &Pubkey,
        mint: &Pubkey,
        asset_id: Option<[u8; HANDLE_BYTES]>,
    ) -> Instruction {
        let mut tail = Vec::new();
        match asset_id {
            Some(id) => {
                tail.push(1);
                tail.extend_from_slice(&id);
            }
            None => tail.push(0),
        }
        tail.extend_from_slice(&PER_TX_CAP.to_be_bytes());
        tail.extend_from_slice(&TOTAL_CAP.to_be_bytes());
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new(*owner, true),
                AccountMeta::new_readonly(self.config, false),
                AccountMeta::new(find_asset_address(&self.program, mint).0, false),
                AccountMeta::new_readonly(*mint, false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: payload(OP_REGISTER_ASSET, &tail),
        }
    }

    fn set_cap(&self, mint: &Pubkey, per_tx_cap: u64, enabled: bool) -> Instruction {
        let mut tail = per_tx_cap.to_be_bytes().to_vec();
        tail.extend_from_slice(&TOTAL_CAP.to_be_bytes());
        tail.push(u8::from(enabled));
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new_readonly(self.context.payer.pubkey(), true),
                AccountMeta::new_readonly(self.config, false),
                AccountMeta::new(find_asset_address(&self.program, mint).0, false),
            ],
            data: payload(OP_SET_CAP, &tail),
        }
    }

    fn set_pause(&self, paused: bool) -> Instruction {
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new_readonly(self.context.payer.pubkey(), true),
                AccountMeta::new(self.config, false),
            ],
            data: payload(OP_SET_PAUSE, &[u8::from(paused)]),
        }
    }

    fn deposit(&self, source: &Pubkey, amount: u64, paxeer_recipient: [u8; 32]) -> Instruction {
        let mut tail = amount.to_be_bytes().to_vec();
        tail.extend_from_slice(&paxeer_recipient);
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new(self.context.payer.pubkey(), true),
                AccountMeta::new(self.config, false),
                AccountMeta::new(find_asset_address(&self.program, &SIDIORA_MINT).0, false),
                AccountMeta::new_readonly(SIDIORA_MINT, false),
                AccountMeta::new(*source, false),
                AccountMeta::new(self.vault_token, false),
                AccountMeta::new(find_receipt_address(&self.program, 1).0, false),
                AccountMeta::new_readonly(spl_token::id(), false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: payload(OP_DEPOSIT, &tail),
        }
    }

    fn register_recipient(
        &self,
        registrant: &Pubkey,
        signs: bool,
        handle: [u8; HANDLE_BYTES],
    ) -> Instruction {
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new(self.context.payer.pubkey(), true),
                AccountMeta::new_readonly(self.config, false),
                AccountMeta::new_readonly(*registrant, signs),
                AccountMeta::new(find_recipient_address(&self.program, &handle).0, false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: payload(OP_REGISTER_RECIPIENT, &handle),
        }
    }
}

#[test]
fn the_release_signs_the_pinned_vector() {
    let program = vector_program();
    assert_eq!(
        program.to_string(),
        "A7SZbByPYuHpunZ9pyMDMrhMYvK44ANT1AVqb8U1FpM9"
    );
    assert_eq!(
        hex(&vault_handle(&program)),
        "334121a65b47bd45c3f6381537d9180e98e445bc"
    );
    let preimage = outbound_preimage(&Outbound {
        chain_id: SOLANA_CHAIN_ID,
        vault: vault_handle(&program),
        paxeer_tx_hash: vector_burn(),
        paxeer_nonce: VECTOR_NONCE,
        recipient: recipient_handle(&vector_recipient()),
        asset: SIDIORA_ASSET_ID,
        amount: VECTOR_AMOUNT,
    });
    assert_eq!(preimage.len(), OUTBOUND_PREIMAGE_BYTES);
    assert_eq!(preimage.to_vec(), unhex(PINNED_PREIMAGE));
    assert_eq!(hex(&keccak::hash(&preimage).to_bytes()), PINNED_DIGEST);
    assert_eq!(
        hex(&nullifier(&vector_burn(), VECTOR_NONCE)),
        PINNED_NULLIFIER
    );
}

#[tokio::test]
async fn a_release_pays_the_recipient_once() {
    let mut harness = Harness::start().await;
    let request = harness.vector_request();
    assert_eq!(
        harness.preimage(&request, SIDIORA_ASSET_ID),
        unhex(PINNED_PREIMAGE),
        "the harness does not release the pinned vector"
    );
    // The attestors sign the pinned bytes themselves, so a program that rebuilt
    // any other preimage would refuse this release.
    let attest = sign_all(
        &[&harness.attestors[0], &harness.attestors[1]],
        &unhex(PINNED_PREIMAGE),
    );
    let release = harness.release(&request);
    let vault_token = harness.vault_token;
    let recipient_token = harness.recipient_token;
    harness.accept(&[attest, release.clone()], &[]).await;

    assert_eq!(harness.token_amount(recipient_token).await, VECTOR_AMOUNT);
    assert_eq!(
        harness.token_amount(vault_token).await,
        DEPOSIT_AMOUNT - VECTOR_AMOUNT
    );
    assert_eq!(
        harness.asset_record(&SIDIORA_MINT).await.outstanding,
        DEPOSIT_AMOUNT - VECTOR_AMOUNT
    );

    let burn = nullifier(&request.paxeer_tx_hash, request.paxeer_nonce);
    assert_eq!(hex(&burn), PINNED_NULLIFIER);
    let key = find_nullifier_address(&harness.program, &burn).0;
    let program = harness.program;
    let account = harness
        .context
        .banks_client
        .get_account(key)
        .await
        .expect("the runtime answers an account lookup")
        .expect("the release created the nullifier");
    assert_eq!(account.owner, program);
    let record = NullifierRecord::decode(&account.data).expect("the nullifier is this layout");
    assert_eq!(record.paxeer_tx_hash, vector_burn());
    assert_eq!(record.paxeer_nonce, VECTOR_NONCE);
    assert_eq!(record.mint, SIDIORA_MINT);
    assert_eq!(record.recipient, vector_recipient());
    assert_eq!(record.amount, VECTOR_AMOUNT);
    assert_eq!(
        release_record(
            &record,
            &SIDIORA_ASSET_ID,
            &recipient_handle(&vector_recipient()),
        ),
        concat!(
            "PXBR/release/v1 asset=21f7b20a555199fa73a238b1a91fd0f549068fee",
            " mint=5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump amount=4200000",
            " recipient=59TLtNdRpCZysEHkGDMPFHkHiHXkBqQVqAVxAupzNoQb",
            " handle=fb02125a3275d53a9f6538626b49894d2aae80cc",
            " paxeer_tx_hash=6f79d9a61a77030bbeba5f435907f53b09321779310326b9faaebb391b3b5d5f",
            " paxeer_nonce=11",
        ),
        "the release logs the asset, the amount, the recipient and the burn"
    );

    // The same burn, attested again and sent in a fresh transaction.
    let attest = harness.attest(&request);
    harness
        .refuse(
            &[attest, release],
            &[],
            RELEASE_INDEX,
            BridgeError::Replayed,
            "a replayed release",
        )
        .await;
    assert_eq!(harness.token_amount(recipient_token).await, VECTOR_AMOUNT);
}

#[tokio::test]
async fn a_release_is_refused_unless_the_native_program_verified_the_attestors() {
    let mut harness = Harness::start().await;
    let request = harness.vector_request();
    let preimage = harness.preimage(&request, SIDIORA_ASSET_ID);
    let release = harness.release(&request);
    let recipient_token = harness.recipient_token;
    let set = attestor_set();
    let (first, second) = (&set[0], &set[1]);
    let outside = outsiders();

    // No secp256k1 instruction at all.
    harness
        .refuse(
            std::slice::from_ref(&release),
            &[],
            0,
            BridgeError::AttestationMissing,
            "a release with no attestation",
        )
        .await;

    // A valid attestation that is not the instruction directly before the
    // release.
    let payer = harness.context.payer.pubkey();
    let attest = sign_all(&[first, second], &preimage);
    let between = system_instruction::transfer(&payer, &harness.config, 1);
    harness
        .refuse(
            &[attest, between, release.clone()],
            &[],
            2,
            BridgeError::AttestationMissing,
            "an attestation separated from its release",
        )
        .await;

    // Threshold short by one.
    let attest = sign_all(&[first], &preimage);
    harness
        .refuse(
            &[attest, release.clone()],
            &[],
            RELEASE_INDEX,
            BridgeError::AttestationThreshold,
            "a release one signature short of the threshold",
        )
        .await;

    // A wrong signer set: real signatures, verified by the native program, by
    // keys that are not attestors.
    let attest = sign_all(&[&outside[0], &outside[1]], &preimage);
    harness
        .refuse(
            &[attest, release.clone()],
            &[],
            RELEASE_INDEX,
            BridgeError::AttestationSigner,
            "a release signed by the wrong signer set",
        )
        .await;

    // One attestor and one stranger, in ascending order.
    let mut mixed = [first.sign(&preimage), outside[0].sign(&preimage)];
    mixed.sort_by_key(|entry| entry.address);
    let attest = secp_instruction(secp_data(&mixed, 0));
    harness
        .refuse(
            &[attest, release.clone()],
            &[],
            RELEASE_INDEX,
            BridgeError::AttestationSigner,
            "a release counting an unknown signer",
        )
        .await;

    // Signatures over a different message: the same burn for one unit more.
    let mut larger = request.clone();
    larger.amount += 1;
    let attest = sign_all(
        &[first, second],
        &harness.preimage(&larger, SIDIORA_ASSET_ID),
    );
    harness
        .refuse(
            &[attest, release.clone()],
            &[],
            RELEASE_INDEX,
            BridgeError::AttestationMessage,
            "a release attested over another amount",
        )
        .await;

    // Signatures over another recipient's handle.
    let mut elsewhere = request.clone();
    elsewhere.recipient = Pubkey::new_unique();
    let attest = sign_all(
        &[first, second],
        &harness.preimage(&elsewhere, SIDIORA_ASSET_ID),
    );
    harness
        .refuse(
            &[attest, release.clone()],
            &[],
            RELEASE_INDEX,
            BridgeError::AttestationMessage,
            "a release attested for another recipient",
        )
        .await;

    // The malleable twin of a valid signature: the native program recovers the
    // same attestor from it, and the program refuses its high s.
    let entries = [first.sign(&preimage).high_s(), second.sign(&preimage)];
    let attest = secp_instruction(secp_data(&entries, 0));
    harness
        .refuse(
            &[attest, release.clone()],
            &[],
            RELEASE_INDEX,
            BridgeError::AttestationMalleable,
            "a release carrying a high-s signature",
        )
        .await;

    // Two attestors in descending order.
    let entries = [second.sign(&preimage), first.sign(&preimage)];
    let attest = secp_instruction(secp_data(&entries, 0));
    harness
        .refuse(
            &[attest, release.clone()],
            &[],
            RELEASE_INDEX,
            BridgeError::AttestationSigner,
            "a release with its signers out of order",
        )
        .await;

    // One attestor counted twice.
    let entries = [first.sign(&preimage), first.sign(&preimage)];
    let attest = secp_instruction(secp_data(&entries, 0));
    harness
        .refuse(
            &[attest, release.clone()],
            &[],
            RELEASE_INDEX,
            BridgeError::AttestationSigner,
            "a release with a repeated signer",
        )
        .await;

    // A secp256k1 instruction whose entries resolve inside another instruction:
    // the native program verifies both, and the program refuses the one before
    // the release because its offsets leave its own data.
    let valid = secp_data(&[first.sign(&preimage), second.sign(&preimage)], 0);
    let borrowed = valid[..1 + 2 * 11].to_vec();
    harness
        .refuse(
            &[
                secp_instruction(valid),
                secp_instruction(borrowed),
                release.clone(),
            ],
            &[],
            2,
            BridgeError::AttestationMalformed,
            "a release whose attestation points into another instruction",
        )
        .await;

    // A forged signature never reaches the program: the native program
    // recovers another signer and refuses the transaction itself.
    let mut forged = first.sign(&preimage);
    forged.signature[0] ^= 0x01;
    let attest = secp_instruction(secp_data(&[forged, second.sign(&preimage)], 0));
    harness
        .refuse_natively(&[attest, release.clone()], "a forged signature")
        .await;

    // An offset past the end of the instruction's data.
    let mut truncated = secp_data(&[first.sign(&preimage), second.sign(&preimage)], 0);
    truncated.truncate(truncated.len() - 1);
    harness
        .refuse_natively(
            &[secp_instruction(truncated), release.clone()],
            "an attestation whose offsets overrun its data",
        )
        .await;

    // A malformed release payload: the wrong layout version, and a trailing
    // byte.
    let attest = sign_all(&[first, second], &preimage);
    let mut wrong_version = release.clone();
    wrong_version.data[5] ^= 0x01;
    harness
        .refuse(
            &[attest.clone(), wrong_version],
            &[],
            RELEASE_INDEX,
            BridgeError::Instruction,
            "a release of another layout version",
        )
        .await;
    let mut trailing = release.clone();
    trailing.data.push(0);
    harness
        .refuse(
            &[attest.clone(), trailing],
            &[],
            RELEASE_INDEX,
            BridgeError::Instruction,
            "a release with a trailing byte",
        )
        .await;

    // Nothing above consumed the burn or moved a token: the attested release
    // still pays.
    assert_eq!(harness.token_amount(recipient_token).await, 0);
    harness.accept(&[attest, release], &[]).await;
    assert_eq!(harness.token_amount(recipient_token).await, VECTOR_AMOUNT);
}

#[tokio::test]
async fn a_release_is_refused_outside_what_custody_allows() {
    let mut harness = Harness::start().await;
    let payer = harness.payer();
    let request = harness.vector_request();

    // Paused.
    let pause = harness.set_pause(true);
    harness.accept(&[pause], &[]).await;
    let attest = harness.attest(&request);
    let release = harness.release(&request);
    harness
        .refuse(
            &[attest, release],
            &[],
            RELEASE_INDEX,
            BridgeError::Paused,
            "a release while paused",
        )
        .await;
    let unpause = harness.set_pause(false);
    harness.accept(&[unpause], &[]).await;

    // A zero amount.
    let mut zero = request.clone();
    zero.amount = 0;
    let attest = harness.attest(&zero);
    let release = harness.release(&zero);
    harness
        .refuse(
            &[attest, release],
            &[],
            RELEASE_INDEX,
            BridgeError::Bounds,
            "a release of nothing",
        )
        .await;

    // Above the per-transaction cap, within custody.
    let lower = harness.set_cap(&SIDIORA_MINT, VECTOR_AMOUNT - 1, true);
    harness.accept(&[lower], &[]).await;
    let attest = harness.attest(&request);
    let release = harness.release(&request);
    harness
        .refuse(
            &[attest, release],
            &[],
            RELEASE_INDEX,
            BridgeError::Cap,
            "a release above the per-transaction cap",
        )
        .await;

    // Within the per-transaction cap, above what custody holds.
    let raise = harness.set_cap(&SIDIORA_MINT, TOTAL_CAP, true);
    harness.accept(&[raise], &[]).await;
    let mut beyond = request.clone();
    beyond.amount = DEPOSIT_AMOUNT + 1;
    let attest = harness.attest(&beyond);
    let release = harness.release(&beyond);
    harness
        .refuse(
            &[attest, release],
            &[],
            RELEASE_INDEX,
            BridgeError::Outstanding,
            "a release above the outstanding amount",
        )
        .await;

    // A disabled mint.
    let disable = harness.set_cap(&SIDIORA_MINT, PER_TX_CAP, false);
    harness.accept(&[disable], &[]).await;
    let attest = harness.attest(&request);
    let release = harness.release(&request);
    harness
        .refuse(
            &[attest, release],
            &[],
            RELEASE_INDEX,
            BridgeError::Asset,
            "a release of a disabled mint",
        )
        .await;
    let enable = harness.set_cap(&SIDIORA_MINT, PER_TX_CAP, true);
    harness.accept(&[enable], &[]).await;

    // An unregistered mint, with real token accounts for it on both sides.
    let other = harness.create_mint(6).await;
    let vault = harness.vault;
    let other_vault = harness.create_token_account(&other, &vault).await;
    harness.other_vault_tokens.insert(other, other_vault);
    let other_recipient = harness
        .create_token_account(&other, &vector_recipient())
        .await;
    let mut unregistered = request.clone();
    unregistered.mint = other;
    unregistered.recipient_token = other_recipient;
    let attest = sign_all(
        &[&harness.attestors[0], &harness.attestors[1]],
        &harness.preimage(&unregistered, pubkey_handle(&other)),
    );
    let release = harness.release(&unregistered);
    harness
        .refuse(
            &[attest, release],
            &[],
            RELEASE_INDEX,
            BridgeError::Asset,
            "a release of an unregistered mint",
        )
        .await;

    // A token account the recipient does not own.
    let foreign = harness
        .create_token_account(&SIDIORA_MINT, &payer.pubkey())
        .await;
    let mut misdirected = request.clone();
    misdirected.recipient_token = foreign;
    let attest = harness.attest(&misdirected);
    let release = harness.release(&misdirected);
    harness
        .refuse(
            &[attest, release],
            &[],
            RELEASE_INDEX,
            BridgeError::Recipient,
            "a release into a token account its recipient does not own",
        )
        .await;

    // The recipient's own token account, but of another mint.
    let mut wrong_mint = request.clone();
    wrong_mint.recipient_token = other_recipient;
    let attest = harness.attest(&wrong_mint);
    let release = harness.release(&wrong_mint);
    harness
        .refuse(
            &[attest, release],
            &[],
            RELEASE_INDEX,
            BridgeError::Recipient,
            "a release into the recipient's account of another mint",
        )
        .await;

    // After every refusal, custody is exactly the deposit.
    assert_eq!(
        harness.asset_record(&SIDIORA_MINT).await.outstanding,
        DEPOSIT_AMOUNT
    );
    let vault_token = harness.vault_token;
    assert_eq!(harness.token_amount(vault_token).await, DEPOSIT_AMOUNT);
}

#[tokio::test]
async fn a_registered_recipient_is_paid_by_the_key_its_record_holds() {
    let mut harness = Harness::start().await;
    let registrant = Keypair::new();
    let own = pubkey_handle(&registrant.pubkey());

    // A handle that is not the handle of the signing key.
    let foreign = harness.register_recipient(&registrant.pubkey(), true, [0x42; HANDLE_BYTES]);
    harness
        .refuse(
            &[foreign],
            &[&registrant],
            0,
            BridgeError::Recipient,
            "a registration under another key's handle",
        )
        .await;

    // The key named without its signature.
    let unsigned = harness.register_recipient(&registrant.pubkey(), false, own);
    harness
        .refuse(
            &[unsigned],
            &[],
            0,
            BridgeError::Authority,
            "a registration its key did not sign",
        )
        .await;

    let register = harness.register_recipient(&registrant.pubkey(), true, own);
    harness.accept(&[register], &[&registrant]).await;

    // The relayer's read-back: the handle a Paxeer burn names resolves to the
    // key this record holds.
    let key = find_recipient_address(&harness.program, &own).0;
    let data = harness
        .data(key)
        .await
        .expect("the recipient record exists");
    let record = RecipientRecord::decode(&data).expect("the recipient record is this layout");
    assert_eq!(record.handle, own);
    assert_eq!(record.key, registrant.pubkey());

    // A second registration of the same handle.
    let again = harness.register_recipient(&registrant.pubkey(), true, own);
    harness
        .refuse(
            &[again],
            &[&registrant],
            0,
            BridgeError::Conflict,
            "a second registration of one handle",
        )
        .await;

    // A burn addressed to the handle pays the key the record holds.
    let recipient_token = harness
        .create_token_account(&SIDIORA_MINT, &record.key)
        .await;
    let request = Request {
        mint: SIDIORA_MINT,
        recipient: record.key,
        recipient_token,
        paxeer_tx_hash: label("PAXEERX_BRIDGE_SOLANA_TEST_REGISTERED_BURN"),
        paxeer_nonce: 1,
        amount: 1_000_000,
    };
    assert_eq!(recipient_handle(&request.recipient), own);
    let attest = harness.attest(&request);
    let release = harness.release(&request);
    harness.accept(&[attest, release], &[]).await;
    assert_eq!(harness.token_amount(recipient_token).await, 1_000_000);
}
