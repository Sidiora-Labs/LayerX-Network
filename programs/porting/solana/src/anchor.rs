//! Anchor instruction, event and cross-program-invocation patterns carried
//! onto the version-one program ABI.
//!
//! Anchor identifies things by an eight-byte discriminator taken from a
//! `sha256` preimage: `account:Name` for an account, `global:name` for an
//! instruction, `event:Name` for an event. Every one of those preimages is
//! kept, so an existing client's instruction data and an existing indexer's
//! event filter both keep matching after the port.

use layerx_programs_runtime::abi::{MAX_CALL_INPUT_BYTES, MAX_EVENT_DATA_BYTES};
use layerx_programs_runtime::{Capability, ProgramId};

use crate::account::{FieldType, FieldValue};
use crate::error::PortRefusal;
use crate::hash::sha256;

/// Native Solana/Anchor names backed by authenticated ABI v2 context.
#[cfg(target_arch="wasm32")]
pub mod context {
    use layerx_program_sdk::{Context, Principal, ProgramError, ProgramId};
    pub struct AnchorContext { pub program_id:ProgramId, pub signer:Principal, pub slot:u64 }
    impl AnchorContext { pub fn current()->Result<Self,ProgramError>{Ok(Self{program_id:Context::executing_program()?,signer:Context::invoking_principal()?,slot:Context::batch_height()?})} }
}

/// The width of every Anchor discriminator.
pub const DISCRIMINATOR_BYTES: usize = 8;

/// Longest identifier a ported discriminator preimage may carry.
pub const MAX_IDENTIFIER_BYTES: usize = 128;

/// Maximum number of arguments a ported instruction or event may declare.
pub const MAX_ARGUMENTS: usize = 32;

fn discriminator(namespace: &str, identifier: &str) -> [u8; DISCRIMINATOR_BYTES] {
    let mut preimage = Vec::with_capacity(
        namespace
            .len()
            .saturating_add(identifier.len())
            .saturating_add(1),
    );
    preimage.extend_from_slice(namespace.as_bytes());
    preimage.push(b':');
    preimage.extend_from_slice(identifier.as_bytes());
    let digest = sha256(&preimage);
    let mut tag = [0_u8; DISCRIMINATOR_BYTES];
    for (slot, byte) in tag.iter_mut().zip(digest) {
        *slot = byte;
    }
    tag
}

/// Returns the discriminator Anchor writes into the first eight bytes of an
/// account named `name`.
#[must_use]
pub fn account_discriminator(name: &str) -> [u8; DISCRIMINATOR_BYTES] {
    discriminator("account", name)
}

/// Returns the discriminator an Anchor client puts at the head of the
/// instruction data for the handler named `name`, which is the handler's
/// `snake_case` function name.
#[must_use]
pub fn instruction_discriminator(name: &str) -> [u8; DISCRIMINATOR_BYTES] {
    discriminator("global", name)
}

/// Returns the discriminator `emit!` writes at the head of an event named
/// `name`.
#[must_use]
pub fn event_discriminator(name: &str) -> [u8; DISCRIMINATOR_BYTES] {
    discriminator("event", name)
}

fn check_arguments(name: &str, arguments: &[FieldType]) -> Result<(), PortRefusal> {
    if name.is_empty() || name.len() > MAX_IDENTIFIER_BYTES || arguments.len() > MAX_ARGUMENTS {
        return Err(PortRefusal::SchemaMismatch);
    }
    Ok(())
}

fn encode_arguments(
    declared: &[FieldType],
    values: &[FieldValue],
    head: [u8; DISCRIMINATOR_BYTES],
) -> Result<Vec<u8>, PortRefusal> {
    if values.len() != declared.len() {
        return Err(PortRefusal::SchemaMismatch);
    }
    let mut encoded = Vec::with_capacity(DISCRIMINATOR_BYTES);
    encoded.extend_from_slice(&head);
    for (kind, value) in declared.iter().zip(values) {
        if value.kind() != *kind {
            return Err(PortRefusal::SchemaMismatch);
        }
        value.encode(&mut encoded);
    }
    Ok(encoded)
}

/// One Anchor instruction handler carried onto the call entry point.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstructionAbi {
    name: String,
    discriminator: [u8; DISCRIMINATOR_BYTES],
    arguments: Vec<FieldType>,
}

impl InstructionAbi {
    /// Declares an instruction by its `snake_case` handler name and its
    /// `borsh` argument types, in declaration order.
    ///
    /// # Errors
    ///
    /// Refuses an unnamed instruction and more arguments than the bound.
    pub fn new(name: &str, arguments: Vec<FieldType>) -> Result<Self, PortRefusal> {
        check_arguments(name, &arguments)?;
        Ok(Self {
            name: name.to_owned(),
            discriminator: instruction_discriminator(name),
            arguments,
        })
    }

    /// Returns the handler name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the eight-byte discriminator, byte-identical to the one an
    /// existing client already sends.
    #[must_use]
    pub const fn discriminator(&self) -> [u8; DISCRIMINATOR_BYTES] {
        self.discriminator
    }

    /// Borrows the declared argument types.
    #[must_use]
    pub fn arguments(&self) -> &[FieldType] {
        &self.arguments
    }

