//! The program's account layouts.
//!
//! Every account is a fixed-width big-endian record prefixed with its own magic
//! and layout version, hand-rolled with no serialisation framework, so the
//! bytes a Solana client reads are the bytes this file declares. A record that
//! is short, long, foreign, or of another version is refused rather than
//! reinterpreted.

use solana_program::account_info::AccountInfo;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;

use crate::identity::HANDLE_BYTES;
use crate::BridgeError;

/// The layout version every record in this file carries.
pub const LAYOUT_VERSION: u16 = 1;

pub const CONFIG_MAGIC: &[u8; 8] = b"PXBRCFG0";
pub const ASSET_MAGIC: &[u8; 8] = b"PXBRAST0";
pub const RECEIPT_MAGIC: &[u8; 8] = b"PXBRRCP0";
pub const RECIPIENT_MAGIC: &[u8; 8] = b"PXBRREC0";
pub const NULLIFIER_MAGIC: &[u8; 8] = b"PXBRNUL0";

pub const CONFIG_SEED: &[u8] = b"config";
pub const VAULT_SEED: &[u8] = b"vault-authority";
pub const ASSET_SEED: &[u8] = b"asset";
pub const RECEIPT_SEED: &[u8] = b"receipt";
pub const RECIPIENT_SEED: &[u8] = b"recipient";
pub const NULLIFIER_SEED: &[u8] = b"nullifier";

/// The most attestors a config record can hold.
pub const MAX_ATTESTORS: usize = 64;
/// Where the attestor table starts inside the config record.
pub const CONFIG_ATTESTORS_OFFSET: usize = 87;
/// The exact size of a config record.
pub const CONFIG_BYTES: usize = CONFIG_ATTESTORS_OFFSET + MAX_ATTESTORS * HANDLE_BYTES;
/// The exact size of an asset record.
pub const ASSET_BYTES: usize = 89;
/// The exact size of a deposit-receipt record.
pub const RECEIPT_BYTES: usize = 130;
/// The exact size of a recipient record.
pub const RECIPIENT_BYTES: usize = 62;
/// The exact size of a release-nullifier record.
pub const NULLIFIER_BYTES: usize = 130;

/// The config PDA: who owns the program, who may accept ownership next, whether
/// custody is paused, the shared attestor set with its threshold, the deposit
/// nonce every receipt is seeded by, and the bumps of the two program addresses
/// the program derives for itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub owner: Pubkey,
    pub pending_owner: Pubkey,
    pub paused: bool,
    pub threshold: u8,
    pub config_bump: u8,
    pub vault_bump: u8,
    pub deposit_nonce: u64,
    pub attestors: Vec<[u8; HANDLE_BYTES]>,
}

impl Config {
    /// Read the config record out of `account`, refusing an account this program
    /// does not own, an address the config seeds do not derive, and a record
    /// this layout does not describe.
    pub fn load(account: &AccountInfo<'_>, program_id: &Pubkey) -> Result<Self, ProgramError> {
        if account.owner != program_id {
            return Err(BridgeError::NotInitialised.into());
        }
        let config = Self::decode(&account.try_borrow_data()?)?;
        if account.key != &config.address(program_id)? {
            return Err(BridgeError::Pda.into());
        }
        Ok(config)
    }

