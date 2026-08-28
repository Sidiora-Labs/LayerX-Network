//! A bounded deterministic scripting program for the LayerX Programs ABI.
//!
//! Scripts are submitted as canonical bytes to this ordinary program. The
//! complete script is validated before any storage or transfer effect is
//! staged. Execution uses fixed-size registers and host-owned storage only;
//! there is no allocator, indirect call, floating point, clock, entropy, or
//! authority source in the instruction set.

#![no_std]

#[cfg(test)]
extern crate std;

use layerx_program_sdk::{
    call, storage, transfer, AccountId, Amount, AssetId, CallResult, Field, Payment, ProgramError,
    Reason, StorageKey, StorageValue,
};

/// Frozen script magic.
pub const MAGIC: [u8; 4] = *b"LXSI";
/// Frozen instruction-set version.
pub const VERSION: u8 = 1;
/// Largest accepted canonical script.
pub const MAX_SCRIPT_BYTES: usize = 4_096;
/// Largest declared register file. Its storage is fixed in the program image.
pub const MAX_REGISTERS: usize = 16;
/// Largest statically expanded instruction count.
pub const MAX_STEPS: u32 = 4_096;
/// Largest repeat nesting depth.
pub const MAX_CONTROL_DEPTH: u8 = 4;
/// Exact canonical header width.
pub const HEADER_BYTES: usize = 10;

const OP_HALT: u8 = 0x00;
const OP_CONST: u8 = 0x01;
const OP_ADD: u8 = 0x02;
const OP_SUB: u8 = 0x03;
const OP_MUL: u8 = 0x04;
const OP_DIV: u8 = 0x05;
const OP_EQ: u8 = 0x06;
const OP_LT: u8 = 0x07;
const OP_LOAD: u8 = 0x08;
const OP_STORE: u8 = 0x09;
const OP_DELETE: u8 = 0x0a;
const OP_TRANSFER: u8 = 0x0b;
const OP_REPEAT: u8 = 0x0c;

/// One decoded instruction from the frozen version-one format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Instruction<'a> {
    /// Stop the current script successfully.
    Halt,
    /// Assign an exact signed integer to a register.
    Constant { destination: u8, value: i64 },
    /// Checked integer addition.
    Add { destination: u8, left: u8, right: u8 },
    /// Checked integer subtraction.
    Subtract { destination: u8, left: u8, right: u8 },
    /// Checked integer multiplication.
    Multiply { destination: u8, left: u8, right: u8 },
    /// Integer division, refusing zero and the signed overflow case.
    Divide { destination: u8, left: u8, right: u8 },
    /// Store one for equality and zero otherwise.
    Equal { destination: u8, left: u8, right: u8 },
    /// Store one for signed less-than and zero otherwise.
    LessThan { destination: u8, left: u8, right: u8 },
    /// Read one canonical big-endian i64 from principal-scoped storage.
    Load { destination: u8, key: &'a [u8] },
    /// Write one register as a canonical big-endian i64.
    Store { source: u8, key: &'a [u8] },
    /// Delete one key from principal-scoped storage.
    Delete { key: &'a [u8] },
    /// Stage an ordinary capability-checked 402LXP transfer.
    Transfer { amount: u8, asset: [u8; 32], recipient: [u8; 32] },
    /// Execute a canonical body an exact positive, statically bounded count.
    Repeat { count: u16, body: &'a [u8] },
}

/// A fully validated borrowed script.
#[derive(Clone, Copy)]
pub struct Interpreter<'a> {
    code: &'a [u8],
    registers: u8,
    maximum_steps: u32,
}

#[derive(Clone, Copy)]
struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self { Self { bytes, offset: 0 } }
    fn take(&mut self, length: usize) -> Result<&'a [u8], ProgramError> {
        let end = self.offset.checked_add(length).ok_or_else(bounds)?;
        let value = self.bytes.get(self.offset..end).ok_or_else(malformed)?;
        self.offset = end;
        Ok(value)
    }
    fn byte(&mut self) -> Result<u8, ProgramError> { Ok(self.take(1)?[0]) }
    fn u16(&mut self) -> Result<u16, ProgramError> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().map_err(|_| malformed())?))
    }
    fn i64(&mut self) -> Result<i64, ProgramError> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into().map_err(|_| malformed())?))
    }
    const fn finished(self) -> bool { self.offset == self.bytes.len() }
}

