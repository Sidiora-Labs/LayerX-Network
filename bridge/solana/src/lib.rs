//! The Paxeer X Network custody program for Solana.
//!
//! Custody is lock and release: a deposit locks SPL tokens under the
//! vault-authority PDA and records a receipt the relayer turns into an inbound
//! attestation, and a release pays them out again against the outbound
//! attestation every other destination of this bridge verifies. This crate
//! carries the custody core - ownership, the attestor set, the asset registry,
//! the caps, the pause, and the deposit with its receipt - and the release,
//! which has the native secp256k1 program verify the attestors' signatures and
//! consumes a nullifier per Paxeer burn.

use solana_program::account_info::AccountInfo;
use solana_program::entrypoint;
use solana_program::entrypoint::ProgramResult;
use solana_program::program::{invoke, invoke_signed};
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::sysvar::Sysvar;
use solana_sdk_ids::system_program;
use solana_system_interface::instruction as system_instruction;

pub mod admin;
pub mod attestation;
pub mod deposit;
pub mod identity;
pub mod recipient;
pub mod release;
pub mod state;

entrypoint!(process_instruction);

/// Every instruction to this program carries this prefix, so a payload built
/// for another program or another layout of this one cannot be mistaken for a
/// custody instruction.
pub const INSTRUCTION_MAGIC: &[u8; 4] = b"PXBR";
/// The instruction layout version the magic is paired with.
pub const INSTRUCTION_VERSION: u16 = 1;

pub const OP_INITIALISE: u8 = 1;
pub const OP_PROPOSE_OWNER: u8 = 2;
pub const OP_ACCEPT_OWNER: u8 = 3;
pub const OP_SET_ATTESTORS: u8 = 4;
pub const OP_REGISTER_ASSET: u8 = 5;
pub const OP_SET_CAP: u8 = 6;
pub const OP_SET_PAUSE: u8 = 7;
pub const OP_DEPOSIT: u8 = 8;
pub const OP_REGISTER_RECIPIENT: u8 = 9;
pub const OP_RELEASE: u8 = 10;

/// The refusals this program can make. Every value is stable: a client reads
/// `ProgramError::Custom(code)` and knows which rule refused it.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeError {
    /// The instruction payload is not a custody instruction of this version.
    Instruction = 1,
    /// The signer is not the account the instruction requires.
    Authority = 2,
    /// An account is not the program address its seeds derive.
    Pda = 3,
    /// An account already holds state this instruction would overwrite.
    Conflict = 4,
    /// The config PDA does not exist yet.
    NotInitialised = 5,
    /// The program is paused and this is not the owner's unpause.
    Paused = 6,
    /// A value is zero, too large, or would overflow.
    Bounds = 7,
    /// The attestor set is empty, too large, out of order, repeated, zero, or
    /// paired with a threshold outside one through the count.
    Attestors = 8,
    /// The mint is unregistered, disabled, or not the one the record names.
    Asset = 9,
    /// The amount exceeds the per-transaction cap or the total cap.
    Cap = 10,
    /// The Paxeer recipient is not a 20-byte address left-padded to 32 bytes,
    /// a Solana recipient names a handle its own key does not hash to, or a
    /// release names a token account its recipient does not own.
    Recipient = 11,
    /// A token account, mint, or program account is not the one required.
    Account = 12,
    /// A registration would pair Sidiora's fixed asset id with another mint,
    /// or Sidiora's mint with another asset id.
    Binding = 13,
    /// The release's nullifier already exists: this Paxeer burn was paid out.
    Replayed = 14,
    /// A release asks for more than this program holds in custody for the mint.
    Outstanding = 15,
    /// The instruction before the release is not the native secp256k1
    /// program's.
    AttestationMissing = 16,
    /// The secp256k1 instruction is short, carries an offset that does not
    /// resolve inside its own data, or carries a recovery id other than 0 or 1.
    AttestationMalformed = 17,
    /// A signature entry covers bytes other than the release's outbound
    /// preimage.
    AttestationMessage = 18,
    /// Fewer signature entries than the attestor threshold, or no attestor set.
    AttestationThreshold = 19,
    /// A signer is not a current attestor, or the signers are not in strictly
    /// ascending order.
    AttestationSigner = 20,
    /// A signature carries an s above half the secp256k1 order.
    AttestationMalleable = 21,
}

impl From<BridgeError> for ProgramError {
    fn from(value: BridgeError) -> Self {
        Self::Custom(value as u32)
    }
}

