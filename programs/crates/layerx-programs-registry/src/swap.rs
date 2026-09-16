use layerx_programs_runtime::{derive_program_account, ProgramId, ABI_V2_VERSION};

use crate::{
    InterfaceCapability, InterfaceEntryPoint, InterfaceRefusal, ProgramInterface, ValueSchema,
    ValueType,
};

pub const TOKEN_A_PROGRAM: [u8; 32] = [0x5a; 32];
pub const TOKEN_A_ASSET: [u8; 32] = [0x6a; 32];
pub const TOKEN_B_PROGRAM: [u8; 32] = [0x5b; 32];
pub const TOKEN_B_ASSET: [u8; 32] = [0x6b; 32];
pub const SHARE_ASSET: [u8; 32] = [0x5c; 32];
pub const SHARE_CEILING: u128 = 100_000;
pub const TREASURY_SEED: &[u8] = b"lx.ref.swap.treasury";
pub const BASIS_POINTS: u128 = 10_000;
pub const FEE_BASIS_POINTS: u128 = 30;
pub const NET_BASIS_POINTS: u128 = BASIS_POINTS - FEE_BASIS_POINTS;

const RECIPIENT_OFFSET: u32 = 10;
const AMOUNT_OFFSET: u32 = 42;

/// # Errors
/// Refuses a module that does not implement the reference's exact exports and capabilities.
pub fn reference_interface(
    module: &[u8],
    program: ProgramId,
) -> Result<ProgramInterface, InterfaceRefusal> {
    let treasury = derive_program_account(program, TREASURY_SEED)
        .map_err(|_| InterfaceRefusal::Invalid)?
        .bytes();
    if treasury == TOKEN_A_PROGRAM
        || treasury == TOKEN_B_PROGRAM
        || treasury == SHARE_ASSET
        || treasury == program.bytes()
    {
        return Err(InterfaceRefusal::Invalid);
    }
    let definitions = [
        ("add_liquidity", 1, settle()?),
        ("quote", 4, read()),
        ("remove_liquidity", 2, settle()?),
        ("reserves", 5, read()),
        ("swap_exact_in", 3, trade()?),
    ];
    ProgramInterface::bind(
        module,
        ABI_V2_VERSION,
        definitions
            .into_iter()
            .map(|(name, ordinal, capabilities)| InterfaceEntryPoint {
                name: name.to_owned(),
                discriminator: [b'L', b'X', b'S', ordinal],
                calldata: ValueSchema::layerx(ValueType::Bytes { max_len: 80 }),
                response: ValueSchema::layerx(ValueType::Bytes { max_len: 64 }),
                capabilities,
                event_topics: vec![],
                failures: vec![],
            })
            .collect(),
    )
}

fn read() -> Vec<InterfaceCapability> {
    vec![
        InterfaceCapability::StorageRead,
        InterfaceCapability::SharedStorageRead,
    ]
}

fn trade() -> Result<Vec<InterfaceCapability>, InterfaceRefusal> {
    let token_a = ProgramId::new(TOKEN_A_PROGRAM).map_err(|_| InterfaceRefusal::Invalid)?;
    let token_b = ProgramId::new(TOKEN_B_PROGRAM).map_err(|_| InterfaceRefusal::Invalid)?;
    Ok(vec![
        InterfaceCapability::StorageRead,
        InterfaceCapability::StorageWrite,
        InterfaceCapability::SharedStorageRead,
        InterfaceCapability::SharedStorageWrite,
        InterfaceCapability::Call { program: token_a },
        InterfaceCapability::Call { program: token_b },
    ])
}

fn settle() -> Result<Vec<InterfaceCapability>, InterfaceRefusal> {
    let mut capabilities = trade()?;
    capabilities.push(InterfaceCapability::CallerAuthorizedSpend {
        asset: SHARE_ASSET,
        maximum_amount: SHARE_CEILING,
        recipient_offset: RECIPIENT_OFFSET,
        amount_offset: AMOUNT_OFFSET,
    });
    Ok(capabilities)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PoolRefusal {
    Amount,
    Ceiling,
    Overflow,
    Slippage,
}

/// The exact monetary law the reference swap program implements: a
/// constant-product pool over two LXT-20 tokens whose fee is retained in the
/// reserves, so every reserve equals everything ever paid in minus everything
/// ever paid out.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Pool {
    pub reserve_a: u128,
    pub reserve_b: u128,
    pub shares: u128,
}

