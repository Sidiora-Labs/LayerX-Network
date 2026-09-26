//! The custody core against the real Solana runtime.
//!
//! Every test here runs the real program through `solana-program-test`, against
//! the real SPL Token program the runtime loads at genesis, with real mints and
//! real token accounts. No account layout is written by a test and no refusal is
//! asserted through a stub: the codes these tests match are the codes the
//! program returns.

use paxeer_x_bridge_solana_program::deposit::deposit_record;
use paxeer_x_bridge_solana_program::identity::{
    find_vault_authority, handle, hex, paxeer_address, pubkey_handle, recipient_record,
    vault_handle, HANDLE_BYTES, SIDIORA_ASSET_ID, SIDIORA_MINT, SOLANA_CHAIN_ID,
};
use paxeer_x_bridge_solana_program::state::{
    find_asset_address, find_config_address, find_receipt_address, find_recipient_address, Asset,
    Config, DepositReceipt, RecipientRecord, CONFIG_BYTES, MAX_ATTESTORS, VAULT_SEED,
};
use paxeer_x_bridge_solana_program::{
    process_instruction, BridgeError, INSTRUCTION_MAGIC, INSTRUCTION_VERSION, OP_ACCEPT_OWNER,
    OP_DEPOSIT, OP_INITIALISE, OP_PROPOSE_OWNER, OP_REGISTER_ASSET, OP_REGISTER_RECIPIENT,
    OP_SET_ATTESTORS, OP_SET_CAP, OP_SET_PAUSE,
};
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_program::instruction::{AccountMeta, Instruction, InstructionError};
use solana_program::keccak;
use solana_program::program_option::COption;
use solana_program::program_pack::Pack;
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program_test::{processor, ProgramTest, ProgramTestContext};
use solana_sdk::account::{Account, AccountSharedData};
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::{Transaction, TransactionError};
use solana_sdk_ids::{bpf_loader_upgradeable, system_program};
use solana_system_interface::instruction as system_instruction;
use spl_token::state::{Account as TokenAccount, Mint};

/// An explicit asset id for an ordinary mint: any non-zero 20 bytes that are
/// not Sidiora's, which the program binds to Sidiora's mint alone.
const NAMED_ASSET_ID: [u8; HANDLE_BYTES] = [0x5a; HANDLE_BYTES];

fn payload(opcode: u8, tail: &[u8]) -> Vec<u8> {
    let mut data = INSTRUCTION_MAGIC.to_vec();
    data.extend_from_slice(&INSTRUCTION_VERSION.to_be_bytes());
    data.push(opcode);
    data.extend_from_slice(tail);
    data
}

fn recipient(low: [u8; HANDLE_BYTES]) -> [u8; 32] {
    let mut out = [0_u8; 32];
    out[12..].copy_from_slice(&low);
    out
}

fn attestor(byte: u8) -> [u8; HANDLE_BYTES] {
    [byte; HANDLE_BYTES]
}

#[derive(Debug)]
enum Failure {
    Runtime(String),
    Transaction(TransactionError),
}

struct Harness {
    context: ProgramTestContext,
    program: Pubkey,
    config: Pubkey,
    vault: Pubkey,
}

impl Harness {
    async fn start() -> Self {
        Self::launch(false).await
    }

    /// A runtime that also holds a mint at Sidiora's real address. No keypair
    /// for that address exists, so the mint is placed at genesis, packed by the
    /// SPL Token crate's own `Mint` type with Sidiora's six decimals and no
    /// mint authority, and owned by the SPL Token program the runtime loads.
    async fn start_with_sidiora_mint() -> Self {
        Self::launch(true).await
    }

    async fn launch(with_sidiora_mint: bool) -> Self {
        let program = Pubkey::new_unique();
        let mut test = ProgramTest::new(
            "paxeer_x_bridge_solana_program",
            program,
            processor!(process_instruction),
        );
        if with_sidiora_mint {
            let mut data = vec![0_u8; Mint::LEN];
            Mint {
                mint_authority: COption::None,
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
        }
        let context = test.start_with_context().await;
        let authority = context.payer.pubkey();
        let mut harness = Self {
            context,
            program,
            config: find_config_address(&program).0,
            vault: find_vault_authority(&program).0,
        };
        harness.set_program_data(Some(authority), bpf_loader_upgradeable::id());
        harness
    }

    /// The address the upgradeable loader derives for this program's
    /// `ProgramData` account.
    fn program_data(&self) -> Pubkey {
        Pubkey::find_program_address(&[self.program.as_ref()], &bpf_loader_upgradeable::id()).0
    }

    /// Place this program's `ProgramData` account, serialised from the
    /// upgradeable loader's own `UpgradeableLoaderState` type, naming
    /// `authority` as the upgrade authority and owned by `owner`. The runtime
    /// runs the program as a builtin, so it keeps no `ProgramData` of its own;
    /// the runtime's payer is the authority a deployment would have.
    fn set_program_data(&mut self, authority: Option<Pubkey>, owner: Pubkey) {
        let state = UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: authority,
        };
        let bytes = UpgradeableLoaderState::size_of_programdata_metadata();
        let account = Account::new_data_with_space(
            Rent::default().minimum_balance(bytes),
            &state,
            bytes,
            &owner,
        )
        .expect("the loader state serialises");
        let address = self.program_data();
        self.context
            .set_account(&address, &AccountSharedData::from(account));
    }