/// The program entrypoint: read the magic and version, then dispatch.
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    instruction: &[u8],
) -> ProgramResult {
    let mut reader = Reader::new(instruction);
    if reader.take(4)? != INSTRUCTION_MAGIC || reader.u16()? != INSTRUCTION_VERSION {
        return Err(BridgeError::Instruction.into());
    }
    match reader.u8()? {
        OP_INITIALISE => admin::initialise(program_id, accounts, &mut reader),
        OP_PROPOSE_OWNER => admin::propose_owner(program_id, accounts, &mut reader),
        OP_ACCEPT_OWNER => admin::accept_owner(program_id, accounts, &mut reader),
        OP_SET_ATTESTORS => admin::set_attestors(program_id, accounts, &mut reader),
        OP_REGISTER_ASSET => admin::register_asset(program_id, accounts, &mut reader),
        OP_SET_CAP => admin::set_cap(program_id, accounts, &mut reader),
        OP_SET_PAUSE => admin::set_pause(program_id, accounts, &mut reader),
        OP_DEPOSIT => deposit::deposit(program_id, accounts, &mut reader),
        OP_REGISTER_RECIPIENT => identity::register_recipient(program_id, accounts, &mut reader),
        OP_RELEASE => release::release(program_id, accounts, &mut reader),
        _ => Err(BridgeError::Instruction.into()),
    }
}

/// Create a program-owned account at `account`, which must be the address
/// `seeds` derive under `program_id`, paid for by `payer`.
///
/// Anyone can send lamports to a PDA of this program without its consent, and
/// every one of these addresses is public: refusing an address that merely
/// holds lamports would let a stranger block the next deposit, the
/// initialisation or an asset registration for good. So a system-owned address
/// with no data is created whether or not it already holds lamports - topped up
/// to the rent-exempt minimum, then allocated and assigned under the seeds. An
/// address that carries data, or that some program already owns, is still
/// refused, which is what keeps a second initialisation and a reused receipt
/// out.
pub fn create_pda<'a>(
    payer: &AccountInfo<'a>,
    account: &AccountInfo<'a>,
    system: &AccountInfo<'a>,
    program_id: &Pubkey,
    bytes: usize,
    seeds: &[&[u8]],
) -> ProgramResult {
    if system.key != &system_program::id() {
        return Err(BridgeError::Account.into());
    }
    if account.owner != &system_program::id() || !account.data_is_empty() {
        return Err(BridgeError::Conflict.into());
    }
    let lamports = Rent::get()?.minimum_balance(bytes);
    let space = u64::try_from(bytes).map_err(|_| BridgeError::Bounds)?;
    let held = account.lamports();
    if held == 0 {
        return invoke_signed(
            &system_instruction::create_account(
                payer.key,
                account.key,
                lamports,
                space,
                program_id,
            ),
            &[payer.clone(), account.clone(), system.clone()],
            &[seeds],
        );
    }
    let top_up = lamports.saturating_sub(held);
    if top_up != 0 {
        invoke(
            &system_instruction::transfer(payer.key, account.key, top_up),
            &[payer.clone(), account.clone(), system.clone()],
        )?;
    }
    invoke_signed(
        &system_instruction::allocate(account.key, space),
        &[account.clone(), system.clone()],
        &[seeds],
    )?;
    invoke_signed(
        &system_instruction::assign(account.key, program_id),
        &[account.clone(), system.clone()],
        &[seeds],
    )
}

/// Require an account that both signs and pays: it signs the transaction and
/// its lamports fund the accounts the instruction creates.
pub fn require_payer(account: &AccountInfo<'_>) -> ProgramResult {
    if account.is_signer && account.is_writable {
        Ok(())
    } else {
        Err(BridgeError::Authority.into())
    }
}

/// Require an account that signs without paying.
pub fn require_signer(account: &AccountInfo<'_>) -> ProgramResult {
    if account.is_signer {
        Ok(())
    } else {
        Err(BridgeError::Authority.into())
    }
}

/// Require an account the instruction writes to.
pub fn require_writable(account: &AccountInfo<'_>) -> ProgramResult {
    if account.is_writable {
        Ok(())
    } else {
        Err(BridgeError::Authority.into())
    }
}

/// A bounds-checked big-endian reader over an instruction payload. Every field
/// of every instruction is read through it, and [`Reader::finish`] refuses a
/// payload with bytes left over.
pub struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub fn take(&mut self, length: usize) -> Result<&'a [u8], ProgramError> {
        let end = self.offset.checked_add(length).ok_or(BridgeError::Bounds)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(BridgeError::Instruction)?;
        self.offset = end;
        Ok(value)
    }

    pub fn u8(&mut self) -> Result<u8, ProgramError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or(BridgeError::Instruction.into())
    }

    pub fn u16(&mut self) -> Result<u16, ProgramError> {
        self.take(2)?
            .try_into()
            .map(u16::from_be_bytes)
            .map_err(|_| BridgeError::Instruction.into())
    }

    pub fn u64(&mut self) -> Result<u64, ProgramError> {
        self.take(8)?
            .try_into()
            .map(u64::from_be_bytes)
            .map_err(|_| BridgeError::Instruction.into())
    }

    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], ProgramError> {
        self.take(N)?
            .try_into()
            .map_err(|_| BridgeError::Instruction.into())
    }

    pub fn pubkey(&mut self) -> Result<Pubkey, ProgramError> {
        self.array::<32>().map(Pubkey::new_from_array)
    }

    pub fn flag(&mut self) -> Result<bool, ProgramError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(BridgeError::Instruction.into()),
        }
    }

    pub fn finish(&self) -> ProgramResult {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(BridgeError::Instruction.into())
        }
    }
}
