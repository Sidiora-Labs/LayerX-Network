#![deny(unsafe_code)]

use solana_program::account_info::{next_account_info, AccountInfo};
use solana_program::entrypoint;
use solana_program::entrypoint::ProgramResult;
use solana_program::hash::{hash, hashv};
use solana_program::program::invoke_signed;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::Pubkey;
use solana_program::rent::Rent;
use solana_program::system_instruction;
use solana_program::system_program;
use solana_program::sysvar::Sysvar;

entrypoint!(process_instruction);

const INSTRUCTION_MAGIC: &[u8; 4] = b"LXMA";
const INSTRUCTION_VERSION: u16 = 2;
const MANIFEST_MAGIC: &[u8; 8] = b"LXMMAN02";
const CHUNK_MAGIC: &[u8; 8] = b"LXMCHK02";
const MANIFEST_BYTES: usize = 237;
const CHUNK_HEADER_BYTES: usize = 80;
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CHUNK_BYTES: usize = 720;
const MAX_CHUNKS: u32 = 524_288;

#[repr(u32)]
enum MirrorError {
    Instruction = 1,
    Authority = 2,
    Pda = 3,
    Conflict = 4,
    Order = 5,
    Bounds = 6,
    Incomplete = 7,
}

impl From<MirrorError> for ProgramError {
    fn from(value: MirrorError) -> Self {
        Self::Custom(value as u32)
    }
}

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    instruction: &[u8],
) -> ProgramResult {
    let mut reader = Reader::new(instruction);
    if reader.take(4)? != INSTRUCTION_MAGIC || reader.u16()? != INSTRUCTION_VERSION {
        return Err(MirrorError::Instruction.into());
    }
    match reader.u8()? {
        1 => initialize(program_id, accounts, &mut reader),
        2 => append(program_id, accounts, &mut reader),
        3 => finalize(program_id, accounts, &mut reader),
        _ => Err(MirrorError::Instruction.into()),
    }
}

fn initialize(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    reader: &mut Reader<'_>,
) -> ProgramResult {
    let commitment = reader.array()?;
    let network_id = reader.u32()?;
    let batch_number = reader.u64()?;
    let checkpoint_id = reader.array()?;
    let total_bytes = reader.u64()?;
    let total_chunks = reader.u32()?;
    let archive_digest = reader.array()?;
    let expected_chain = reader.array()?;
    reader.finish()?;
    if commitment == [0; 32]
        || network_id == 0
        || batch_number == 0
        || total_bytes == 0
        || total_bytes > MAX_ARCHIVE_BYTES
        || total_chunks == 0
        || total_chunks > MAX_CHUNKS
        || archive_digest == [0; 32]
        || expected_chain == [0; 32]
    {
        return Err(MirrorError::Bounds.into());
    }
    let mut accounts = accounts.iter();
    let payer = next_account_info(&mut accounts)?;
    let manifest = next_account_info(&mut accounts)?;
    let system = next_account_info(&mut accounts)?;
    require_payer(payer)?;
    if system.key != &system_program::id() || !manifest.is_writable {
        return Err(MirrorError::Authority.into());
    }
    let (expected, bump) =
        Pubkey::find_program_address(&[b"manifest", payer.key.as_ref(), &commitment], program_id);
    if manifest.key != &expected {
        return Err(MirrorError::Pda.into());
    }
    let mut canonical = [0_u8; MANIFEST_BYTES];
    canonical[..8].copy_from_slice(MANIFEST_MAGIC);
    canonical[8..40].copy_from_slice(&commitment);
    canonical[40..72].copy_from_slice(payer.key.as_ref());
    canonical[72..76].copy_from_slice(&network_id.to_be_bytes());
    canonical[76..84].copy_from_slice(&batch_number.to_be_bytes());
    canonical[84..116].copy_from_slice(&checkpoint_id);
    canonical[116..124].copy_from_slice(&total_bytes.to_be_bytes());
    canonical[124..128].copy_from_slice(&total_chunks.to_be_bytes());
    canonical[128..160].copy_from_slice(&archive_digest);
    canonical[160..192].copy_from_slice(&expected_chain);
    if manifest.owner == program_id {
        let existing = manifest.try_borrow_data()?;
        return if existing.as_ref() == canonical {
            Ok(())
        } else {
            Err(MirrorError::Conflict.into())
        };
    }
    if manifest.lamports() != 0 || !manifest.data_is_empty() {
        return Err(MirrorError::Conflict.into());
    }
    create_pda(
        payer,
        manifest,
        system,
        program_id,
        MANIFEST_BYTES,
        &[b"manifest", payer.key.as_ref(), &commitment, &[bump]],
    )?;
    manifest.try_borrow_mut_data()?.copy_from_slice(&canonical);
    Ok(())
}

