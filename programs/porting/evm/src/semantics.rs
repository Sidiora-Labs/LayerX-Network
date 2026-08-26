//! Event, call and failure semantics translated from Solidity to the
//! version-one program ABI.

use layerx_programs_runtime::abi::{
    MAX_CALL_INPUT_BYTES, MAX_EVENT_DATA_BYTES, MAX_EVENT_TOPIC_BYTES,
};
use layerx_programs_runtime::{Capability, ProgramId};

use crate::error::PortRefusal;
use crate::keccak::{keccak256, selector};
use crate::value::Word;

/// Solidity execution vocabulary read directly from authenticated ABI v2 facts.
#[cfg(target_arch="wasm32")]
pub use layerx_porting_evm_guest as context;

const MAX_SIGNATURE_BYTES: usize = 256;

fn parameter_count(signature: &str) -> Result<usize, PortRefusal> {
    let open = signature.find('(').ok_or(PortRefusal::InvalidSignature)?;
    if !signature.ends_with(')') || open == 0 || signature.len() > MAX_SIGNATURE_BYTES {
        return Err(PortRefusal::InvalidSignature);
    }
    let inner = signature
        .get(open + 1..signature.len() - 1)
        .ok_or(PortRefusal::InvalidSignature)?;
    if inner.is_empty() {
        return Ok(0);
    }
    if inner.split(',').any(str::is_empty) {
        return Err(PortRefusal::InvalidSignature);
    }
    Ok(inner.split(',').count())
}

/// A Solidity event carried onto the program ABI's single-topic event shape.
///
/// `topic0` keeps its `keccak256` value so an existing indexer's filters match
/// unchanged. Solidity's additional indexed topics have no version-one
/// equivalent and are not needed for `msg.sender`, which every program event
/// already carries as its principal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventAbi {
    signature: String,
    topic: [u8; 32],
    arguments: usize,
    carried: usize,
}

impl EventAbi {
    /// Parses a canonical event signature such as `Transfer(address,uint256)`
    /// whose every argument is carried in the payload.
    ///
    /// # Errors
    ///
    /// Refuses a malformed or oversized signature.
    pub fn new(signature: &str) -> Result<Self, PortRefusal> {
        let arguments = parameter_count(signature)?;
        Self::build(signature, arguments, arguments)
    }

    /// Parses a signature whose leading `derived` arguments are supplied by the
    /// event envelope instead of the payload.
    ///
    /// A program event already carries the emitting program and the invoking
    /// principal, so a Solidity `from`/`to` address pair that only ever holds
    /// the mint sentinel and `msg.sender` adds nothing to the payload. Keeping
    /// the signature, and with it `topic0`, means an existing indexer's filter
    /// still matches.
    ///
    /// # Errors
    ///
    /// Refuses a malformed signature or a derived count beyond the arguments.
    pub fn envelope_derived(signature: &str, derived: usize) -> Result<Self, PortRefusal> {
        let arguments = parameter_count(signature)?;
        let carried = arguments
            .checked_sub(derived)
            .ok_or(PortRefusal::ArgumentCountMismatch)?;
        Self::build(signature, arguments, carried)
    }

    fn build(signature: &str, arguments: usize, carried: usize) -> Result<Self, PortRefusal> {
        let topic = keccak256(signature.as_bytes());
        if topic.len() > MAX_EVENT_TOPIC_BYTES {
            return Err(PortRefusal::InvalidSignature);
        }
        Ok(Self {
            signature: signature.to_string(),
            topic,
            arguments,
            carried,
        })
    }

    /// Returns the canonical signature text.
    #[must_use]
    pub fn signature(&self) -> &str {
        &self.signature
    }

    /// Returns `topic0`, byte-identical to the EVM log topic.
    #[must_use]
    pub const fn topic(&self) -> [u8; 32] {
        self.topic
    }

    /// Returns the declared argument count of the Solidity event.
    #[must_use]
    pub const fn arguments(&self) -> usize {
        self.arguments
    }

    /// Returns the number of arguments carried in the ported payload.
    #[must_use]
    pub const fn carried(&self) -> usize {
        self.carried
    }