    fn payer(&self) -> Keypair {
        self.context.payer.insecure_clone()
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
                panic!("the program refused a valid instruction: {error}")
            }
            Err(Failure::Runtime(message)) => {
                panic!("a valid instruction never reached the program: {message}")
            }
        }
    }

    async fn refuse(
        &mut self,
        instructions: &[Instruction],
        signers: &[&Keypair],
        expected: BridgeError,
        what: &str,
    ) {
        match self.send(instructions, signers).await {
            Ok(_) => panic!("{what} was admitted"),
            Err(Failure::Transaction(TransactionError::InstructionError(
                _,
                InstructionError::Custom(code),
            ))) => assert_eq!(
                code, expected as u32,
                "{what} was refused by the wrong rule"
            ),
            Err(Failure::Transaction(error)) => {
                panic!("{what} was refused outside the program: {error}")
            }
            Err(Failure::Runtime(message)) => {
                panic!("{what} never reached the program: {message}")
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

    async fn config_record(&mut self) -> Config {
        let key = self.config;
        let data = self.data(key).await.expect("the config account exists");
        Config::decode(&data).expect("the config record is this layout")
    }

    async fn asset_record(&mut self, mint: &Pubkey) -> Asset {
        let key = find_asset_address(&self.program, mint).0;
        let data = self.data(key).await.expect("the asset account exists");
        Asset::decode(&data).expect("the asset record is this layout")
    }

    async fn recipient_record(&mut self, handle: &[u8; HANDLE_BYTES]) -> RecipientRecord {
        let key = find_recipient_address(&self.program, handle).0;
        let data = self.data(key).await.expect("the recipient account exists");
        RecipientRecord::decode(&data).expect("the recipient record is this layout")
    }

    async fn receipt_record(&mut self, nonce: u64) -> DepositReceipt {
        let key = find_receipt_address(&self.program, nonce).0;
        let data = self.data(key).await.expect("the receipt account exists");
        DepositReceipt::decode(&data).expect("the receipt record is this layout")
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

    /// Fund `account` from the runtime's payer, so it can sign instructions that
    /// create accounts of their own.
    async fn fund(&mut self, account: &Pubkey, lamports: u64) {
        let payer = self.payer();
        let transfer = system_instruction::transfer(&payer.pubkey(), account, lamports);
        self.accept(&[transfer], &[]).await;
    }

    /// Create a real SPL mint under `authority`.
    async fn create_mint(&mut self, authority: &Pubkey, decimals: u8) -> Pubkey {
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
            authority,
            None,
            decimals,
        )
        .expect("the SPL mint initialiser is well formed");
        self.accept(&[create, initialise], &[&mint]).await;
        mint.pubkey()
    }

    /// Create a real SPL token account for `mint` under `owner`.
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

    fn initialise(&self, payer: &Pubkey, owner: &Pubkey) -> Instruction {
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new(*payer, true),
                AccountMeta::new(self.config, false),
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new_readonly(self.program_data(), false),
            ],
            data: payload(OP_INITIALISE, owner.as_ref()),
        }
    }

    fn propose_owner(&self, owner: &Pubkey, pending: &Pubkey) -> Instruction {
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new_readonly(*owner, true),
                AccountMeta::new(self.config, false),
            ],
            data: payload(OP_PROPOSE_OWNER, pending.as_ref()),
        }
    }

    fn accept_owner(&self, pending: &Pubkey) -> Instruction {
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new_readonly(*pending, true),
                AccountMeta::new(self.config, false),
            ],
            data: payload(OP_ACCEPT_OWNER, &[]),
        }
    }

    fn set_attestors(
        &self,
        owner: &Pubkey,
        attestors: &[[u8; HANDLE_BYTES]],
        threshold: u8,
    ) -> Instruction {
        let mut tail = vec![attestors.len() as u8];
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
        per_tx_cap: u64,
        total_cap: u64,
    ) -> Instruction {
        let mut tail = Vec::new();
        match asset_id {
            Some(id) => {
                tail.push(1);
                tail.extend_from_slice(&id);
            }
            None => tail.push(0),
        }
        tail.extend_from_slice(&per_tx_cap.to_be_bytes());
        tail.extend_from_slice(&total_cap.to_be_bytes());
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

    fn set_cap(
        &self,
        owner: &Pubkey,
        mint: &Pubkey,
        per_tx_cap: u64,
        total_cap: u64,
        enabled: bool,
    ) -> Instruction {
        let mut tail = per_tx_cap.to_be_bytes().to_vec();
        tail.extend_from_slice(&total_cap.to_be_bytes());
        tail.push(u8::from(enabled));
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new_readonly(*owner, true),
                AccountMeta::new_readonly(self.config, false),
                AccountMeta::new(find_asset_address(&self.program, mint).0, false),
            ],
            data: payload(OP_SET_CAP, &tail),
        }
    }

    fn register_recipient(
        &self,
        payer: &Pubkey,
        registrant: &Pubkey,
        signs: bool,
        handle: [u8; HANDLE_BYTES],
        record: Pubkey,
    ) -> Instruction {
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new(*payer, true),
                AccountMeta::new_readonly(self.config, false),
                AccountMeta::new_readonly(*registrant, signs),
                AccountMeta::new(record, false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: payload(OP_REGISTER_RECIPIENT, &handle),
        }
    }

    fn set_pause(&self, owner: &Pubkey, paused: bool) -> Instruction {
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new_readonly(*owner, true),
                AccountMeta::new(self.config, false),
            ],
            data: payload(OP_SET_PAUSE, &[u8::from(paused)]),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn deposit(
        &self,
        depositor: &Pubkey,
        mint: &Pubkey,
        source: &Pubkey,
        vault_token: &Pubkey,
        nonce: u64,
        amount: u64,
        paxeer_recipient: [u8; 32],
    ) -> Instruction {
        let mut tail = amount.to_be_bytes().to_vec();
        tail.extend_from_slice(&paxeer_recipient);
        Instruction {
            program_id: self.program,
            accounts: vec![
                AccountMeta::new(*depositor, true),
                AccountMeta::new(self.config, false),
                AccountMeta::new(find_asset_address(&self.program, mint).0, false),
                AccountMeta::new_readonly(*mint, false),
                AccountMeta::new(*source, false),
                AccountMeta::new(*vault_token, false),
                AccountMeta::new(find_receipt_address(&self.program, nonce).0, false),
                AccountMeta::new_readonly(spl_token::id(), false),
                AccountMeta::new_readonly(system_program::id(), false),
            ],
            data: payload(OP_DEPOSIT, &tail),
        }
    }
}