    /// The config PDA the record's own bump derives.
    pub fn address(&self, program_id: &Pubkey) -> Result<Pubkey, ProgramError> {
        Pubkey::create_program_address(&[CONFIG_SEED, &[self.config_bump]], program_id)
            .map_err(|_| BridgeError::Pda.into())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProgramError> {
        if bytes.len() != CONFIG_BYTES
            || &bytes[..8] != CONFIG_MAGIC
            || read_u16(&bytes[8..10])? != LAYOUT_VERSION
        {
            return Err(BridgeError::Conflict.into());
        }
        let paused = read_flag(bytes[74])?;
        let count = usize::from(bytes[75]);
        let threshold = bytes[76];
        if count > MAX_ATTESTORS || usize::from(threshold) > count {
            return Err(BridgeError::Attestors.into());
        }
        let mut attestors = Vec::with_capacity(count);
        for index in 0..count {
            let start = CONFIG_ATTESTORS_OFFSET + index * HANDLE_BYTES;
            attestors.push(read_handle(&bytes[start..start + HANDLE_BYTES])?);
        }
        Ok(Self {
            owner: read_pubkey(&bytes[10..42])?,
            pending_owner: read_pubkey(&bytes[42..74])?,
            paused,
            threshold,
            config_bump: bytes[77],
            vault_bump: bytes[78],
            deposit_nonce: read_u64(&bytes[79..87])?,
            attestors,
        })
    }

    pub fn encode(&self, bytes: &mut [u8]) -> Result<(), ProgramError> {
        if bytes.len() != CONFIG_BYTES || self.attestors.len() > MAX_ATTESTORS {
            return Err(BridgeError::Conflict.into());
        }
        let count = u8::try_from(self.attestors.len()).map_err(|_| BridgeError::Attestors)?;
        bytes.fill(0);
        bytes[..8].copy_from_slice(CONFIG_MAGIC);
        bytes[8..10].copy_from_slice(&LAYOUT_VERSION.to_be_bytes());
        bytes[10..42].copy_from_slice(self.owner.as_ref());
        bytes[42..74].copy_from_slice(self.pending_owner.as_ref());
        bytes[74] = u8::from(self.paused);
        bytes[75] = count;
        bytes[76] = self.threshold;
        bytes[77] = self.config_bump;
        bytes[78] = self.vault_bump;
        bytes[79..87].copy_from_slice(&self.deposit_nonce.to_be_bytes());
        for (index, attestor) in self.attestors.iter().enumerate() {
            let start = CONFIG_ATTESTORS_OFFSET + index * HANDLE_BYTES;
            bytes[start..start + HANDLE_BYTES].copy_from_slice(attestor);
        }
        Ok(())
    }

    pub fn store(&self, account: &AccountInfo<'_>) -> Result<(), ProgramError> {
        self.encode(&mut account.try_borrow_mut_data()?)
    }

    /// Refuse anything but the owner's own signature.
    pub fn require_owner(&self, signer: &AccountInfo<'_>) -> Result<(), ProgramError> {
        if signer.is_signer && signer.key == &self.owner {
            Ok(())
        } else {
            Err(BridgeError::Authority.into())
        }
    }

    /// Refuse every instruction while custody is paused. The owner's unpause is
    /// the one instruction that does not call this.
    pub fn require_unpaused(&self) -> Result<(), ProgramError> {
        if self.paused {
            Err(BridgeError::Paused.into())
        } else {
            Ok(())
        }
    }
}

/// An asset PDA, one per registered mint: the mint it holds, the 20 bytes that
/// mint occupies in an attestation digest, its decimals, its caps, how much of
/// it this program currently holds in custody, and whether it is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub mint: Pubkey,
    pub asset_id: [u8; HANDLE_BYTES],
    pub decimals: u8,
    pub enabled: bool,
    pub bump: u8,
    pub per_tx_cap: u64,
    pub total_cap: u64,
    pub outstanding: u64,
}

impl Asset {
    /// Read the asset record out of `account`, refusing an account this program
    /// does not own, an address the mint's asset seeds do not derive, and a
    /// record this layout does not describe.
    pub fn load(account: &AccountInfo<'_>, program_id: &Pubkey) -> Result<Self, ProgramError> {
        if account.owner != program_id {
            return Err(BridgeError::Asset.into());
        }
        let asset = Self::decode(&account.try_borrow_data()?)?;
        if account.key != &asset.address(program_id)? {
            return Err(BridgeError::Pda.into());
        }
        Ok(asset)
    }

    /// The asset PDA the record's own mint and bump derive.
    pub fn address(&self, program_id: &Pubkey) -> Result<Pubkey, ProgramError> {
        Pubkey::create_program_address(&[ASSET_SEED, self.mint.as_ref(), &[self.bump]], program_id)
            .map_err(|_| BridgeError::Pda.into())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProgramError> {
        if bytes.len() != ASSET_BYTES
            || &bytes[..8] != ASSET_MAGIC
            || read_u16(&bytes[8..10])? != LAYOUT_VERSION
        {
            return Err(BridgeError::Asset.into());
        }
        Ok(Self {
            mint: read_pubkey(&bytes[10..42])?,
            asset_id: read_handle(&bytes[42..62])?,
            decimals: bytes[62],
            enabled: read_flag(bytes[63])?,
            bump: bytes[64],
            per_tx_cap: read_u64(&bytes[65..73])?,
            total_cap: read_u64(&bytes[73..81])?,
            outstanding: read_u64(&bytes[81..89])?,
        })
    }

    pub fn encode(&self, bytes: &mut [u8]) -> Result<(), ProgramError> {
        if bytes.len() != ASSET_BYTES {
            return Err(BridgeError::Asset.into());
        }
        bytes[..8].copy_from_slice(ASSET_MAGIC);
        bytes[8..10].copy_from_slice(&LAYOUT_VERSION.to_be_bytes());
        bytes[10..42].copy_from_slice(self.mint.as_ref());
        bytes[42..62].copy_from_slice(&self.asset_id);
        bytes[62] = self.decimals;
        bytes[63] = u8::from(self.enabled);
        bytes[64] = self.bump;
        bytes[65..73].copy_from_slice(&self.per_tx_cap.to_be_bytes());
        bytes[73..81].copy_from_slice(&self.total_cap.to_be_bytes());
        bytes[81..89].copy_from_slice(&self.outstanding.to_be_bytes());
        Ok(())
    }

    pub fn store(&self, account: &AccountInfo<'_>) -> Result<(), ProgramError> {
        self.encode(&mut account.try_borrow_mut_data()?)
    }

    /// Admit `amount` against both caps, returning the outstanding balance the
    /// deposit leaves behind.
    pub fn admit(&self, amount: u64) -> Result<u64, ProgramError> {
        if amount == 0 {
            return Err(BridgeError::Bounds.into());
        }
        if amount > self.per_tx_cap {
            return Err(BridgeError::Cap.into());
        }
        let outstanding = self
            .outstanding
            .checked_add(amount)
            .ok_or(BridgeError::Bounds)?;
        if outstanding > self.total_cap {
            return Err(BridgeError::Cap.into());
        }
        Ok(outstanding)
    }

    /// Admit a release of `amount` against the per-transaction cap and the
    /// balance held in custody, returning the outstanding balance the release
    /// leaves behind. Nothing leaves custody that a deposit did not bring in.
    pub fn withdraw(&self, amount: u64) -> Result<u64, ProgramError> {
        if amount == 0 {
            return Err(BridgeError::Bounds.into());
        }
        if amount > self.per_tx_cap {
            return Err(BridgeError::Cap.into());
        }
        self.outstanding
            .checked_sub(amount)
            .ok_or(BridgeError::Outstanding.into())
    }
}

/// A deposit-receipt PDA, one per deposit nonce. Its existence is the record: a
/// nonce cannot be reused, because creating its receipt a second time fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepositReceipt {
    pub nonce: u64,
    pub mint: Pubkey,
    pub amount: u64,
    pub paxeer_recipient: [u8; 32],
    pub depositor: Pubkey,
    pub slot: u64,
}