fn append(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    reader: &mut Reader<'_>,
) -> ProgramResult {
    let commitment = reader.array()?;
    let index = reader.u32()?;
    let length = usize::from(reader.u16()?);
    let value = reader.take(length)?;
    let claimed_digest = reader.array()?;
    reader.finish()?;
    if length == 0 || length > MAX_CHUNK_BYTES || hash(value).to_bytes() != claimed_digest {
        return Err(MirrorError::Bounds.into());
    }
    let mut accounts = accounts.iter();
    let payer = next_account_info(&mut accounts)?;
    let manifest = next_account_info(&mut accounts)?;
    let chunk = next_account_info(&mut accounts)?;
    let system = next_account_info(&mut accounts)?;
    require_payer(payer)?;
    if system.key != &system_program::id()
        || manifest.owner != program_id
        || !manifest.is_writable
        || !chunk.is_writable
    {
        return Err(MirrorError::Authority.into());
    }
    let (manifest_key, _) =
        Pubkey::find_program_address(&[b"manifest", payer.key.as_ref(), &commitment], program_id);
    let index_bytes = index.to_be_bytes();
    let (chunk_key, bump) = Pubkey::find_program_address(
        &[b"chunk", payer.key.as_ref(), &commitment, &index_bytes],
        program_id,
    );
    if manifest.key != &manifest_key || chunk.key != &chunk_key {
        return Err(MirrorError::Pda.into());
    }
    let mut manifest_data = manifest.try_borrow_mut_data()?;
    validate_manifest(&manifest_data, commitment, payer.key)?;
    let total_bytes = read_u64(&manifest_data[116..124])?;
    let total_chunks = read_u32(&manifest_data[124..128])?;
    let received_bytes = read_u64(&manifest_data[224..232])?;
    let next_chunk = read_u32(&manifest_data[232..236])?;
    if manifest_data[236] != 0 {
        return Err(MirrorError::Conflict.into());
    }
    let mut canonical = Vec::with_capacity(CHUNK_HEADER_BYTES + length);
    canonical.extend_from_slice(CHUNK_MAGIC);
    canonical.extend_from_slice(&commitment);
    canonical.extend_from_slice(&index_bytes);
    canonical.extend_from_slice(&claimed_digest);
    canonical.extend_from_slice(
        &u32::try_from(length)
            .map_err(|_| MirrorError::Bounds)?
            .to_be_bytes(),
    );
    canonical.extend_from_slice(value);
    if index < next_chunk {
        if chunk.owner != program_id || chunk.try_borrow_data()?.as_ref() != canonical {
            return Err(MirrorError::Conflict.into());
        }
        return Ok(());
    }
    if index != next_chunk || index >= total_chunks {
        return Err(MirrorError::Order.into());
    }
    let next_bytes = received_bytes
        .checked_add(u64::try_from(length).map_err(|_| MirrorError::Bounds)?)
        .ok_or(MirrorError::Bounds)?;
    if next_bytes > total_bytes {
        return Err(MirrorError::Bounds.into());
    }
    if chunk.owner == program_id || chunk.lamports() != 0 || !chunk.data_is_empty() {
        return Err(MirrorError::Conflict.into());
    }
    create_pda(
        payer,
        chunk,
        system,
        program_id,
        canonical.len(),
        &[
            b"chunk",
            payer.key.as_ref(),
            &commitment,
            &index_bytes,
            &[bump],
        ],
    )?;
    chunk.try_borrow_mut_data()?.copy_from_slice(&canonical);
    let observed: [u8; 32] = manifest_data[192..224]
        .try_into()
        .map_err(|_| MirrorError::Instruction)?;
    let next_chain = hashv(&[
        &observed,
        &index_bytes,
        &claimed_digest,
        &u32::try_from(length)
            .map_err(|_| MirrorError::Bounds)?
            .to_be_bytes(),
    ]);
    manifest_data[192..224].copy_from_slice(next_chain.as_ref());
    manifest_data[224..232].copy_from_slice(&next_bytes.to_be_bytes());
    manifest_data[232..236].copy_from_slice(&(index + 1).to_be_bytes());
    Ok(())
}

