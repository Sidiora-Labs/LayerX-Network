//! Scalar-only C bridge into the programs monetary-law validator.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::abi::{
    AbiEffects, AuthorizationContext, CallFrameId, Capability, CapabilitySet, TransferRequest,
};
use crate::transfer::{
    AtomicTransferSet, KernelTransferEvidence, KernelTransferPrimitive, TransferCapability,
    TransferLawError, TransferSource,
};
use crate::{PrincipalId, ProgramAuthority, ProgramId, MAX_PROGRAM_ACCOUNT_SEED_BYTES};

const RESULT_OK: i32 = 0;
const RESULT_NON_CANONICAL: i32 = -3;
const MODULE_PROGRAMS: u16 = 9;
const REASON_PAYMENT: u16 = 1;
const TRANSFER_CONSERVED: u8 = 0;

static NEXT_PROGRAM_SPEND_TOKEN: AtomicU64 = AtomicU64::new(1);

struct ActiveProgramSpend {
    token: u64,
    owner_program: ProgramId,
    seed: Vec<u8>,
    source: [u8; 32],
    staging_program: ProgramId,
    frame_path: [u8; 8],
    frame_depth: u8,
    destination: [u8; 32],
    asset: [u8; 32],
    amount: u128,
    transfer_set_root: [u8; 32],
    consumed: bool,
}

std::thread_local! {
    static ACTIVE_PROGRAM_SPEND: RefCell<Option<ActiveProgramSpend>> = const { RefCell::new(None) };
}

struct ActiveProgramSpendGuard {
    token: u64,
}

impl Drop for ActiveProgramSpendGuard {
    fn drop(&mut self) {
        ACTIVE_PROGRAM_SPEND.with(|active| {
            let mut active = active.borrow_mut();
            if active
                .as_ref()
                .is_some_and(|permit| permit.token == self.token)
            {
                *active = None;
            }
        });
    }
}

fn issue_program_spend(
    permit: ActiveProgramSpend,
) -> Result<ActiveProgramSpendGuard, TransferLawError> {
    let token = permit.token;
    ACTIVE_PROGRAM_SPEND.with(|active| {
        let mut active = active.borrow_mut();
        if active.is_some() {
            return Err(TransferLawError::KernelRefused);
        }
        *active = Some(permit);
        Ok(ActiveProgramSpendGuard { token })
    })
}

fn next_program_spend_token() -> u64 {
    loop {
        let token = NEXT_PROGRAM_SPEND_TOKEN.fetch_add(1, Ordering::Relaxed);
        if token != 0 {
            return token;
        }
    }
}

fn program_spend_consumed(token: u64) -> bool {
    ACTIVE_PROGRAM_SPEND.with(|active| {
        active
            .borrow()
            .as_ref()
            .is_some_and(|permit| permit.token == token && permit.consumed)
    })
}

fn bytes(words: [u64; 4]) -> [u8; 32] {
    let mut result = [0; 32];
    for (index, word) in words.into_iter().enumerate() {
        result[index * 8..index * 8 + 8].copy_from_slice(&word.to_be_bytes());
    }
    result
}

/// Authorizes one 402LXP leg without exposing a balance mutation surface.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn layerx_programs_authorize_402lxp_leg(
    p0: u64,
    p1: u64,
    p2: u64,
    p3: u64,
    r0: u64,
    r1: u64,
    r2: u64,
    r3: u64,
    h0: u64,
    h1: u64,
    h2: u64,
    h3: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    t0: u64,
    t1: u64,
    t2: u64,
    t3: u64,
    amount_hi: u64,
    amount_lo: u64,
) -> i32 {
    let Ok(program) = ProgramId::new(bytes([p0, p1, p2, p3])) else {
        return RESULT_NON_CANONICAL;
    };
    let Ok(principal) = PrincipalId::new(bytes([r0, r1, r2, r3])) else {
        return RESULT_NON_CANONICAL;
    };
    let amount = (u128::from(amount_hi) << 64) | u128::from(amount_lo);
    // The C transition has already authenticated `principal` against its
    // resolved authority and rejects a forged payer before reaching this ABI.
    // Ordinal-5 remains that direct authenticated kernel path; it must not
    // mint a guest capability or manufacture a second settlement authority.
    if program.bytes() == [0; 32]
        || principal.bytes() == [0; 32]
        || bytes([h0, h1, h2, h3]) == [0; 32]
        || bytes([a0, a1, a2, a3]) == [0; 32]
        || bytes([t0, t1, t2, t3]) == [0; 32]
        || amount == 0
    {
        return RESULT_NON_CANONICAL;
    }
    RESULT_OK
}