impl DepositReceipt {
    pub fn decode(bytes: &[u8]) -> Result<Self, ProgramError> {
        if bytes.len() != RECEIPT_BYTES
            || &bytes[..8] != RECEIPT_MAGIC
            || read_u16(&bytes[8..10])? != LAYOUT_VERSION
        {
            return Err(BridgeError::Conflict.into());
        }
        Ok(Self {
            nonce: read_u64(&bytes[10..18])?,
            mint: read_pubkey(&bytes[18..50])?,
            amount: read_u64(&bytes[50..58])?,
            paxeer_recipient: read_recipient(&bytes[58..90])?,
            depositor: read_pubkey(&bytes[90..122])?,
            slot: read_u64(&bytes[122..130])?,
        })
    }

    pub fn encode(&self, bytes: &mut [u8]) -> Result<(), ProgramError> {
        if bytes.len() != RECEIPT_BYTES {
            return Err(BridgeError::Conflict.into());
        }
        bytes[..8].copy_from_slice(RECEIPT_MAGIC);
        bytes[8..10].copy_from_slice(&LAYOUT_VERSION.to_be_bytes());
        bytes[10..18].copy_from_slice(&self.nonce.to_be_bytes());
        bytes[18..50].copy_from_slice(self.mint.as_ref());
        bytes[50..58].copy_from_slice(&self.amount.to_be_bytes());
        bytes[58..90].copy_from_slice(&self.paxeer_recipient);
        bytes[90..122].copy_from_slice(self.depositor.as_ref());
        bytes[122..130].copy_from_slice(&self.slot.to_be_bytes());
        Ok(())
    }

    pub fn store(&self, account: &AccountInfo<'_>) -> Result<(), ProgramError> {
        self.encode(&mut account.try_borrow_mut_data()?)
    }
}

/// The asset PDA of `mint` and its bump.
pub fn find_asset_address(program_id: &Pubkey, mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[ASSET_SEED, mint.as_ref()], program_id)
}

/// The recipient PDA: the 32-byte Solana key a 20-byte handle stands for,
/// seeded by the handle, so a release addressed to the handle can be paid to
/// the key the handle was derived from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipientRecord {
    pub handle: [u8; HANDLE_BYTES],
    pub key: Pubkey,
}

impl RecipientRecord {
    pub fn decode(bytes: &[u8]) -> Result<Self, ProgramError> {
        if bytes.len() != RECIPIENT_BYTES
            || &bytes[..8] != RECIPIENT_MAGIC
            || read_u16(&bytes[8..10])? != LAYOUT_VERSION
        {
            return Err(BridgeError::Conflict.into());
        }
        Ok(Self {
            handle: read_handle(&bytes[10..30])?,
            key: read_pubkey(&bytes[30..62])?,
        })
    }