const fn malformed() -> ProgramError { ProgramError::value(Field::CallInput, Reason::Malformed) }
const fn bounds() -> ProgramError { ProgramError::value(Field::CallInput, Reason::TooLarge) }
const fn arithmetic(reason: Reason) -> ProgramError { ProgramError::value(Field::CallInput, reason) }

fn register(index: u8, registers: u8) -> Result<u8, ProgramError> {
    if index < registers { Ok(index) } else { Err(malformed()) }
}

fn key<'a>(cursor: &mut Cursor<'a>) -> Result<&'a [u8], ProgramError> {
    let length = usize::from(cursor.byte()?);
    let bytes = cursor.take(length)?;
    StorageKey::new(bytes)?;
    Ok(bytes)
}

fn decode<'a>(cursor: &mut Cursor<'a>, registers: u8) -> Result<Instruction<'a>, ProgramError> {
    match cursor.byte()? {
        OP_HALT => Ok(Instruction::Halt),
        OP_CONST => Ok(Instruction::Constant {
            destination: register(cursor.byte()?, registers)?,
            value: cursor.i64()?,
        }),
        opcode @ (OP_ADD | OP_SUB | OP_MUL | OP_DIV | OP_EQ | OP_LT) => {
            let destination = register(cursor.byte()?, registers)?;
            let left = register(cursor.byte()?, registers)?;
            let right = register(cursor.byte()?, registers)?;
            Ok(match opcode {
                OP_ADD => Instruction::Add { destination, left, right },
                OP_SUB => Instruction::Subtract { destination, left, right },
                OP_MUL => Instruction::Multiply { destination, left, right },
                OP_DIV => Instruction::Divide { destination, left, right },
                OP_EQ => Instruction::Equal { destination, left, right },
                _ => Instruction::LessThan { destination, left, right },
            })
        }
        OP_LOAD => Ok(Instruction::Load {
            destination: register(cursor.byte()?, registers)?,
            key: key(cursor)?,
        }),
        OP_STORE => Ok(Instruction::Store {
            source: register(cursor.byte()?, registers)?,
            key: key(cursor)?,
        }),
        OP_DELETE => Ok(Instruction::Delete { key: key(cursor)? }),
        OP_TRANSFER => {
            let amount = register(cursor.byte()?, registers)?;
            let asset = cursor.take(32)?.try_into().map_err(|_| malformed())?;
            let recipient = cursor.take(32)?.try_into().map_err(|_| malformed())?;
            AssetId::new(asset)?;
            AccountId::new(recipient)?;
            Ok(Instruction::Transfer { amount, asset, recipient })
        }
        OP_REPEAT => {
            let count = cursor.u16()?;
            let body_length = usize::from(cursor.u16()?);
            if count == 0 || body_length == 0 { return Err(malformed()); }
            Ok(Instruction::Repeat { count, body: cursor.take(body_length)? })
        }
        _ => Err(malformed()),
    }
}

fn validate_block(
    code: &[u8],
    registers: u8,
    depth: u8,
    remaining: &mut u32,
) -> Result<(), ProgramError> {
    if depth > MAX_CONTROL_DEPTH { return Err(bounds()); }
    let mut cursor = Cursor::new(code);
    while !cursor.finished() {
        let instruction = decode(&mut cursor, registers)?;
        *remaining = remaining.checked_sub(1).ok_or_else(bounds)?;
        if let Instruction::Repeat { count, body } = instruction {
            let mut body_budget = MAX_STEPS;
            validate_block(body, registers, depth.checked_add(1).ok_or_else(bounds)?, &mut body_budget)?;
            let body_steps = MAX_STEPS.checked_sub(body_budget).ok_or_else(bounds)?;
            let expanded = body_steps.checked_mul(u32::from(count)).ok_or_else(bounds)?;
            *remaining = remaining.checked_sub(expanded).ok_or_else(bounds)?;
        }
    }
    Ok(())
}