#[test]
fn the_identity_mapping_is_the_one_the_bridge_signs() {
    assert_eq!(
        SOLANA_CHAIN_ID,
        u64::from_be_bytes([0, 0, b'S', b'O', b'L', b'A', b'N', b'A']),
        "Solana's chain id is the padded big-endian ASCII of SOLANA"
    );
    assert_eq!(
        hex(&handle(&[0_u8; 32])),
        "88386fc84ba6bc95484008f6362f93160ef3e563",
        "the handle is the last 20 bytes of keccak256 of the key"
    );
    let program = Pubkey::new_unique();
    assert_eq!(
        vault_handle(&program),
        pubkey_handle(&find_vault_authority(&program).0),
        "the vault handle is the handle of the vault-authority PDA"
    );
    // The shared attestation vectors derive their program id from a label and
    // then derive the vault authority from the single seed below. Pinning the
    // whole chain here keeps the custody program and the vectors in step: if
    // either the seed or the derivation moves, this fails before a relayer
    // signs against a vault the program does not own.
    assert_eq!(VAULT_SEED, b"vault-authority");
    let vector_program =
        Pubkey::new_from_array(keccak::hash(b"PAXEERX_BRIDGE_SOLANA_VECTOR_PROGRAM").to_bytes());
    assert_eq!(
        vector_program.to_string(),
        "A7SZbByPYuHpunZ9pyMDMrhMYvK44ANT1AVqb8U1FpM9",
        "the vectors label the program id with the keccak256 of its name"
    );
    let (vector_vault, vector_bump) = find_vault_authority(&vector_program);
    assert_eq!(
        (vector_vault.to_string().as_str(), vector_bump),
        ("GxxA9Cs9v5pAGVsaCe2jjDrtmieeBijcY4S5HHTY8Vq6", 255),
        "the vault authority of the vector program is the one the vectors hold"
    );
    assert_eq!(
        hex(&vault_handle(&vector_program)),
        "334121a65b47bd45c3f6381537d9180e98e445bc",
        "the vault handle of the vector program is the one the vectors hold"
    );
    assert_eq!(
        hex(&SIDIORA_ASSET_ID),
        "21f7b20a555199fa73a238b1a91fd0f549068fee",
        "Sidiora's asset id is the pointer Paxeer maps to its denom"
    );
    assert_eq!(
        SIDIORA_MINT.to_string(),
        "5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump",
        "Sidiora's asset id is bound to Sidiora's mint on Solana"
    );
    assert_eq!(
        paxeer_address(&recipient(SIDIORA_ASSET_ID)).expect("a padded 20-byte address"),
        SIDIORA_ASSET_ID
    );
    let mut high = recipient(attestor(7));
    high[0] = 1;
    assert!(
        paxeer_address(&high).is_err(),
        "a recipient with a non-zero high half is not a Paxeer address"
    );
    assert!(
        paxeer_address(&[0_u8; 32]).is_err(),
        "the zero address is not a Paxeer address"
    );
}

#[tokio::test]
async fn initialisation_records_the_owner_and_refuses_a_second_one() {
    let mut harness = Harness::start().await;
    let payer = harness.payer();
    let owner = Keypair::new();

    let early = harness.propose_owner(&payer.pubkey(), &owner.pubkey());
    harness
        .refuse(
            &[early],
            &[],
            BridgeError::NotInitialised,
            "an owner instruction before initialisation",
        )
        .await;

    let zero = harness.initialise(&payer.pubkey(), &Pubkey::default());
    harness
        .refuse(&[zero], &[], BridgeError::Authority, "a zero owner")
        .await;

    let initialise = harness.initialise(&payer.pubkey(), &owner.pubkey());
    harness.accept(std::slice::from_ref(&initialise), &[]).await;

    let config = harness.config_record().await;
    assert_eq!(config.owner, owner.pubkey());
    assert_eq!(config.pending_owner, Pubkey::default());
    assert!(!config.paused);
    assert_eq!(config.threshold, 0);
    assert!(config.attestors.is_empty());
    assert_eq!(config.deposit_nonce, 0);
    assert_eq!(config.config_bump, find_config_address(&harness.program).1);
    assert_eq!(config.vault_bump, find_vault_authority(&harness.program).1);

    harness
        .refuse(
            &[initialise],
            &[],
            BridgeError::Conflict,
            "a second initialisation",
        )
        .await;
}