impl Pool {
    /// Charges the exact token amounts backing `shares` and mints them.
    ///
    /// # Errors
    ///
    /// Refuses a zero mint, a charge above the caller's supplied maximum, a
    /// mint past the share ceiling and any product beyond the protocol width.
    pub fn add_liquidity(
        &mut self,
        shares: u128,
        maximum_a: u128,
        maximum_b: u128,
    ) -> Result<(u128, u128), PoolRefusal> {
        if shares == 0 {
            return Err(PoolRefusal::Amount);
        }
        let (amount_a, amount_b) = if self.shares == 0 {
            if self.reserve_a != 0 || self.reserve_b != 0 {
                return Err(PoolRefusal::Amount);
            }
            (shares, maximum_b)
        } else {
            (
                mul_div_ceil(shares, self.reserve_a, self.shares)?,
                mul_div_ceil(shares, self.reserve_b, self.shares)?,
            )
        };
        let next_shares = self
            .shares
            .checked_add(shares)
            .ok_or(PoolRefusal::Overflow)?;
        if amount_a == 0 || amount_b == 0 {
            return Err(PoolRefusal::Amount);
        }
        if amount_a > maximum_a || amount_b > maximum_b {
            return Err(PoolRefusal::Slippage);
        }
        if next_shares > SHARE_CEILING {
            return Err(PoolRefusal::Ceiling);
        }
        self.reserve_a = self
            .reserve_a
            .checked_add(amount_a)
            .ok_or(PoolRefusal::Overflow)?;
        self.reserve_b = self
            .reserve_b
            .checked_add(amount_b)
            .ok_or(PoolRefusal::Overflow)?;
        self.shares = next_shares;
        Ok((amount_a, amount_b))
    }

    /// Burns `shares` and pays out the floor of their share of each reserve.
    ///
    /// # Errors
    ///
    /// Refuses a zero or unbacked burn, a payout below the caller's supplied
    /// minimum and any product beyond the protocol width.
    pub fn remove_liquidity(
        &mut self,
        shares: u128,
        minimum_a: u128,
        minimum_b: u128,
    ) -> Result<(u128, u128), PoolRefusal> {
        if shares == 0 || self.shares == 0 || shares > self.shares {
            return Err(PoolRefusal::Amount);
        }
        let amount_a = mul_div(shares, self.reserve_a, self.shares)?;
        let amount_b = mul_div(shares, self.reserve_b, self.shares)?;
        if amount_a == 0 || amount_b == 0 {
            return Err(PoolRefusal::Amount);
        }
        if amount_a < minimum_a || amount_b < minimum_b {
            return Err(PoolRefusal::Slippage);
        }
        self.reserve_a = self
            .reserve_a
            .checked_sub(amount_a)
            .ok_or(PoolRefusal::Overflow)?;
        self.reserve_b = self
            .reserve_b
            .checked_sub(amount_b)
            .ok_or(PoolRefusal::Overflow)?;
        self.shares = self
            .shares
            .checked_sub(shares)
            .ok_or(PoolRefusal::Overflow)?;
        Ok((amount_a, amount_b))
    }

    /// Returns the output the pool would pay for `amount_in` without moving value.
    ///
    /// # Errors
    ///
    /// Refuses a zero input, an empty reserve, an output that would drain the
    /// pool and any product beyond the protocol width.
    pub fn quote(&self, a_to_b: bool, amount_in: u128) -> Result<u128, PoolRefusal> {
        let (reserve_in, reserve_out) = self.sides(a_to_b);
        output_amount(amount_in, reserve_in, reserve_out)
    }