    /// Encodes instruction data exactly as an Anchor client does: the
    /// discriminator followed by every argument in `borsh` order.
    ///
    /// # Errors
    ///
    /// Refuses a wrong argument list or data beyond the ABI's input bound.
    pub fn data(&self, values: &[FieldValue]) -> Result<Vec<u8>, PortRefusal> {
        let encoded = encode_arguments(&self.arguments, values, self.discriminator)?;
        if encoded.len() > MAX_CALL_INPUT_BYTES {
            return Err(PortRefusal::InstructionDataTooLarge);
        }
        Ok(encoded)
    }

    /// Returns the discriminator as the little-endian `i64` a ported dispatcher
    /// compares an eight-byte load against.
    #[must_use]
    pub const fn dispatch_word(&self) -> i64 {
        i64::from_le_bytes(self.discriminator)
    }
}

/// One `#[event]` struct carried onto the ABI's single-topic event shape.
///
/// Anchor emits an event as a program log holding the discriminator followed by
/// the `borsh` fields. The port keeps the discriminator as the topic and the
/// `borsh` fields as the payload, so a client decodes the payload with the
/// generated type unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnchorEvent {
    name: String,
    discriminator: [u8; DISCRIMINATOR_BYTES],
    fields: Vec<FieldType>,
}

impl AnchorEvent {
    /// Declares an event by its struct name and its `borsh` field types.
    ///
    /// # Errors
    ///
    /// Refuses an unnamed event and more fields than the bound.
    pub fn new(name: &str, fields: Vec<FieldType>) -> Result<Self, PortRefusal> {
        check_arguments(name, &fields)?;
        Ok(Self {
            name: name.to_owned(),
            discriminator: event_discriminator(name),
            fields,
        })
    }

    /// Returns the event struct name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the eight-byte event topic, byte-identical to the Anchor
    /// discriminator an existing indexer filters on.
    #[must_use]
    pub const fn topic(&self) -> [u8; DISCRIMINATOR_BYTES] {
        self.discriminator
    }

    /// Borrows the declared field types.
    #[must_use]
    pub fn fields(&self) -> &[FieldType] {
        &self.fields
    }

    /// Encodes the event payload as the `borsh` fields alone. The
    /// discriminator travels as the topic instead of being repeated in the
    /// payload.
    ///
    /// # Errors
    ///
    /// Refuses a wrong field list or a payload beyond the ABI's data bound.
    pub fn data(&self, values: &[FieldValue]) -> Result<Vec<u8>, PortRefusal> {
        let framed = encode_arguments(&self.fields, values, self.discriminator)?;
        let payload = framed
            .get(DISCRIMINATOR_BYTES..)
            .ok_or(PortRefusal::SchemaMismatch)?
            .to_vec();
        if payload.len() > MAX_EVENT_DATA_BYTES {
            return Err(PortRefusal::EventDataTooLarge);
        }
        Ok(payload)
    }
}

/// One translated `CpiContext` call site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CpiRequest {
    /// The callee program that replaces the account-passed program handle.
    pub callee: ProgramId,
    /// Instruction data in Anchor's own discriminator-plus-`borsh` encoding.
    pub input: Vec<u8>,
    /// The single authority the call needs, which the caller must already hold.
    pub authority: Capability,
}

/// Translates a `CpiContext::new(program.to_account_info(), accounts)` call
/// into the call request the ABI accepts.
///
/// A Solana cross-program invocation passes account handles the callee may
/// mutate, and `invoke_signed` additionally lends the caller's program-derived
/// signing authority. Neither survives: a ported call carries instruction data
/// and one narrowed `Call` capability, and the callee reaches only its own
/// namespace.
///
/// # Errors
///
/// Refuses a wrong argument list or oversized instruction data.
pub fn cross_program_invocation(
    callee: ProgramId,
    instruction: &InstructionAbi,
    arguments: &[FieldValue],
) -> Result<CpiRequest, PortRefusal> {
    let input = instruction.data(arguments)?;
    Ok(CpiRequest {
        callee,
        input,
        authority: Capability::Call { program: callee },
    })
}

/// What the runtime does with each Anchor failure mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureMapping {
    /// `require!(cond, MyError::Variant)` and `err!(MyError::Variant)`.
    Require,
    /// A failed `#[account(...)]` constraint Anchor checks before the handler.
    ConstraintViolation,
    /// A `#[account]` load whose discriminator does not match.
    DiscriminatorMismatch,
    /// A Rust `panic!`, an arithmetic overflow or an index out of bounds.
    Panic,
    /// Exhausting the transaction's compute budget.
    ComputeBudget,
    /// Exceeding the cross-program-invocation depth limit.
    CpiDepth,
}

/// The version-one behaviour an Anchor failure mode becomes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeOutcome {
    /// The guest executes `unreachable`; every staged write and effect is
    /// discarded, which is exactly Solana's all-or-nothing transaction failure.
    Trap,
    /// The deterministic meter refuses the execution before any effect escapes.
    ResourceRefusal,
    /// The declared value-stack or call-depth bound is exhausted.
    StackExhausted,
}

impl FailureMapping {
    /// Returns the runtime behaviour the failure mode maps onto.
    #[must_use]
    pub const fn outcome(self) -> RuntimeOutcome {
        match self {
            Self::Require
            | Self::ConstraintViolation
            | Self::DiscriminatorMismatch
            | Self::Panic => RuntimeOutcome::Trap,
            Self::ComputeBudget => RuntimeOutcome::ResourceRefusal,
            Self::CpiDepth => RuntimeOutcome::StackExhausted,
        }
    }
}
