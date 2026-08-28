use super::{
    Abi, AbiError, AuthorizationContext, Capability, CapabilitySet, ReceiptOracle, ReceiptView,
    MAX_CANONICAL_CAPABILITY_SET_BYTES, MAX_CAPABILITIES, MAX_CAPABILITY_ENCODING_BYTES,
    MAX_EVENT_DATA_BYTES, MAX_EVENT_TOPIC_BYTES, MAX_EVENTS_PER_ACTIVITY,
};
use crate::{
    derive_program_account, FeeSchedule, Meter, PrincipalId, ProgramId, ResourceBudget, Storage,
    ABI_VERSION, MAX_PROGRAM_ACCOUNT_SEED_BYTES,
};

struct NoReceipts;

impl ReceiptOracle for NoReceipts {
    fn verified_receipt(&self, _: [u8; 32]) -> Result<ReceiptView, AbiError> {
        Err(AbiError::ReceiptMismatch)
    }
}

fn program(byte: u8) -> ProgramId {
    ProgramId::new([byte; 32]).unwrap_or_else(|error| panic!("program: {error}"))
}

fn principal(byte: u8) -> PrincipalId {
    PrincipalId::new([byte; 32]).unwrap_or_else(|error| panic!("principal: {error}"))
}

fn event_abi() -> Abi {
    Abi::new(
        ABI_VERSION,
        program(1),
        AuthorizationContext::new(
            principal(2),
            CapabilitySet::new([Capability::EmitEvent])
                .unwrap_or_else(|error| panic!("event capability: {error}")),
        ),
        Storage::new(),
        &NoReceipts,
    )
    .unwrap_or_else(|error| panic!("event ABI: {error}"))
}

#[test]
fn event_count_boundary_refuses_before_a_sixty_fifth_stage() {
    let mut abi = event_abi();
    let mut meter = Meter::declared();
    for _ in 0..MAX_EVENTS_PER_ACTIVITY {
        abi.emit_event(&mut meter, 1, 0)
            .unwrap_or_else(|error| panic!("reserve boundary event: {error}"));
        abi.stage_reserved_event(vec![1], Vec::new())
            .unwrap_or_else(|error| panic!("stage boundary event: {error}"));
    }
    assert_eq!(
        abi.emit_event(&mut meter, 1, 0),
        Err(AbiError::EventBounds)
    );
    assert_eq!(abi.commit().effects.events.len(), MAX_EVENTS_PER_ACTIVITY);
}

#[test]
fn event_bytes_are_charged_before_staging_and_exhaust_atomically() {
    let limit = u64::try_from(MAX_EVENT_TOPIC_BYTES + MAX_EVENT_DATA_BYTES)
        .unwrap_or_else(|error| panic!("event limit: {error}"));
    let mut abi = event_abi();
    let mut meter = Meter::new(
        ResourceBudget::declared().with_output_bytes(limit),
        FeeSchedule::declared(),
    );
    abi.emit_event(&mut meter, MAX_EVENT_TOPIC_BYTES, MAX_EVENT_DATA_BYTES)
        .unwrap_or_else(|error| panic!("reserve exact byte ceiling: {error}"));
    abi.stage_reserved_event(
        vec![1; MAX_EVENT_TOPIC_BYTES],
        vec![2; MAX_EVENT_DATA_BYTES],
    )
    .unwrap_or_else(|error| panic!("stage exact byte ceiling: {error}"));
    assert!(matches!(
        abi.emit_event(&mut meter, 1, 0),
        Err(AbiError::Meter(_))
    ));
    assert_eq!(abi.commit().effects.events.len(), 1);
}

#[test]
fn maximum_largest_grant_set_fits_the_transport_ceiling() {
    let owner = program(3);
    let grant = |index: usize| {
        let mut seed = vec![7; MAX_PROGRAM_ACCOUNT_SEED_BYTES];
        let ordinal = u16::try_from(index)
            .unwrap_or_else(|error| panic!("capability ordinal: {error}"));
        seed[..2].copy_from_slice(&ordinal.to_be_bytes());
        let source_account = derive_program_account(owner, &seed)
            .unwrap_or_else(|error| panic!("derived account: {error}"))
            .bytes();
        Capability::ProgramSpend {
            owner_program: owner,
            seed,
            source_account,
            asset: [4; 32],
            to: [5; 32],
            maximum_amount: 1,
        }
    };
    let set = CapabilitySet::new((0..MAX_CAPABILITIES).map(&grant))
        .unwrap_or_else(|error| panic!("maximum capability set: {error}"));
    assert_eq!(MAX_CAPABILITIES, 238);
    assert_eq!(MAX_CANONICAL_CAPABILITY_SET_BYTES, 65_452);
    assert_eq!(set.canonical_encoding().len(), MAX_CANONICAL_CAPABILITY_SET_BYTES);
    assert!(MAX_CANONICAL_CAPABILITY_SET_BYTES <= MAX_CAPABILITY_ENCODING_BYTES);
    assert_eq!(
        CapabilitySet::new((0..=MAX_CAPABILITIES).map(grant)),
        Err(AbiError::InvalidCapability)
    );
}