    /// Encodes event data as one 32-byte word per carried argument, in
    /// declaration order, exactly as `abi.encode` lays out a log payload.
    ///
    /// # Errors
    ///
    /// Refuses a wrong argument count or a payload beyond the ABI bound.
    pub fn data(&self, arguments: &[Word]) -> Result<Vec<u8>, PortRefusal> {
        if arguments.len() != self.carried {
            return Err(PortRefusal::ArgumentCountMismatch);
        }
        let mut encoded = Vec::with_capacity(arguments.len().saturating_mul(32));
        for argument in arguments {
            encoded.extend_from_slice(&argument.bytes());
        }
        if encoded.len() > MAX_EVENT_DATA_BYTES {
            return Err(PortRefusal::EventDataTooLarge);
        }
        Ok(encoded)
    }
}

/// A Solidity external function carried onto `program_call`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MethodAbi {
    signature: String,
    selector: [u8; 4],
    parameters: usize,
}

impl MethodAbi {
    /// Parses a canonical method signature such as `transfer(address,uint256)`.
    ///
    /// # Errors
    ///
    /// Refuses a malformed or oversized signature.
    pub fn new(signature: &str) -> Result<Self, PortRefusal> {
        let parameters = parameter_count(signature)?;
        Ok(Self {
            signature: signature.to_string(),
            selector: selector(signature),
            parameters,
        })
    }

    /// Returns the canonical signature text.
    #[must_use]
    pub fn signature(&self) -> &str {
        &self.signature
    }

    /// Returns the four-byte selector, byte-identical to the EVM's.
    #[must_use]
    pub const fn selector(&self) -> [u8; 4] {
        self.selector
    }

    /// Returns the declared parameter count.
    #[must_use]
    pub const fn parameters(&self) -> usize {
        self.parameters
    }

    /// Encodes calldata as `selector . word*`, the head-only `abi.encode`
    /// layout every value-typed Solidity signature produces.
    ///
    /// # Errors
    ///
    /// Refuses a wrong argument count or an input beyond the ABI bound.
    pub fn calldata(&self, arguments: &[Word]) -> Result<Vec<u8>, PortRefusal> {
        if arguments.len() != self.parameters {
            return Err(PortRefusal::ArgumentCountMismatch);
        }
        let mut encoded = Vec::with_capacity(4 + arguments.len().saturating_mul(32));
        encoded.extend_from_slice(&self.selector);
        for argument in arguments {
            encoded.extend_from_slice(&argument.bytes());
        }
        if encoded.len() > MAX_CALL_INPUT_BYTES {
            return Err(PortRefusal::CalldataTooLarge);
        }
        Ok(encoded)
    }
}

/// One translated `IContract(target).method(args)` call site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallRequest {
    /// The callee program that replaces the target contract address.
    pub callee: ProgramId,
    /// Calldata in the EVM's own head-only encoding.
    pub input: Vec<u8>,
    /// The single authority the call needs, which the caller must already hold.
    pub authority: Capability,
}

/// Translates a Solidity external call into the call request the ABI accepts.
///
/// A Solidity call inherits the caller's whole authority. A program call
/// carries only what the caller explicitly narrows, so the translation names
/// the exact grant rather than assuming ambient reach.
///
/// # Errors
///
/// Refuses a wrong argument count or oversized calldata.
pub fn external_call(
    callee: ProgramId,
    method: &MethodAbi,
    arguments: &[Word],
) -> Result<CallRequest, PortRefusal> {
    let input = method.calldata(arguments)?;
    Ok(CallRequest {
        callee,
        input,
        authority: Capability::Call { program: callee },
    })
}

/// What the runtime does with each Solidity failure mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureMapping {
    /// `require(cond, "reason")`.
    Require,
    /// `revert CustomError()` and `revert("reason")`.
    Revert,
    /// `assert(cond)` and every compiler-inserted panic.
    AssertPanic,
    /// Running out of gas.
    OutOfGas,
    /// Exceeding the call-depth limit.
    CallDepth,
}

/// The version-one behaviour a Solidity failure mode becomes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeOutcome {
    /// The guest executes `unreachable`; every staged write and effect is
    /// discarded, which is exactly Solidity's all-or-nothing revert.
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
            Self::Require | Self::Revert | Self::AssertPanic => RuntimeOutcome::Trap,
            Self::OutOfGas => RuntimeOutcome::ResourceRefusal,
            Self::CallDepth => RuntimeOutcome::StackExhausted,
        }
    }
}