    pub fn encode(&self, bytes: &mut [u8]) -> Result<(), ProgramError> {
        if bytes.len() != RECIPIENT_BYTES {
            return Err(BridgeError::Conflict.into());
        }
        bytes[..8].copy_from_slice(RECIPIENT_MAGIC);
        bytes[8..10].copy_from_slice(&LAYOUT_VERSION.to_be_bytes());
        bytes[10..30].copy_from_slice(&self.handle);
        bytes[30..62].copy_from_slice(self.key.as_ref());
        Ok(())
    }

    pub fn store(&self, account: &AccountInfo<'_>) -> Result<(), ProgramError> {
        self.encode(&mut account.try_borrow_mut_data()?)
    }
}

/// A release-nullifier PDA, one per Paxeer burn: seeded by the keccak256 of
/// the burn's paxeerTxHash and paxeerNonce, created by the release that pays it
/// out. Its existence is the record: a replayed release fails because creating
/// the same nullifier a second time is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NullifierRecord {
    pub paxeer_tx_hash: [u8; 32],
    pub paxeer_nonce: u64,
    pub mint: Pubkey,
    pub recipient: Pubkey,
    pub amount: u64,
    pub slot: u64,
}

impl NullifierRecord {
    pub fn decode(bytes: &[u8]) -> Result<Self, ProgramError> {
        if bytes.len() != NULLIFIER_BYTES
            || &bytes[..8] != NULLIFIER_MAGIC
            || read_u16(&bytes[8..10])? != LAYOUT_VERSION
        {
            return Err(BridgeError::Conflict.into());
        }
        Ok(Self {
            paxeer_tx_hash: read_recipient(&bytes[10..42])?,
            paxeer_nonce: read_u64(&bytes[42..50])?,
            mint: read_pubkey(&bytes[50..82])?,
            recipient: read_pubkey(&bytes[82..114])?,
            amount: read_u64(&bytes[114..122])?,
            slot: read_u64(&bytes[122..130])?,
        })
    }

    pub fn encode(&self, bytes: &mut [u8]) -> Result<(), ProgramError> {
        if bytes.len() != NULLIFIER_BYTES {
            return Err(BridgeError::Conflict.into());
        }
        bytes[..8].copy_from_slice(NULLIFIER_MAGIC);
        bytes[8..10].copy_from_slice(&LAYOUT_VERSION.to_be_bytes());
        bytes[10..42].copy_from_slice(&self.paxeer_tx_hash);
        bytes[42..50].copy_from_slice(&self.paxeer_nonce.to_be_bytes());
        bytes[50..82].copy_from_slice(self.mint.as_ref());
        bytes[82..114].copy_from_slice(self.recipient.as_ref());
        bytes[114..122].copy_from_slice(&self.amount.to_be_bytes());
        bytes[122..130].copy_from_slice(&self.slot.to_be_bytes());
        Ok(())
    }

    pub fn store(&self, account: &AccountInfo<'_>) -> Result<(), ProgramError> {
        self.encode(&mut account.try_borrow_mut_data()?)
    }
}

/// The nullifier PDA of a release whose nullifier hash is `nullifier` and its
/// bump.
pub fn find_nullifier_address(program_id: &Pubkey, nullifier: &[u8; 32]) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[NULLIFIER_SEED, nullifier], program_id)
}

/// The recipient PDA of `handle` and its bump.
pub fn find_recipient_address(program_id: &Pubkey, handle: &[u8; HANDLE_BYTES]) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[RECIPIENT_SEED, handle], program_id)
}

/// The receipt PDA of `nonce` and its bump.
pub fn find_receipt_address(program_id: &Pubkey, nonce: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[RECEIPT_SEED, &nonce.to_be_bytes()], program_id)
}

/// The config PDA and its bump.
pub fn find_config_address(program_id: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CONFIG_SEED], program_id)
}

fn read_flag(byte: u8) -> Result<bool, ProgramError> {
    match byte {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(BridgeError::Conflict.into()),
    }
}

fn read_u16(bytes: &[u8]) -> Result<u16, ProgramError> {
    bytes
        .try_into()
        .map(u16::from_be_bytes)
        .map_err(|_| BridgeError::Conflict.into())
}

fn read_u64(bytes: &[u8]) -> Result<u64, ProgramError> {
    bytes
        .try_into()
        .map(u64::from_be_bytes)
        .map_err(|_| BridgeError::Conflict.into())
}

fn read_pubkey(bytes: &[u8]) -> Result<Pubkey, ProgramError> {
    bytes
        .try_into()
        .map(Pubkey::new_from_array)
        .map_err(|_| BridgeError::Conflict.into())
}

fn read_handle(bytes: &[u8]) -> Result<[u8; HANDLE_BYTES], ProgramError> {
    bytes.try_into().map_err(|_| BridgeError::Conflict.into())
}

fn read_recipient(bytes: &[u8]) -> Result<[u8; 32], ProgramError> {
    bytes.try_into().map_err(|_| BridgeError::Conflict.into())
}