unsafe extern "C" {
    fn layerx_programs_wind_down_transfer_begin(
        token: u64,
        program_spend_token: u64,
        source_kind: u8,
        f0: u64,
        f1: u64,
        f2: u64,
        f3: u64,
        o0: u64,
        o1: u64,
        o2: u64,
        o3: u64,
        p0: u64,
        p1: u64,
        p2: u64,
        p3: u64,
        frame_path: u64,
        frame_depth: u8,
        seed_length: u16,
        t0: u64,
        t1: u64,
        t2: u64,
        t3: u64,
        a0: u64,
        a1: u64,
        a2: u64,
        a3: u64,
        amount_hi: u64,
        amount_lo: u64,
    ) -> i32;
    fn layerx_programs_wind_down_transfer_seed_byte(token: u64, offset: u16, byte: u8) -> i32;
    fn layerx_programs_wind_down_transfer_apply(token: u64) -> i32;
    fn layerx_programs_wind_down_transfer_root_byte(token: u64, offset: u32) -> i32;
}

fn words(bytes: [u8; 32]) -> [u64; 4] {
    let mut result = [0_u64; 4];
    for (index, chunk) in bytes.chunks_exact(8).enumerate() {
        let Ok(chunk) = chunk.try_into() else {
            return [0; 4];
        };
        result[index] = u64::from_be_bytes(chunk);
    }
    result
}

fn c_ok(result: i32) -> Result<(), TransferLawError> {
    if result == RESULT_OK {
        Ok(())
    } else {
        Err(TransferLawError::KernelRefused)
    }
}

struct WindDownKernel {
    token: u64,
}

impl KernelTransferPrimitive for WindDownKernel {
    fn apply_and_verify_402lxp_set(
        &mut self,
        transfers: &AtomicTransferSet,
    ) -> Result<KernelTransferEvidence, TransferLawError> {
        if transfers.legs().len() != 1 || !transfers.is_candidate_v2() {
            return Err(TransferLawError::InvalidTransferSet);
        }
        let leg = &transfers.legs()[0];
        let TransferSource::Program(authority) = &leg.source else {
            return Err(TransferLawError::InvalidProgramAuthority);
        };
        let from = words(authority.source_account());
        let owner = words(authority.owner_program().bytes());
        let staging = words(leg.program.bytes());
        let to = words(leg.to);
        let asset = words(leg.asset);
        let (frame_path, frame_depth) = leg.frame.canonical_bytes();
        let amount = leg.amount.to_be_bytes();
        let amount_hi = u64::from_be_bytes(
            amount[..8]
                .try_into()
                .map_err(|_| TransferLawError::InvalidTransferSet)?,
        );
        let amount_lo = u64::from_be_bytes(
            amount[8..]
                .try_into()
                .map_err(|_| TransferLawError::InvalidTransferSet)?,
        );
        let transfer_set_root = transfers.kernel_root();
        let program_spend_token = next_program_spend_token();
        let _permit = issue_program_spend(ActiveProgramSpend {
            token: program_spend_token,
            owner_program: authority.owner_program(),
            seed: authority.seed().to_vec(),
            source: authority.source_account(),
            staging_program: leg.program,
            frame_path,
            frame_depth,
            destination: leg.to,
            asset: leg.asset,
            amount: leg.amount,
            transfer_set_root,
            consumed: false,
        })?;
        c_ok(unsafe {
            layerx_programs_wind_down_transfer_begin(
                self.token,
                program_spend_token,
                2,
                from[0],
                from[1],
                from[2],
                from[3],
                owner[0],
                owner[1],
                owner[2],
                owner[3],
                staging[0],
                staging[1],
                staging[2],
                staging[3],
                u64::from_be_bytes(frame_path),
                frame_depth,
                u16::try_from(authority.seed().len())
                    .map_err(|_| TransferLawError::InvalidProgramAuthority)?,
                to[0],
                to[1],
                to[2],
                to[3],
                asset[0],
                asset[1],
                asset[2],
                asset[3],
                amount_hi,
                amount_lo,
            )
        })?;
        for (offset, byte) in authority.seed().iter().copied().enumerate() {
            c_ok(unsafe {
                layerx_programs_wind_down_transfer_seed_byte(
                    self.token,
                    u16::try_from(offset).map_err(|_| TransferLawError::InvalidProgramAuthority)?,
                    byte,
                )
            })?;
        }
        c_ok(unsafe { layerx_programs_wind_down_transfer_apply(self.token) })?;
        if !program_spend_consumed(program_spend_token) {
            return Err(TransferLawError::KernelRefused);
        }
        let mut root = [0_u8; 32];
        for (offset, byte) in root.iter_mut().enumerate() {
            let value = unsafe {
                layerx_programs_wind_down_transfer_root_byte(
                    self.token,
                    u32::try_from(offset).map_err(|_| TransferLawError::ReceiptMismatch)?,
                )
            };
            *byte = u8::try_from(value).map_err(|_| TransferLawError::ReceiptMismatch)?;
        }
        Ok(KernelTransferEvidence {
            transfer_set_root: root,
            leg_count: 1,
            total_amount: leg.amount,
        })
    }

