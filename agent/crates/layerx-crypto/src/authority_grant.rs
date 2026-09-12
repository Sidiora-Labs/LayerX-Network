use layerx_wire::decode::Decoder;
use layerx_wire::encode::Encoder;
use layerx_wire::WireError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum GrantKind {
    DelegatedCapability = 3,
    BudgetAllowance = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GrantScope {
    pub module_mask: u64,
    pub ordinal_min: u16,
    pub ordinal_max: u16,
    pub asset: [u8; 32],
    pub maximum_per_activity: u128,
    pub maximum_total: u128,
    pub period_length: u64,
    pub maximum_per_period: u128,
    pub period_start: u64,
    pub purpose: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorityGrant {
    pub grantor: [u8; 32],
    pub grantee: [u8; 32],
    pub kind: GrantKind,
    pub delegate_key: [u8; 32],
    pub scope: GrantScope,
    pub not_before: u64,
    pub not_after: u64,
    pub revocation_sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrantError {
    Invalid,
    Wire(WireError),
}

impl From<WireError> for GrantError {
    fn from(value: WireError) -> Self {
        Self::Wire(value)
    }
}

fn fixed<const N: usize>(decoder: &mut Decoder<'_>) -> Result<[u8; N], GrantError> {
    decoder
        .bytes(N)?
        .try_into()
        .map_err(|_| GrantError::Invalid)
}

impl GrantScope {
    fn validate(&self, not_before: u64) -> Result<(), GrantError> {
        if self.module_mask == 0
            || self.module_mask & !0x03fe != 0
            || self.ordinal_min == 0
            || self.ordinal_min > self.ordinal_max
            || self.asset == [0; 32]
            || self.purpose == [0; 32]
            || self.maximum_per_activity == 0
            || (self.maximum_total == 0
                && (self.period_length == 0 || self.maximum_per_period == 0))
            || (self.maximum_total != 0 && self.maximum_per_activity > self.maximum_total)
            || (self.period_length == 0 && (self.period_start != 0 || self.maximum_per_period != 0))
            || (self.period_length != 0
                && (self.period_start != not_before
                    || self.maximum_per_activity > self.maximum_per_period))
        {
            return Err(GrantError::Invalid);
        }
        Ok(())
    }

    fn encode(&self, encoder: &mut Encoder) -> Result<(), GrantError> {
        encoder.u64(self.module_mask)?;
        encoder.u16(self.ordinal_min)?;
        encoder.u16(self.ordinal_max)?;
        encoder.bytes(&self.asset, 32)?;
        encoder.u128(self.maximum_per_activity)?;
        encoder.u128(self.maximum_total)?;
        encoder.u128(0)?;
        encoder.u64(self.period_length)?;
        encoder.u128(self.maximum_per_period)?;
        encoder.u128(0)?;
        encoder.u64(self.period_start)?;
        encoder.bytes(&self.purpose, 32)?;
        Ok(())
    }

    fn decode(decoder: &mut Decoder<'_>) -> Result<Self, GrantError> {
        let module_mask = decoder.u64()?;
        let ordinal_min = decoder.u16()?;
        let ordinal_max = decoder.u16()?;
        let asset = fixed(decoder)?;
        let maximum_per_activity = decoder.u128()?;
        let maximum_total = decoder.u128()?;
        if decoder.u128()? != 0 {
            return Err(GrantError::Invalid);
        }
        let period_length = decoder.u64()?;
        let maximum_per_period = decoder.u128()?;
        if decoder.u128()? != 0 {
            return Err(GrantError::Invalid);
        }
        let period_start = decoder.u64()?;
        let purpose = fixed(decoder)?;
        Ok(Self {
            module_mask,
            ordinal_min,
            ordinal_max,
            asset,
            maximum_per_activity,
            maximum_total,
            period_length,
            maximum_per_period,
            period_start,
            purpose,
        })
    }
}

impl AuthorityGrant {
    /// # Errors
    /// Refuses unbounded grants, invalid delegates and inconsistent spend scopes.
    pub fn validate(&self) -> Result<(), GrantError> {
        if self.grantor == [0; 32]
            || self.grantee != self.grantor
            || !crate::ed25519::public_key_is_canonical(&self.delegate_key)
            || self.not_after == u64::MAX
            || self.not_after <= self.not_before
            || self.revocation_sequence == 0
        {
            return Err(GrantError::Invalid);
        }
        self.scope.validate(self.not_before)
    }

    /// # Errors
    /// Refuses a grant that cannot be issued by the canonical governance activity.
    pub fn encode(&self) -> Result<Vec<u8>, GrantError> {
        self.validate()?;
        let mut encoder = Encoder::new(1024);
        encoder.structure_header(0x2001)?;
        encoder.u8(1)?;
        encoder.bytes(&self.grantor, 32)?;
        encoder.bytes(&self.grantee, 32)?;
        encoder.u8(self.kind as u8)?;
        encoder.bytes(&self.delegate_key, 32)?;
        self.scope.encode(&mut encoder)?;
        encoder.u64(self.not_before)?;
        encoder.u64(self.not_after)?;
        encoder.u64(self.revocation_sequence)?;
        encoder.u8(0)?;
        encoder.u64(0)?;
        encoder.bytes(&[0; 64], 64)?;
        Ok(encoder.finish())
    }

    /// # Errors
    /// Refuses noncanonical bodies, nonzero initial counters and unsupported kinds.
    pub fn decode(bytes: &[u8]) -> Result<Self, GrantError> {
        let mut decoder = Decoder::new(bytes, 0);
        decoder.structure_header(0x2001)?;
        if decoder.u8()? != 1 {
            return Err(GrantError::Invalid);
        }
        let grantor = fixed(&mut decoder)?;
        let grantee = fixed(&mut decoder)?;
        let kind = match decoder.u8()? {
            3 => GrantKind::DelegatedCapability,
            4 => GrantKind::BudgetAllowance,
            _ => return Err(GrantError::Invalid),
        };
        let delegate_key = fixed(&mut decoder)?;
        let scope = GrantScope::decode(&mut decoder)?;
        let not_before = decoder.u64()?;
        let not_after = decoder.u64()?;
        let revocation_sequence = decoder.u64()?;
        if decoder.u8()? != 0 || decoder.u64()? != 0 || fixed::<64>(&mut decoder)? != [0; 64] {
            return Err(GrantError::Invalid);
        }
        decoder.finish()?;
        let grant = Self {
            grantor,
            grantee,
            kind,
            delegate_key,
            scope,
            not_before,
            not_after,
            revocation_sequence,
        };
        if grant.encode()? != bytes {
            return Err(GrantError::Invalid);
        }
        Ok(grant)
    }

    /// # Errors
    /// Refuses malformed grant semantics or encoding beyond the payload bound.
    pub fn payload(&self) -> Result<Vec<u8>, GrantError> {
        let mut encoder = Encoder::new(1024);
        encoder.fixed(&[0x71, 8, 1, 1])?;
        encoder.bytes(&self.encode()?, 1024)?;
        Ok(encoder.finish())
    }

    /// # Errors
    /// Refuses unsupported envelope versions, fields, truncation and trailing bytes.
    pub fn from_payload(bytes: &[u8]) -> Result<Self, GrantError> {
        if bytes.len() > 1024 {
            return Err(GrantError::Invalid);
        }
        let mut decoder = Decoder::new(bytes, 0);
        if decoder.u16()? != 0x7108 || decoder.u16()? != 0x0101 {
            return Err(GrantError::Invalid);
        }
        let grant = Self::decode(decoder.bytes(1024)?)?;
        decoder.finish()?;
        Ok(grant)
    }
}