#[tokio::test]
async fn only_the_upgrade_authority_initialises() {
    let mut harness = Harness::start().await;
    let payer = harness.payer();
    let owner = Keypair::new();

    let stranger = Keypair::new();
    let lamports = harness.rent_exempt(CONFIG_BYTES).await * 2;
    harness.fund(&stranger.pubkey(), lamports).await;
    let by_stranger = harness.initialise(&stranger.pubkey(), &stranger.pubkey());
    harness
        .refuse(
            &[by_stranger],
            &[&stranger],
            BridgeError::Authority,
            "an initialisation by a key that is not the upgrade authority",
        )
        .await;
    assert_eq!(harness.data(harness.config).await, None);

    let mut elsewhere = harness.initialise(&payer.pubkey(), &owner.pubkey());
    elsewhere.accounts[3].pubkey = harness.config;
    harness
        .refuse(
            &[elsewhere],
            &[],
            BridgeError::Pda,
            "an initialisation naming another account as the program data",
        )
        .await;

    harness.set_program_data(Some(payer.pubkey()), system_program::id());
    let foreign = harness.initialise(&payer.pubkey(), &owner.pubkey());
    harness
        .refuse(
            &[foreign],
            &[],
            BridgeError::Account,
            "an initialisation against program data the loader does not own",
        )
        .await;

    harness.set_program_data(None, bpf_loader_upgradeable::id());
    let immutable = harness.initialise(&payer.pubkey(), &owner.pubkey());
    harness
        .refuse(
            &[immutable],
            &[],
            BridgeError::Authority,
            "an initialisation of a program with no upgrade authority",
        )
        .await;
    assert_eq!(harness.data(harness.config).await, None);

    // A stranger funds the config's address first; the initialisation still
    // lands there.
    let config = harness.config;
    harness.fund(&config, 1_000_000).await;
    assert_eq!(harness.data(config).await, Some(Vec::new()));
    harness.set_program_data(Some(payer.pubkey()), bpf_loader_upgradeable::id());
    let initialise = harness.initialise(&payer.pubkey(), &owner.pubkey());
    harness.accept(&[initialise], &[]).await;
    assert_eq!(harness.config_record().await.owner, owner.pubkey());
}

#[tokio::test]
async fn ownership_moves_only_in_two_steps() {
    let mut harness = Harness::start().await;
    let payer = harness.payer();
    let owner = Keypair::new();
    let successor = Keypair::new();
    let stranger = Keypair::new();

    let initialise = harness.initialise(&payer.pubkey(), &owner.pubkey());
    harness.accept(&[initialise], &[]).await;

    let by_stranger = harness.propose_owner(&stranger.pubkey(), &successor.pubkey());
    harness
        .refuse(
            &[by_stranger],
            &[&stranger],
            BridgeError::Authority,
            "a proposal from an account that is not the owner",
        )
        .await;

    let to_nobody = harness.propose_owner(&owner.pubkey(), &Pubkey::default());
    harness
        .refuse(
            &[to_nobody],
            &[&owner],
            BridgeError::Authority,
            "a proposal naming the zero address",
        )
        .await;

    let premature = harness.accept_owner(&successor.pubkey());
    harness
        .refuse(
            &[premature],
            &[&successor],
            BridgeError::Authority,
            "an acceptance with no proposal outstanding",
        )
        .await;

    let propose = harness.propose_owner(&owner.pubkey(), &successor.pubkey());
    harness.accept(&[propose], &[&owner]).await;
    let config = harness.config_record().await;
    assert_eq!(
        config.owner,
        owner.pubkey(),
        "a proposal alone moves nothing"
    );
    assert_eq!(config.pending_owner, successor.pubkey());

    let by_wrong_account = harness.accept_owner(&stranger.pubkey());
    harness
        .refuse(
            &[by_wrong_account],
            &[&stranger],
            BridgeError::Authority,
            "an acceptance by an account the owner did not name",
        )
        .await;

    let accept = harness.accept_owner(&successor.pubkey());
    harness
        .accept(std::slice::from_ref(&accept), &[&successor])
        .await;
    let config = harness.config_record().await;
    assert_eq!(config.owner, successor.pubkey());
    assert_eq!(
        config.pending_owner,
        Pubkey::default(),
        "accepting clears the proposal"
    );

    let by_former_owner = harness.propose_owner(&owner.pubkey(), &stranger.pubkey());
    harness
        .refuse(
            &[by_former_owner],
            &[&owner],
            BridgeError::Authority,
            "a proposal from the former owner",
        )
        .await;
    harness
        .refuse(
            &[accept],
            &[&successor],
            BridgeError::Authority,
            "a replayed acceptance",
        )
        .await;
}