impl<'a> Interpreter<'a> {
    /// Validates the complete canonical script before execution can begin.
    ///
    /// # Errors
    ///
    /// Refuses unknown/truncated/trailing encodings, invalid registers or
    /// keys, zero repeats, excessive nesting, and any expanded step count
    /// beyond both the script declaration and the protocol ceiling.
    pub fn validate(script: &'a [u8]) -> Result<Self, ProgramError> {
        if script.len() > MAX_SCRIPT_BYTES || script.len() < HEADER_BYTES { return Err(bounds()); }
        let mut cursor = Cursor::new(script);
        if cursor.take(4)? != MAGIC || cursor.byte()? != VERSION { return Err(malformed()); }
        let registers = cursor.byte()?;
        if registers == 0 || usize::from(registers) > MAX_REGISTERS { return Err(bounds()); }
        let maximum_steps = u32::from(cursor.u16()?);
        if maximum_steps == 0 || maximum_steps > MAX_STEPS { return Err(bounds()); }
        let code_length = usize::from(cursor.u16()?);
        let code = cursor.take(code_length)?;
        if !cursor.finished() || code.is_empty() { return Err(malformed()); }
        let mut remaining = maximum_steps;
        validate_block(code, registers, 0, &mut remaining)?;
        Ok(Self { code, registers, maximum_steps })
    }

    /// Returns the statically declared execution ceiling.
    #[must_use]
    pub const fn maximum_steps(self) -> u32 { self.maximum_steps }

    /// Returns the exact fixed register count.
    #[must_use]
    pub const fn register_count(self) -> u8 { self.registers }
}

trait Host {
    fn load(&mut self, key: &[u8]) -> Result<Option<i64>, ProgramError>;
    fn store(&mut self, key: &[u8], value: i64) -> Result<(), ProgramError>;
    fn delete(&mut self, key: &[u8]) -> Result<(), ProgramError>;
    fn transfer(&mut self, asset: [u8; 32], recipient: [u8; 32], amount: i64) -> Result<(), ProgramError>;
}

enum Flow { Continue, Halt }

fn execute_block(
    code: &[u8],
    register_count: u8,
    registers: &mut [i64; MAX_REGISTERS],
    steps: &mut u32,
    maximum_steps: u32,
    host: &mut impl Host,
) -> Result<Flow, ProgramError> {
    let mut cursor = Cursor::new(code);
    while !cursor.finished() {
        *steps = steps.checked_add(1).ok_or_else(bounds)?;
        if *steps > maximum_steps { return Err(bounds()); }
        match decode(&mut cursor, register_count)? {
            Instruction::Halt => return Ok(Flow::Halt),
            Instruction::Constant { destination, value } => registers[usize::from(destination)] = value,
            Instruction::Add { destination, left, right } => {
                registers[usize::from(destination)] = registers[usize::from(left)]
                    .checked_add(registers[usize::from(right)]).ok_or_else(|| arithmetic(Reason::Overflow))?;
            }
            Instruction::Subtract { destination, left, right } => {
                registers[usize::from(destination)] = registers[usize::from(left)]
                    .checked_sub(registers[usize::from(right)]).ok_or_else(|| arithmetic(Reason::Overflow))?;
            }
            Instruction::Multiply { destination, left, right } => {
                registers[usize::from(destination)] = registers[usize::from(left)]
                    .checked_mul(registers[usize::from(right)]).ok_or_else(|| arithmetic(Reason::Overflow))?;
            }
            Instruction::Divide { destination, left, right } => {
                registers[usize::from(destination)] = registers[usize::from(left)]
                    .checked_div(registers[usize::from(right)]).ok_or_else(|| arithmetic(Reason::Malformed))?;
            }
            Instruction::Equal { destination, left, right } => {
                registers[usize::from(destination)] = i64::from(registers[usize::from(left)] == registers[usize::from(right)]);
            }
            Instruction::LessThan { destination, left, right } => {
                registers[usize::from(destination)] = i64::from(registers[usize::from(left)] < registers[usize::from(right)]);
            }
            Instruction::Load { destination, key } => {
                registers[usize::from(destination)] = host.load(key)?.unwrap_or(0);
            }
            Instruction::Store { source, key } => host.store(key, registers[usize::from(source)])?,
            Instruction::Delete { key } => host.delete(key)?,
            Instruction::Transfer { amount, asset, recipient } => {
                host.transfer(asset, recipient, registers[usize::from(amount)])?;
            }
            Instruction::Repeat { count, body } => {
                for _ in 0..count {
                    if let Flow::Halt = execute_block(body, register_count, registers, steps, maximum_steps, host)? {
                        return Ok(Flow::Halt);
                    }
                }
            }
        }
    }
    Ok(Flow::Continue)
}