    fn verify_402lxp_transfer_set_root(
        &self,
        transfers: &AtomicTransferSet,
        evidence: &KernelTransferEvidence,
    ) -> Result<(), TransferLawError> {
        if evidence.transfer_set_root == transfers.kernel_root()
            && evidence.leg_count == transfers.legs().len()
            && evidence.total_amount == transfers.total_amount()
        {
            Ok(())
        } else {
            Err(TransferLawError::ReceiptMismatch)
        }
    }
}

/// Consumes the one Programs spend permit issued for the exact authorized
/// transfer set currently entering the ordinary C ledger.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn layerx_programs_consume_program_spend_authorization(
    token: u64,
    origin_module_id: u16,
    from: *const u8,
    to: *const u8,
    asset: *const u8,
    amount_hi: u64,
    amount_lo: u64,
    reason: u16,
    supply_mode: u8,
    transfer_set_root: *const u8,
) -> i32 {
    if token == 0
        || origin_module_id != MODULE_PROGRAMS
        || from.is_null()
        || to.is_null()
        || asset.is_null()
        || transfer_set_root.is_null()
    {
        return RESULT_NON_CANONICAL;
    }
    let from = unsafe { core::slice::from_raw_parts(from, 32) };
    let to = unsafe { core::slice::from_raw_parts(to, 32) };
    let asset = unsafe { core::slice::from_raw_parts(asset, 32) };
    let transfer_set_root = unsafe { core::slice::from_raw_parts(transfer_set_root, 32) };
    let amount = (u128::from(amount_hi) << 64) | u128::from(amount_lo);
    ACTIVE_PROGRAM_SPEND.with(|active| {
        let mut active = active.borrow_mut();
        let Some(permit) = active.as_mut() else {
            return RESULT_NON_CANONICAL;
        };
        if permit.token != token
            || permit.consumed
            || permit.owner_program.bytes() == [0; 32]
            || permit.seed.len() > MAX_PROGRAM_ACCOUNT_SEED_BYTES
            || permit.source.as_slice() != from
            || permit.staging_program != permit.owner_program
            || permit.frame_path != [0; 8]
            || permit.frame_depth != 0
            || permit.destination.as_slice() != to
            || permit.asset.as_slice() != asset
            || permit.amount != amount
            || reason != REASON_PAYMENT
            || supply_mode != TRANSFER_CONSERVED
            || permit.transfer_set_root.as_slice() != transfer_set_root
        {
            return RESULT_NON_CANONICAL;
        }
        permit.consumed = true;
        RESULT_OK
    })
}