#[tokio::test]
async fn the_attestor_set_is_accepted_only_ascending_with_a_valid_threshold() {
    let mut harness = Harness::start().await;
    let payer = harness.payer();
    let stranger = Keypair::new();
    let initialise = harness.initialise(&payer.pubkey(), &payer.pubkey());
    harness.accept(&[initialise], &[]).await;
    let owner = payer.pubkey();

    let empty = harness.set_attestors(&owner, &[], 1);
    harness
        .refuse(
            &[empty],
            &[],
            BridgeError::Attestors,
            "an empty attestor set",
        )
        .await;

    let too_many: Vec<[u8; HANDLE_BYTES]> = (0..=MAX_ATTESTORS)
        .map(|index| {
            let mut entry = [0_u8; HANDLE_BYTES];
            entry[..2].copy_from_slice(&((index + 1) as u16).to_be_bytes());
            entry
        })
        .collect();
    let overflowing = harness.set_attestors(&owner, &too_many, 1);
    harness
        .refuse(
            &[overflowing],
            &[],
            BridgeError::Attestors,
            "an attestor set above the recorded maximum",
        )
        .await;

    let with_zero = harness.set_attestors(&owner, &[attestor(0), attestor(2)], 1);
    harness
        .refuse(
            &[with_zero],
            &[],
            BridgeError::Attestors,
            "the zero address as an attestor",
        )
        .await;

    let descending = harness.set_attestors(&owner, &[attestor(3), attestor(1)], 1);
    harness
        .refuse(
            &[descending],
            &[],
            BridgeError::Attestors,
            "a descending attestor set",
        )
        .await;

    let repeated = harness.set_attestors(&owner, &[attestor(1), attestor(1)], 1);
    harness
        .refuse(
            &[repeated],
            &[],
            BridgeError::Attestors,
            "a repeated attestor",
        )
        .await;

    let no_threshold = harness.set_attestors(&owner, &[attestor(1), attestor(2)], 0);
    harness
        .refuse(
            &[no_threshold],
            &[],
            BridgeError::Attestors,
            "a threshold of zero",
        )
        .await;

    let above_count = harness.set_attestors(&owner, &[attestor(1), attestor(2)], 3);
    harness
        .refuse(
            &[above_count],
            &[],
            BridgeError::Attestors,
            "a threshold above the attestor count",
        )
        .await;

    let by_stranger = harness.set_attestors(&stranger.pubkey(), &[attestor(1)], 1);
    harness
        .refuse(
            &[by_stranger],
            &[&stranger],
            BridgeError::Authority,
            "an attestor set from an account that is not the owner",
        )
        .await;

    let set = [attestor(1), attestor(2), attestor(9)];
    let accepted = harness.set_attestors(&owner, &set, 2);
    harness.accept(&[accepted], &[]).await;
    let config = harness.config_record().await;
    assert_eq!(config.attestors, set.to_vec());
    assert_eq!(config.threshold, 2);
}

#[tokio::test]
async fn asset_registration_derives_the_id_or_records_the_one_the_owner_names() {
    let mut harness = Harness::start().await;
    let payer = harness.payer();
    let owner = payer.pubkey();
    let stranger = Keypair::new();
    harness.fund(&stranger.pubkey(), 1_000_000_000).await;
    let initialise = harness.initialise(&payer.pubkey(), &owner);
    harness.accept(&[initialise], &[]).await;

    let wrapped_sol = harness.create_mint(&owner, 9).await;
    let named = harness.create_mint(&owner, 6).await;

    let no_cap = harness.register_asset(&owner, &wrapped_sol, None, 0, 1_000);
    harness
        .refuse(
            &[no_cap],
            &[],
            BridgeError::Cap,
            "a registration with a zero per-transaction cap",
        )
        .await;
    let no_total = harness.register_asset(&owner, &wrapped_sol, None, 1_000, 0);
    harness
        .refuse(
            &[no_total],
            &[],
            BridgeError::Cap,
            "a registration with a zero total cap",
        )
        .await;
    let inverted = harness.register_asset(&owner, &wrapped_sol, None, 2_000, 1_000);
    harness
        .refuse(
            &[inverted],
            &[],
            BridgeError::Cap,
            "a per-transaction cap above the total cap",
        )
        .await;
    let zero_id = harness.register_asset(&owner, &wrapped_sol, Some([0; HANDLE_BYTES]), 1, 1);
    harness
        .refuse(
            &[zero_id],
            &[],
            BridgeError::Asset,
            "the zero address as an explicit asset id",
        )
        .await;
    let not_a_mint = harness.register_asset(&owner, &harness.config, None, 1, 1);
    harness
        .refuse(
            &[not_a_mint],
            &[],
            BridgeError::Account,
            "a registration of an account the SPL token program does not own",
        )
        .await;
    let by_stranger = harness.register_asset(&stranger.pubkey(), &wrapped_sol, None, 1, 1);
    harness
        .refuse(
            &[by_stranger],
            &[&stranger],
            BridgeError::Authority,
            "a registration from an account that is not the owner",
        )
        .await;

    // A stranger funds the asset's address first; the registration still
    // lands there.
    let asset = find_asset_address(&harness.program, &wrapped_sol).0;
    harness.fund(&asset, 1_000_000).await;
    let derived = harness.register_asset(&owner, &wrapped_sol, None, 500, 5_000);
    harness.accept(std::slice::from_ref(&derived), &[]).await;
    let record = harness.asset_record(&wrapped_sol).await;
    assert_eq!(record.mint, wrapped_sol);
    assert_eq!(
        record.asset_id,
        pubkey_handle(&wrapped_sol),
        "an asset id defaults to the handle of the mint"
    );
    assert_eq!(record.decimals, 9, "the decimals come from the mint itself");
    assert!(record.enabled);
    assert_eq!(record.per_tx_cap, 500);
    assert_eq!(record.total_cap, 5_000);
    assert_eq!(record.outstanding, 0);
    assert_eq!(
        record.bump,
        find_asset_address(&harness.program, &wrapped_sol).1
    );

    harness
        .refuse(
            &[derived],
            &[],
            BridgeError::Conflict,
            "a second registration of the same mint",
        )
        .await;

    let explicit = harness.register_asset(&owner, &named, Some(NAMED_ASSET_ID), 100, 1_000);
    harness.accept(&[explicit], &[]).await;
    let record = harness.asset_record(&named).await;
    assert_eq!(
        record.asset_id, NAMED_ASSET_ID,
        "an explicit asset id is recorded as the owner named it"
    );
    assert_eq!(record.decimals, 6);

    let unregistered = harness.create_mint(&owner, 6).await;
    let missing = harness.set_cap(&owner, &unregistered, 1, 2, true);
    harness
        .refuse(
            &[missing],
            &[],
            BridgeError::Asset,
            "a cap on a mint that was never registered",
        )
        .await;
    let by_stranger = harness.set_cap(&stranger.pubkey(), &named, 1, 2, true);
    harness
        .refuse(
            &[by_stranger],
            &[&stranger],
            BridgeError::Authority,
            "a cap from an account that is not the owner",
        )
        .await;
    let zero = harness.set_cap(&owner, &named, 0, 2, true);
    harness
        .refuse(&[zero], &[], BridgeError::Cap, "a zero per-transaction cap")
        .await;

    let closed = harness.set_cap(&owner, &named, 50, 500, false);
    harness.accept(&[closed], &[]).await;
    let record = harness.asset_record(&named).await;
    assert!(!record.enabled);
    assert_eq!(record.per_tx_cap, 50);
    assert_eq!(record.total_cap, 500);
    assert_eq!(
        record.asset_id, NAMED_ASSET_ID,
        "moving a cap does not move the asset id"
    );
}