#[cfg(target_arch = "wasm32")]
struct AbiHost;

#[cfg(target_arch = "wasm32")]
impl Host for AbiHost {
    fn load(&mut self, key: &[u8]) -> Result<Option<i64>, ProgramError> {
        let mut value = [0_u8; 8];
        match storage::read(StorageKey::new(key)?, &mut value)? {
            None => Ok(None),
            Some(8) => Ok(Some(i64::from_be_bytes(value))),
            Some(_) => Err(ProgramError::value(Field::StorageValue, Reason::Malformed)),
        }
    }
    fn store(&mut self, key: &[u8], value: i64) -> Result<(), ProgramError> {
        storage::write(StorageKey::new(key)?, StorageValue::new(&value.to_be_bytes())?)
    }
    fn delete(&mut self, key: &[u8]) -> Result<(), ProgramError> { storage::delete(StorageKey::new(key)?) }
    fn transfer(&mut self, asset: [u8; 32], recipient: [u8; 32], amount: i64) -> Result<(), ProgramError> {
        let amount = Amount::from_i64(amount)?;
        transfer::pay(Payment::new(AssetId::new(asset)?, AccountId::new(recipient)?, amount)?)
    }
}

#[cfg(target_arch = "wasm32")]
fn invoke(input: &[u8]) -> Result<CallResult, ProgramError> {
    let interpreter = Interpreter::validate(input)?;
    let mut registers = [0_i64; MAX_REGISTERS];
    let mut steps = 0;
    execute_block(
        interpreter.code,
        interpreter.registers,
        &mut registers,
        &mut steps,
        interpreter.maximum_steps,
        &mut AbiHost,
    )?;
    call::publish_response(CallResult::OK, &steps.to_be_bytes())?;
    Ok(CallResult::OK)
}

#[cfg(target_arch = "wasm32")]
fn legacy(_: i64) -> Result<i64, ProgramError> { Err(malformed()) }

#[cfg(target_arch = "wasm32")]
layerx_program_sdk::trap_on_panic!();
#[cfg(target_arch = "wasm32")]
layerx_program_sdk::program!(legacy);
#[cfg(target_arch = "wasm32")]
layerx_program_sdk::entrypoint!(invoke);

#[cfg(test)]
mod tests {
    use super::{Interpreter, MAGIC, VERSION};

    fn accepted_vectors() -> std::vec::Vec<std::vec::Vec<u8>> {
        core::str::from_utf8(include_bytes!("../vectors/v1-arithmetic.hex"))
            .unwrap_or_else(|error| panic!("vectors: {error}"))
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| {
                line.as_bytes()
                    .chunks_exact(2)
                    .map(|pair| {
                        fn nibble(byte: u8) -> u8 {
                            match byte {
                                b'0'..=b'9' => byte - b'0',
                                b'a'..=b'f' => byte - b'a' + 10,
                                _ => panic!("non-hex vector"),
                            }
                        }
                        (nibble(pair[0]) << 4) | nibble(pair[1])
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn frozen_success_encodings_are_canonical() {
        for script in accepted_vectors() {
            assert_eq!(&script[..4], &MAGIC);
            assert_eq!(script[4], VERSION);
            Interpreter::validate(&script)
                .unwrap_or_else(|error| panic!("accepted vector: {error}"));
        }
    }
}