    /// Moves the full input into the pool and pays the quoted output out.
    ///
    /// # Errors
    ///
    /// Refuses an output below the caller's supplied minimum and everything
    /// [`Pool::quote`] refuses.
    pub fn swap_exact_in(
        &mut self,
        a_to_b: bool,
        amount_in: u128,
        minimum_out: u128,
    ) -> Result<u128, PoolRefusal> {
        let (reserve_in, reserve_out) = self.sides(a_to_b);
        let amount_out = output_amount(amount_in, reserve_in, reserve_out)?;
        if amount_out < minimum_out {
            return Err(PoolRefusal::Slippage);
        }
        let next_in = reserve_in
            .checked_add(amount_in)
            .ok_or(PoolRefusal::Overflow)?;
        let next_out = reserve_out
            .checked_sub(amount_out)
            .ok_or(PoolRefusal::Overflow)?;
        if a_to_b {
            self.reserve_a = next_in;
            self.reserve_b = next_out;
        } else {
            self.reserve_b = next_in;
            self.reserve_a = next_out;
        }
        Ok(amount_out)
    }

    const fn sides(&self, a_to_b: bool) -> (u128, u128) {
        if a_to_b {
            (self.reserve_a, self.reserve_b)
        } else {
            (self.reserve_b, self.reserve_a)
        }
    }
}

fn output_amount(
    amount_in: u128,
    reserve_in: u128,
    reserve_out: u128,
) -> Result<u128, PoolRefusal> {
    if amount_in == 0 || reserve_in == 0 || reserve_out == 0 {
        return Err(PoolRefusal::Amount);
    }
    let net = mul_div(amount_in, NET_BASIS_POINTS, BASIS_POINTS)?;
    if net == 0 {
        return Err(PoolRefusal::Amount);
    }
    let denominator = reserve_in.checked_add(net).ok_or(PoolRefusal::Overflow)?;
    let amount_out = mul_div(reserve_out, net, denominator)?;
    if amount_out == 0 || amount_out >= reserve_out {
        return Err(PoolRefusal::Amount);
    }
    Ok(amount_out)
}

fn mul_div(left: u128, right: u128, divisor: u128) -> Result<u128, PoolRefusal> {
    let (high, low) = wide_mul(left, right);
    wide_div(high, low, divisor)
        .map(|(quotient, _)| quotient)
        .ok_or(PoolRefusal::Overflow)
}

fn mul_div_ceil(left: u128, right: u128, divisor: u128) -> Result<u128, PoolRefusal> {
    let (high, low) = wide_mul(left, right);
    let (quotient, remainder) = wide_div(high, low, divisor).ok_or(PoolRefusal::Overflow)?;
    if remainder == 0 {
        Ok(quotient)
    } else {
        quotient.checked_add(1).ok_or(PoolRefusal::Overflow)
    }
}

fn wide_mul(left: u128, right: u128) -> (u128, u128) {
    let mask = u128::from(u64::MAX);
    let (left_high, left_low) = (left >> 64, left & mask);
    let (right_high, right_low) = (right >> 64, right & mask);
    let low_low = left_low * right_low;
    let low_high = left_low * right_high;
    let high_low = left_high * right_low;
    let high_high = left_high * right_high;
    let middle = (low_low >> 64) + (low_high & mask) + (high_low & mask);
    let low = (low_low & mask) | (middle << 64);
    let high = high_high + (low_high >> 64) + (high_low >> 64) + (middle >> 64);
    (high, low)
}

fn wide_div(high: u128, low: u128, divisor: u128) -> Option<(u128, u128)> {
    if divisor == 0 || high >= divisor {
        return None;
    }
    let mut remainder = high;
    let mut quotient = 0_u128;
    let mut index = 128_u32;
    while index > 0 {
        index -= 1;
        let carry = remainder >> 127;
        remainder = (remainder << 1) | ((low >> index) & 1);
        if carry == 1 || remainder >= divisor {
            remainder = remainder.wrapping_sub(divisor);
            quotient |= 1_u128 << index;
        }
    }
    Some((quotient, remainder))
}