#[tokio::test]
async fn a_deposit_locks_the_tokens_and_records_a_receipt_per_nonce() {
    let mut harness = Harness::start().await;
    let payer = harness.payer();
    let owner = payer.pubkey();
    let initialise = harness.initialise(&payer.pubkey(), &owner);
    harness.accept(&[initialise], &[]).await;

    let mint = harness.create_mint(&owner, 6).await;
    let other_mint = harness.create_mint(&owner, 6).await;
    let source = harness.create_token_account(&mint, &owner).await;
    let vault = harness.vault;
    let vault_token = harness.create_token_account(&mint, &vault).await;
    harness.mint_to(&mint, &source, &payer, 1_000_000).await;
    let register = harness.register_asset(&owner, &mint, None, 500_000, 800_000);
    harness.accept(&[register], &[]).await;
    let target = recipient(attestor(0xab));

    let unregistered = harness.deposit(&owner, &other_mint, &source, &vault_token, 1, 1, target);
    harness
        .refuse(
            &[unregistered],
            &[],
            BridgeError::Asset,
            "a deposit of a mint that was never registered",
        )
        .await;
    let zero_amount = harness.deposit(&owner, &mint, &source, &vault_token, 1, 0, target);
    harness
        .refuse(
            &[zero_amount],
            &[],
            BridgeError::Bounds,
            "a deposit of nothing",
        )
        .await;
    let over_cap = harness.deposit(&owner, &mint, &source, &vault_token, 1, 500_001, target);
    harness
        .refuse(
            &[over_cap],
            &[],
            BridgeError::Cap,
            "a deposit above the per-transaction cap",
        )
        .await;
    let mut padded = target;
    padded[0] = 1;
    let high_bytes = harness.deposit(&owner, &mint, &source, &vault_token, 1, 1, padded);
    harness
        .refuse(
            &[high_bytes],
            &[],
            BridgeError::Recipient,
            "a recipient whose high twelve bytes are not zero",
        )
        .await;
    let zero_recipient = harness.deposit(&owner, &mint, &source, &vault_token, 1, 1, [0; 32]);
    harness
        .refuse(
            &[zero_recipient],
            &[],
            BridgeError::Recipient,
            "a deposit to the zero address",
        )
        .await;
    let stray_vault = harness.create_token_account(&mint, &owner).await;
    let wrong_custody = harness.deposit(&owner, &mint, &source, &stray_vault, 1, 1, target);
    harness
        .refuse(
            &[wrong_custody],
            &[],
            BridgeError::Account,
            "a deposit into a token account the vault authority does not own",
        )
        .await;

    let first = harness.deposit(&owner, &mint, &source, &vault_token, 1, 300_000, target);
    harness.accept(&[first], &[]).await;
    let receipt = harness.receipt_record(1).await;
    assert_eq!(receipt.nonce, 1);
    assert_eq!(receipt.mint, mint);
    assert_eq!(receipt.amount, 300_000);
    assert_eq!(receipt.paxeer_recipient, target);
    assert_eq!(receipt.depositor, owner);
    assert!(
        receipt.slot > 0,
        "the receipt records the slot it was written in"
    );
    let asset = harness.asset_record(&mint).await;
    assert_eq!(asset.outstanding, 300_000);
    assert_eq!(harness.config_record().await.deposit_nonce, 1);
    assert_eq!(harness.token_amount(vault_token).await, 300_000);
    assert_eq!(harness.token_amount(source).await, 700_000);
    let recorded =
        paxeer_address(&receipt.paxeer_recipient).expect("the receipt records a Paxeer address");
    assert_eq!(
        deposit_record(&receipt, &asset.asset_id, &recorded),
        format!(
            "PXBR/deposit/v1 nonce=1 mint={} asset={} amount=300000 recipient={} depositor={} slot={}",
            mint,
            hex(&pubkey_handle(&mint)),
            "ab".repeat(20),
            owner,
            receipt.slot
        ),
        "the deposit records what the relayer turns into an inbound attestation"
    );

    let replayed_receipt = harness.deposit(&owner, &mint, &source, &vault_token, 1, 1, target);
    harness
        .refuse(
            &[replayed_receipt],
            &[],
            BridgeError::Pda,
            "a deposit reusing a receipt a nonce already wrote",
        )
        .await;
    assert_eq!(
        harness.receipt_record(1).await,
        receipt,
        "a refused deposit leaves the earlier receipt exactly as it was"
    );

    let second = harness.deposit(&owner, &mint, &source, &vault_token, 2, 400_000, target);
    harness.accept(&[second], &[]).await;
    assert_eq!(harness.receipt_record(2).await.nonce, 2);
    assert_eq!(harness.config_record().await.deposit_nonce, 2);
    assert_eq!(harness.asset_record(&mint).await.outstanding, 700_000);
    assert_eq!(harness.token_amount(vault_token).await, 700_000);

    // The address of the next receipt is public, and anyone can send lamports to
    // it without this program's consent. A stranger doing that must not be able
    // to stop the bridge: the deposit still takes the nonce and still writes the
    // receipt over the address it pre-funded.
    let blocked = find_receipt_address(&harness.program, 3).0;
    harness.fund(&blocked, 1_000_000).await;
    assert_eq!(
        harness
            .data(blocked)
            .await
            .expect("the pre-funded address exists before the deposit")
            .len(),
        0,
        "a stranger can fund the address but cannot give it data"
    );
    let pre_funded = harness.deposit(&owner, &mint, &source, &vault_token, 3, 100_000, target);
    harness.accept(&[pre_funded], &[]).await;
    let third = harness.receipt_record(3).await;
    assert_eq!(third.nonce, 3);
    assert_eq!(third.mint, mint);
    assert_eq!(third.amount, 100_000);
    assert_eq!(third.paxeer_recipient, target);
    assert_eq!(third.depositor, owner);
    assert_eq!(harness.config_record().await.deposit_nonce, 3);
    assert_eq!(harness.asset_record(&mint).await.outstanding, 800_000);
    assert_eq!(harness.token_amount(vault_token).await, 800_000);

    let over_total = harness.deposit(&owner, &mint, &source, &vault_token, 4, 1, target);
    harness
        .refuse(
            &[over_total],
            &[],
            BridgeError::Cap,
            "a deposit taking custody above the total cap",
        )
        .await;

    let close = harness.set_cap(&owner, &mint, 500_000, 800_000, false);
    harness.accept(&[close], &[]).await;
    let while_closed = harness.deposit(&owner, &mint, &source, &vault_token, 4, 1, target);
    harness
        .refuse(
            &[while_closed],
            &[],
            BridgeError::Asset,
            "a deposit of a disabled asset",
        )
        .await;
}