fn finalize(
    program_id: &Pubkey,
    accounts: &[AccountInfo<'_>],
    reader: &mut Reader<'_>,
) -> ProgramResult {
    let commitment = reader.array()?;
    reader.finish()?;
    let mut accounts = accounts.iter();
    let payer = next_account_info(&mut accounts)?;
    let manifest = next_account_info(&mut accounts)?;
    require_payer(payer)?;
    if manifest.owner != program_id || !manifest.is_writable {
        return Err(MirrorError::Authority.into());
    }
    let (expected, _) =
        Pubkey::find_program_address(&[b"manifest", payer.key.as_ref(), &commitment], program_id);
    if manifest.key != &expected {
        return Err(MirrorError::Pda.into());
    }
    let mut data = manifest.try_borrow_mut_data()?;
    validate_manifest(&data, commitment, payer.key)?;
    if data[236] == 1 {
        return Ok(());
    }
    if read_u64(&data[116..124])? != read_u64(&data[224..232])?
        || read_u32(&data[124..128])? != read_u32(&data[232..236])?
        || data[160..192] != data[192..224]
    {
        return Err(MirrorError::Incomplete.into());
    }
    data[236] = 1;
    Ok(())
}

fn create_pda<'a>(
    payer: &AccountInfo<'a>,
    account: &AccountInfo<'a>,
    system: &AccountInfo<'a>,
    program_id: &Pubkey,
    bytes: usize,
    seeds: &[&[u8]],
) -> ProgramResult {
    let lamports = Rent::get()?.minimum_balance(bytes);
    let space = u64::try_from(bytes).map_err(|_| MirrorError::Bounds)?;
    invoke_signed(
        &system_instruction::create_account(payer.key, account.key, lamports, space, program_id),
        &[payer.clone(), account.clone(), system.clone()],
        &[seeds],
    )
}

fn require_payer(account: &AccountInfo<'_>) -> ProgramResult {
    if account.is_signer && account.is_writable {
        Ok(())
    } else {
        Err(MirrorError::Authority.into())
    }
}

fn validate_manifest(bytes: &[u8], commitment: [u8; 32], publisher: &Pubkey) -> ProgramResult {
    if bytes.len() == MANIFEST_BYTES
        && &bytes[..8] == MANIFEST_MAGIC
        && bytes[8..40] == commitment
        && bytes[40..72] == publisher.to_bytes()
    {
        Ok(())
    } else {
        Err(MirrorError::Conflict.into())
    }
}

fn read_u32(bytes: &[u8]) -> Result<u32, ProgramError> {
    bytes
        .try_into()
        .map(u32::from_be_bytes)
        .map_err(|_| MirrorError::Instruction.into())
}

fn read_u64(bytes: &[u8]) -> Result<u64, ProgramError> {
    bytes
        .try_into()
        .map(u64::from_be_bytes)
        .map_err(|_| MirrorError::Instruction.into())
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ProgramError> {
        let end = self.offset.checked_add(length).ok_or(MirrorError::Bounds)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(MirrorError::Instruction)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, ProgramError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or(MirrorError::Instruction.into())
    }

    fn u16(&mut self) -> Result<u16, ProgramError> {
        self.take(2)?
            .try_into()
            .map(u16::from_be_bytes)
            .map_err(|_| MirrorError::Instruction.into())
    }

    fn u32(&mut self) -> Result<u32, ProgramError> {
        self.take(4)?
            .try_into()
            .map(u32::from_be_bytes)
            .map_err(|_| MirrorError::Instruction.into())
    }

    fn u64(&mut self) -> Result<u64, ProgramError> {
        self.take(8)?
            .try_into()
            .map(u64::from_be_bytes)
            .map_err(|_| MirrorError::Instruction.into())
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ProgramError> {
        self.take(N)?
            .try_into()
            .map_err(|_| MirrorError::Instruction.into())
    }

    fn finish(self) -> ProgramResult {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(MirrorError::Instruction.into())
        }
    }
}
