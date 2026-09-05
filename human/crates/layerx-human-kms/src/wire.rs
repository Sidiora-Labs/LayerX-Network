use layerx_crypto::disclosure::{AmountRole, CounterpartyRole, Disclosure};

pub(crate) const MAX_FRAME: usize = 2_097_152;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    Refused = 1,
    NotFound = 2,
    Conflict = 3,
    Unavailable = 4,
    Integrity = 5,
}
pub(crate) type Result<T> = std::result::Result<T, Error>;

pub(crate) struct Request<'a> {
    pub version: u16,
    pub operation: u8,
    pub provider: &'a str,
    pub binding: [u8; 32],
    pub network: u32,
    pub class: u8,
    pub reference: &'a [u8],
    pub expected: Option<[u8; 32]>,
    pub digest: [u8; 32],
    pub canonical: &'a [u8],
    pub disclosure: &'a [u8],
}
impl<'a> Request<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > MAX_FRAME {
            return Err(Error::Refused);
        }
        let mut r = Reader { bytes, at: 0 };
        if r.take(4)? != b"LXKP" {
            return Err(Error::Refused);
        }
        let version = u16::from_be_bytes(r.fixed()?);
        let operation = r.byte()?;
        if !matches!(version, 1 | 2) || operation > 5 || (version == 2 && operation != 3) {
            return Err(Error::Refused);
        }
        let provider = std::str::from_utf8(r.blob(256)?).map_err(|_| Error::Refused)?;
        if provider.is_empty() || provider.contains('\0') {
            return Err(Error::Refused);
        }
        let mut value = Self {
            version,
            operation,
            provider,
            binding: [0; 32],
            network: 0,
            class: 0,
            reference: &[],
            expected: None,
            digest: [0; 32],
            canonical: &[],
            disclosure: &[],
        };
        if operation != 0 {
            value.binding = r.fixed()?;
            value.network = u32::from_be_bytes(r.fixed()?);
            value.class = r.byte()?;
            value.reference = r.blob(4096)?;
            if value.binding == [0; 32]
                || value.network == 0
                || !matches!(value.class, 1 | 2)
                || (operation == 1) != value.reference.is_empty()
            {
                return Err(Error::Refused);
            }
        }
        if operation == 3 && version == 2 {
            value.expected = Some(r.fixed()?);
        }
        if operation == 5 {
            value.digest = r.fixed()?;
            value.canonical = r.blob(MAX_FRAME)?;
            value.disclosure = r.blob(MAX_FRAME)?;
            if value.canonical.is_empty() || value.disclosure.is_empty() {
                return Err(Error::Refused);
            }
        }
        if r.at != bytes.len() {
            return Err(Error::Refused);
        }
        Ok(value)
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(count).ok_or(Error::Refused)?;
        let value = self.bytes.get(self.at..end).ok_or(Error::Refused)?;
        self.at = end;
        Ok(value)
    }
    fn fixed<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.take(N)?.try_into().map_err(|_| Error::Refused)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.fixed::<1>()?[0])
    }
    fn blob(&mut self, max: usize) -> Result<&'a [u8]> {
        let len = usize::try_from(u32::from_be_bytes(self.fixed()?)).map_err(|_| Error::Refused)?;
        if len > max {
            return Err(Error::Refused);
        }
        self.take(len)
    }
}
pub(crate) fn response(version: u16, operation: u8, result: Result<Vec<u8>>) -> Vec<u8> {
    let mut out = b"LXKP".to_vec();
    out.extend(version.to_be_bytes());
    out.push(operation);
    match result {
        Ok(bytes) => {
            out.push(0);
            out.extend(bytes);
        }
        Err(error) => out.push(error as u8),
    }
    out
}
pub(crate) fn blob(out: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    out.extend(
        u32::try_from(bytes.len())
            .map_err(|_| Error::Refused)?
            .to_be_bytes(),
    );
    out.extend(bytes);
    Ok(())
}
pub(crate) fn disclosure(value: &Disclosure) -> Result<Vec<u8>> {
    let mut out = vec![1];
    out.extend(value.activity_type.value().to_be_bytes());
    blob(&mut out, &value.actor)?;
    blob(&mut out, &value.authority)?;
    out.extend(
        u32::try_from(value.counterparties.len())
            .map_err(|_| Error::Refused)?
            .to_be_bytes(),
    );
    for party in &value.counterparties {
        out.push(match party.role {
            CounterpartyRole::Payer => 1,
            CounterpartyRole::Recipient => 2,
        });
        out.extend(party.account);
    }
    out.extend(
        u32::try_from(value.amounts.len())
            .map_err(|_| Error::Refused)?
            .to_be_bytes(),
    );
    for amount in &value.amounts {
        out.push(match amount.role {
            AmountRole::Transfer => 1,
            AmountRole::SpendingLimit => 2,
        });
        out.extend(amount.value.to_be_bytes());
    }
    out.extend(value.asset);
    out.extend(value.fee_limit.to_be_bytes());
    out.extend(value.expiry.not_before.to_be_bytes());
    out.extend(value.expiry.not_after.to_be_bytes());
    out.extend(value.expiry.payload_expires_at.to_be_bytes());
    out.extend(value.idempotency_key);
    if let Some(binding) = value.evm_payout_binding {
        out.push(1);
        out.extend(binding.did_id);
        out.extend(binding.network_id.to_be_bytes());
        out.extend(binding.payout_address);
        out.extend(binding.ownership_signature_digest);
    } else {
        out.push(0);
    }
    Ok(out)
}