#[tokio::test]
async fn the_pause_refuses_every_instruction_but_the_unpause() {
    let mut harness = Harness::start().await;
    let payer = harness.payer();
    let owner = payer.pubkey();
    let stranger = Keypair::new();
    let initialise = harness.initialise(&payer.pubkey(), &owner);
    harness.accept(&[initialise], &[]).await;

    let mint = harness.create_mint(&owner, 6).await;
    let spare = harness.create_mint(&owner, 6).await;
    let source = harness.create_token_account(&mint, &owner).await;
    let vault = harness.vault;
    let vault_token = harness.create_token_account(&mint, &vault).await;
    harness.mint_to(&mint, &source, &payer, 1_000).await;
    let register = harness.register_asset(&owner, &mint, None, 500, 1_000);
    harness.accept(&[register], &[]).await;
    let target = recipient(attestor(0x11));

    let by_stranger = harness.set_pause(&stranger.pubkey(), true);
    harness
        .refuse(
            &[by_stranger],
            &[&stranger],
            BridgeError::Authority,
            "a pause from an account that is not the owner",
        )
        .await;

    let pause = harness.set_pause(&owner, true);
    harness.accept(std::slice::from_ref(&pause), &[]).await;
    assert!(harness.config_record().await.paused);

    harness
        .refuse(&[pause], &[], BridgeError::Paused, "a second pause")
        .await;
    let deposit = harness.deposit(&owner, &mint, &source, &vault_token, 1, 100, target);
    harness
        .refuse(
            std::slice::from_ref(&deposit),
            &[],
            BridgeError::Paused,
            "a deposit while paused",
        )
        .await;
    let propose = harness.propose_owner(&owner, &stranger.pubkey());
    harness
        .refuse(
            &[propose],
            &[],
            BridgeError::Paused,
            "an ownership proposal while paused",
        )
        .await;
    let attestors = harness.set_attestors(&owner, &[attestor(1), attestor(2)], 2);
    harness
        .refuse(
            &[attestors],
            &[],
            BridgeError::Paused,
            "an attestor set while paused",
        )
        .await;
    let register_spare = harness.register_asset(&owner, &spare, None, 1, 2);
    harness
        .refuse(
            &[register_spare],
            &[],
            BridgeError::Paused,
            "a registration while paused",
        )
        .await;
    let cap = harness.set_cap(&owner, &mint, 10, 20, true);
    harness
        .refuse(&[cap], &[], BridgeError::Paused, "a cap while paused")
        .await;
    let registrant = Keypair::new();
    let own = pubkey_handle(&registrant.pubkey());
    let register_recipient = harness.register_recipient(
        &owner,
        &registrant.pubkey(),
        true,
        own,
        find_recipient_address(&harness.program, &own).0,
    );
    harness
        .refuse(
            &[register_recipient],
            &[&registrant],
            BridgeError::Paused,
            "a recipient registration while paused",
        )
        .await;

    let unpause = harness.set_pause(&owner, false);
    harness.accept(&[unpause], &[]).await;
    assert!(!harness.config_record().await.paused);
    harness.accept(&[deposit], &[]).await;
    assert_eq!(harness.token_amount(vault_token).await, 100);
    assert_eq!(harness.receipt_record(1).await.amount, 100);
}