/// Creates and consumes the existing owner-root program-spend capability for
/// one exact wind-down leg. The Rust transfer law owns authorization and calls
/// the ordinary C kernel primitive synchronously before this function can
/// return success.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn layerx_programs_settle_wind_down_402lxp_leg(
    token: u64,
    p0: u64,
    p1: u64,
    p2: u64,
    p3: u64,
    r0: u64,
    r1: u64,
    r2: u64,
    r3: u64,
    h0: u64,
    h1: u64,
    h2: u64,
    h3: u64,
    seed: *const u8,
    seed_length: usize,
    s0: u64,
    s1: u64,
    s2: u64,
    s3: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    t0: u64,
    t1: u64,
    t2: u64,
    t3: u64,
    amount_hi: u64,
    amount_lo: u64,
) -> i32 {
    if token == 0
        || seed_length > MAX_PROGRAM_ACCOUNT_SEED_BYTES
        || (seed.is_null() && seed_length != 0)
    {
        return RESULT_NON_CANONICAL;
    }
    let Ok(program) = ProgramId::new(bytes([p0, p1, p2, p3])) else {
        return RESULT_NON_CANONICAL;
    };
    let Ok(principal) = PrincipalId::new(bytes([r0, r1, r2, r3])) else {
        return RESULT_NON_CANONICAL;
    };
    let invocation_authority = bytes([h0, h1, h2, h3]);
    let seed = if seed_length == 0 {
        &[]
    } else {
        // SAFETY: the C caller supplies the route record's immutable bytes and
        // validated bounded length for the duration of this scalar call.
        unsafe { core::slice::from_raw_parts(seed, seed_length) }
    };
    let amount = (u128::from(amount_hi) << 64) | u128::from(amount_lo);
    let source = bytes([s0, s1, s2, s3]);
    let asset = bytes([a0, a1, a2, a3]);
    let destination = bytes([t0, t1, t2, t3]);
    let Ok(program_authority) =
        ProgramAuthority::for_owner_frame(program, seed, source, asset, destination, amount)
    else {
        return RESULT_NON_CANONICAL;
    };
    let Ok(capabilities) = CapabilitySet::new([Capability::ProgramSpend {
        owner_program: program,
        seed: seed.to_vec(),
        source_account: source,
        asset,
        to: destination,
        maximum_amount: amount,
    }]) else {
        return RESULT_NON_CANONICAL;
    };
    let authorization = AuthorizationContext::new(principal, capabilities);
    let Ok(transfer) =
        TransferCapability::from_root_authorization(program, &authorization, invocation_authority)
    else {
        return RESULT_NON_CANONICAL;
    };
    let effects = AbiEffects {
        transfers: vec![TransferRequest {
            program,
            principal,
            frame: CallFrameId::root(),
            source: TransferSource::Program(program_authority),
            asset,
            to: destination,
            amount,
        }],
        ..AbiEffects::default()
    };
    let Ok(set) = transfer.authorize(&effects) else {
        return RESULT_NON_CANONICAL;
    };
    match transfer.settle_authorized_set(&set, &mut WindDownKernel { token }) {
        Ok(_) => RESULT_OK,
        Err(_) => RESULT_NON_CANONICAL,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        issue_program_spend, layerx_programs_consume_program_spend_authorization,
        program_spend_consumed, ActiveProgramSpend, ProgramId, TransferLawError, MODULE_PROGRAMS,
        REASON_PAYMENT, RESULT_NON_CANONICAL, RESULT_OK, TRANSFER_CONSERVED,
    };

    fn program(byte: u8) -> ProgramId {
        ProgramId::new([byte; 32]).unwrap_or_else(|_| panic!("nonzero fixture is canonical"))
    }

    fn permit(token: u64) -> ActiveProgramSpend {
        ActiveProgramSpend {
            token,
            owner_program: program(1),
            seed: b"escrow/primary".to_vec(),
            source: [2; 32],
            staging_program: program(1),
            frame_path: [0; 8],
            frame_depth: 0,
            destination: [3; 32],
            asset: [4; 32],
            amount: 9,
            transfer_set_root: [5; 32],
            consumed: false,
        }
    }

    unsafe fn consume(token: u64, source: &[u8; 32], root: &[u8; 32]) -> i32 {
        unsafe {
            layerx_programs_consume_program_spend_authorization(
                token,
                MODULE_PROGRAMS,
                source.as_ptr(),
                [3; 32].as_ptr(),
                [4; 32].as_ptr(),
                0,
                9,
                REASON_PAYMENT,
                TRANSFER_CONSERVED,
                root.as_ptr(),
            )
        }
    }

    #[test]
    fn program_spend_permit_is_exact_one_shot_and_cannot_nest() {
        let token = 71;
        let guard = issue_program_spend(permit(token))
            .unwrap_or_else(|_| panic!("first permit occupies the runtime slot"));
        assert_eq!(
            issue_program_spend(permit(token + 1)).err(),
            Some(TransferLawError::KernelRefused)
        );
        assert_eq!(
            unsafe { consume(token, &[6; 32], &[5; 32]) },
            RESULT_NON_CANONICAL
        );
        assert_eq!(
            unsafe { consume(token, &[2; 32], &[6; 32]) },
            RESULT_NON_CANONICAL
        );
        assert_eq!(unsafe { consume(token, &[2; 32], &[5; 32]) }, RESULT_OK);
        assert!(program_spend_consumed(token));
        assert_eq!(
            unsafe { consume(token, &[2; 32], &[5; 32]) },
            RESULT_NON_CANONICAL
        );
        drop(guard);
        assert_eq!(
            unsafe { consume(token, &[2; 32], &[5; 32]) },
            RESULT_NON_CANONICAL
        );
    }

    #[test]
    fn program_spend_permit_refuses_non_owner_frame() {
        let token = 72;
        let mut wrong_staging = permit(token);
        wrong_staging.staging_program = program(7);
        let guard = issue_program_spend(wrong_staging)
            .unwrap_or_else(|_| panic!("first permit occupies the runtime slot"));
        assert_eq!(
            unsafe { consume(token, &[2; 32], &[5; 32]) },
            RESULT_NON_CANONICAL
        );
        assert!(!program_spend_consumed(token));
        drop(guard);

        let mut wrong_frame = permit(token + 1);
        wrong_frame.frame_depth = 1;
        wrong_frame.frame_path[7] = 1;
        let guard = issue_program_spend(wrong_frame)
            .unwrap_or_else(|_| panic!("released slot accepts the next permit"));
        assert_eq!(
            unsafe { consume(token + 1, &[2; 32], &[5; 32]) },
            RESULT_NON_CANONICAL
        );
        assert!(!program_spend_consumed(token + 1));
        drop(guard);
    }
}
