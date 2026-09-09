use crate::{derive_program_account, AbiError, Capability, ProgramId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallerAuthorizedSpend {
    pub asset: [u8; 32],
    pub maximum_amount: u128,
    pub recipient_offset: u32,
    pub amount_offset: u32,
}

impl CallerAuthorizedSpend {
    /// # Errors
    /// Refuses malformed calldata, an exceeded descriptor ceiling, or a missing
    /// caller grant bound to this program, derived source, asset and recipient.
    pub fn authorize(
        self,
        program: ProgramId,
        calldata: &[u8],
        grants: &[Capability],
    ) -> Result<(), AbiError> {
        let to = Self::field::<32>(calldata, self.recipient_offset)?;
        let amount = u128::from_be_bytes(Self::field::<16>(calldata, self.amount_offset)?);
        if self.asset == [0; 32] || to == [0; 32] || amount == 0 || amount > self.maximum_amount {
            return Err(AbiError::InvalidCapability);
        }
        if grants.iter().any(|grant| match grant {
            Capability::ProgramSpend {
                owner_program,
                seed,
                source_account,
                asset,
                to: recipient,
                maximum_amount,
            } => {
                *owner_program == program
                    && *asset == self.asset
                    && *recipient == to
                    && *maximum_amount >= amount
                    && derive_program_account(program, seed)
                        .is_ok_and(|account| account.bytes() == *source_account)
            }
            _ => false,
        }) {
            Ok(())
        } else {
            Err(AbiError::InvalidCapability)
        }
    }

    /// # Errors
    /// Refuses any descriptor not authorized by the caller, then narrows this
    /// program's spend grants to the described assets, recipients and ceilings.
    pub fn constrain_grants(
        program: ProgramId,
        calldata: &[u8],
        descriptors: &[Self],
        grants: &[Capability],
    ) -> Result<Vec<Capability>, AbiError> {
        for descriptor in descriptors {
            descriptor.authorize(program, calldata, grants)?;
        }
        let mut constrained = Vec::new();
        for grant in grants {
            let mut grant = grant.clone();
            if let Capability::ProgramSpend {
                owner_program,
                asset,
                to,
                maximum_amount,
                ..
            } = &mut grant
            {
                if *owner_program == program {
                    let mut ceiling = 0;
                    for descriptor in descriptors {
                        if descriptor.asset == *asset
                            && Self::field::<32>(calldata, descriptor.recipient_offset)? == *to
                        {
                            ceiling = ceiling.max(descriptor.maximum_amount);
                        }
                    }
                    if ceiling == 0 {
                        continue;
                    }
                    *maximum_amount = (*maximum_amount).min(ceiling);
                }
            }
            constrained.push(grant);
        }
        Ok(constrained)
    }

    fn field<const N: usize>(calldata: &[u8], offset: u32) -> Result<[u8; N], AbiError> {
        let offset = usize::try_from(offset).map_err(|_| AbiError::InvalidCapability)?;
        let end = offset.checked_add(N).ok_or(AbiError::InvalidCapability)?;
        calldata
            .get(offset..end)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(AbiError::InvalidCapability)
    }
}