#[tokio::test]
async fn the_sidiora_asset_id_binds_only_to_the_sidiora_mint() {
    let mut harness = Harness::start_with_sidiora_mint().await;
    let payer = harness.payer();
    let owner = payer.pubkey();
    let initialise = harness.initialise(&payer.pubkey(), &owner);
    harness.accept(&[initialise], &[]).await;
    let ordinary = harness.create_mint(&owner, 6).await;

    let borrowed = harness.register_asset(&owner, &ordinary, Some(SIDIORA_ASSET_ID), 100, 1_000);
    harness
        .refuse(
            &[borrowed],
            &[],
            BridgeError::Binding,
            "Sidiora's asset id on a mint that is not Sidiora's",
        )
        .await;
    let derived = harness.register_asset(&owner, &SIDIORA_MINT, None, 100, 1_000);
    harness
        .refuse(
            &[derived],
            &[],
            BridgeError::Binding,
            "Sidiora's mint under the handle of the mint rather than its fixed id",
        )
        .await;
    let renamed = harness.register_asset(&owner, &SIDIORA_MINT, Some(NAMED_ASSET_ID), 100, 1_000);
    harness
        .refuse(
            &[renamed],
            &[],
            BridgeError::Binding,
            "Sidiora's mint under an asset id Paxeer does not map to Sidiora",
        )
        .await;

    let bound = harness.register_asset(&owner, &SIDIORA_MINT, Some(SIDIORA_ASSET_ID), 100, 1_000);
    harness.accept(&[bound], &[]).await;
    let record = harness.asset_record(&SIDIORA_MINT).await;
    assert_eq!(record.mint, SIDIORA_MINT);
    assert_eq!(record.asset_id, SIDIORA_ASSET_ID);
    assert_eq!(record.decimals, 6, "the decimals come from Sidiora's mint");

    let plain = harness.register_asset(&owner, &ordinary, None, 100, 1_000);
    harness.accept(&[plain], &[]).await;
    assert_eq!(
        harness.asset_record(&ordinary).await.asset_id,
        pubkey_handle(&ordinary),
        "the binding leaves every other mint to its own derived id"
    );
}

#[tokio::test]
async fn a_solana_account_registers_the_key_its_handle_stands_for() {
    let mut harness = Harness::start().await;
    let payer = harness.payer();
    let owner = payer.pubkey();
    let registrant = Keypair::new();
    let key = registrant.pubkey();
    let own = pubkey_handle(&key);
    let record = find_recipient_address(&harness.program, &own).0;

    let before_initialise = harness.register_recipient(&owner, &key, true, own, record);
    harness
        .refuse(
            &[before_initialise],
            &[&registrant],
            BridgeError::NotInitialised,
            "a recipient registration before the bridge is initialised",
        )
        .await;
    let initialise = harness.initialise(&payer.pubkey(), &owner);
    harness.accept(&[initialise], &[]).await;

    let other = pubkey_handle(&Pubkey::new_unique());
    let foreign = harness.register_recipient(
        &owner,
        &key,
        true,
        other,
        find_recipient_address(&harness.program, &other).0,
    );
    harness
        .refuse(
            &[foreign],
            &[&registrant],
            BridgeError::Recipient,
            "a handle the registrant's own key does not hash to",
        )
        .await;
    let unsigned = harness.register_recipient(&owner, &key, false, own, record);
    harness
        .refuse(
            &[unsigned],
            &[],
            BridgeError::Authority,
            "a registration the registrant did not sign",
        )
        .await;
    let misplaced = harness.register_recipient(
        &owner,
        &key,
        true,
        own,
        find_recipient_address(&harness.program, &other).0,
    );
    harness
        .refuse(
            &[misplaced],
            &[&registrant],
            BridgeError::Pda,
            "a record at an address the handle does not seed",
        )
        .await;

    // A stranger funds the record's address first; the registration still
    // lands there, and the payer is not the registrant.
    harness.fund(&record, 1_000_000).await;
    let register = harness.register_recipient(&owner, &key, true, own, record);
    harness
        .accept(std::slice::from_ref(&register), &[&registrant])
        .await;
    assert_eq!(
        harness.recipient_record(&own).await,
        RecipientRecord { handle: own, key },
        "a release addressed to the handle resolves to the key it was derived from"
    );
    assert_eq!(
        recipient_record(&harness.recipient_record(&own).await),
        format!(
            "PXBR/register-recipient/v1 handle={} key={}",
            hex(&own),
            key
        ),
        "the registration logs the handle and the key"
    );
    harness
        .refuse(
            &[register],
            &[&registrant],
            BridgeError::Conflict,
            "a second registration of the same handle",
        )
        .await;
}
