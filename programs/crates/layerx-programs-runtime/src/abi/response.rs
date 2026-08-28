//! Bounded successful response transport for the candidate ABI revision.

use core::fmt::{self, Display};

use crate::meter::MeterRefusal;

use super::HostFunction;

/// Explicitly non-current module carrying candidate response operations.
pub const CANDIDATE_ABI_MODULE: &str = super::manifest::ABI_V2_MODULE;
pub const CANDIDATE_ABI_MANIFEST: &str = super::manifest::ABI_V2_MANIFEST;

/// Compatibility alias for the complete frozen ABI-v2 host table.
pub const CANDIDATE_HOST_FUNCTIONS: [HostFunction; 19] =
    super::manifest::ABI_V2_HOST_FUNCTIONS;

/// Maximum successful response payload crossing one call boundary.
pub const MAX_CALL_RESPONSE_BYTES: usize = 1_048_576;

/// Owned response returned by one successful program invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallResponse {
    pub code: i32,
    pub bytes: Vec<u8>,
}

/// Typed refusal while publishing or transporting a response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResponseRefusal {
    TooLarge { bytes: usize, limit: usize },
    CapacityExceeded { bytes: usize, capacity: usize },
    DuplicatePublication,
    InvalidPublication,
    CodeMismatch { published: i32, returned: i32 },
    Meter(MeterRefusal),
}

impl Display for ResponseRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { bytes, limit } => {
                write!(formatter, "response size {bytes} exceeds limit {limit}")
            }
            Self::CapacityExceeded { bytes, capacity } => {
                write!(
                    formatter,
                    "response size {bytes} exceeds caller capacity {capacity}"
                )
            }
            Self::DuplicatePublication => formatter.write_str("response already published"),
            Self::InvalidPublication => {
                formatter.write_str("response publication region is invalid")
            }
            Self::CodeMismatch {
                published,
                returned,
            } => write!(
                formatter,
                "published response code {published} differs from returned code {returned}"
            ),
            Self::Meter(refusal) => Display::fmt(refusal, formatter),
        }
    }
}

impl std::error::Error for ResponseRefusal {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResponseRegion {
    capacity: usize,
    published: Option<CallResponse>,
    refusal: Option<ResponseRefusal>,
}

impl ResponseRegion {
    pub(crate) fn canonical_state_len(&self) -> Result<u64, ()> {
        let payload = self.published.as_ref().map_or(0, |response| response.bytes.len());
        let refusal = self.refusal.as_ref().map_or(0, ResponseRefusal::canonical_len);
        8_u64.checked_add(1).and_then(|value| value.checked_add(if self.published.is_some() { 12_u64.checked_add(u64::try_from(payload).ok()?)? } else { 0 }))
            .and_then(|value| value.checked_add(1))
            .and_then(|value| value.checked_add(u64::try_from(refusal).ok()?)).ok_or(())
    }

    pub(crate) fn canonical_state_write(&self, mut write: impl FnMut(&[u8])) {
        write(&(self.capacity as u64).to_be_bytes());
        match &self.published {
            None => write(&[0]),
            Some(response) => { write(&[1]); write(&response.code.to_be_bytes()); write(&(response.bytes.len() as u64).to_be_bytes()); write(&response.bytes); }
        }
        match &self.refusal { None => write(&[0]), Some(refusal) => { write(&[1]); refusal.canonical_write(write); } }
    }

    pub(crate) fn canonical_state_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.canonical_state_len().unwrap_or(0) as usize);
        self.canonical_state_write(|part| bytes.extend_from_slice(part));
        bytes
    }

    pub(crate) const fn has_publication(&self) -> bool {
        self.published.is_some() || self.refusal.is_some()
    }

    pub(crate) fn new(capacity: usize) -> Result<Self, ResponseRefusal> {
        if capacity > MAX_CALL_RESPONSE_BYTES {
            return Err(ResponseRefusal::TooLarge {
                bytes: capacity,
                limit: MAX_CALL_RESPONSE_BYTES,
            });
        }
        Ok(Self {
            capacity,
            published: None,
            refusal: None,
        })
    }

    pub(crate) fn publish(&mut self, response: CallResponse) -> Result<(), ResponseRefusal> {
        if let Some(refusal) = &self.refusal {
            return Err(refusal.clone());
        }
        let result = if self.published.is_some() {
            Err(ResponseRefusal::DuplicatePublication)
        } else if response.bytes.len() > MAX_CALL_RESPONSE_BYTES {
            Err(ResponseRefusal::TooLarge {
                bytes: response.bytes.len(),
                limit: MAX_CALL_RESPONSE_BYTES,
            })
        } else if response.bytes.len() > self.capacity {
            Err(ResponseRefusal::CapacityExceeded {
                bytes: response.bytes.len(),
                capacity: self.capacity,
            })
        } else {
            self.published = Some(response);
            Ok(())
        };
        if let Err(refusal) = &result {
            self.refusal = Some(refusal.clone());
        }
        result
    }

    pub(crate) fn refuse(&mut self, refusal: ResponseRefusal) {
        if self.refusal.is_none() {
            self.refusal = Some(refusal);
        }
    }

    pub(crate) fn finish(&self, returned: i32) -> Result<CallResponse, ResponseRefusal> {
        if let Some(refusal) = &self.refusal {
            return Err(refusal.clone());
        }
        let response = self.published.clone().unwrap_or(CallResponse {
            code: returned,
            bytes: Vec::new(),
        });
        if response.code != returned {
            return Err(ResponseRefusal::CodeMismatch {
                published: response.code,
                returned,
            });
        }
        Ok(response)
    }
}

impl ResponseRefusal {
    pub(crate) fn canonical_len(&self) -> usize { match self { Self::TooLarge { .. } | Self::CapacityExceeded { .. } => 17, Self::CodeMismatch { .. } => 9, Self::Meter(refusal) => 1 + meter_len(refusal), _ => 1 } }
    pub(crate) fn canonical_write(&self, mut write: impl FnMut(&[u8])) {
        match self {
            Self::TooLarge { bytes, limit } => { write(&[0]); write(&(*bytes as u64).to_be_bytes()); write(&(*limit as u64).to_be_bytes()); }
            Self::CapacityExceeded { bytes, capacity } => { write(&[1]); write(&(*bytes as u64).to_be_bytes()); write(&(*capacity as u64).to_be_bytes()); }
            Self::DuplicatePublication => write(&[2]), Self::InvalidPublication => write(&[3]),
            Self::CodeMismatch { published, returned } => { write(&[4]); write(&published.to_be_bytes()); write(&returned.to_be_bytes()); }
            Self::Meter(refusal) => { write(&[5]); meter_write(refusal, write); }
        }
    }
}

fn resource_code(resource: crate::meter::ResourceKind) -> u8 { match resource { crate::meter::ResourceKind::Cpu => 0, crate::meter::ResourceKind::Memory => 1, crate::meter::ResourceKind::StorageRead => 2, crate::meter::ResourceKind::StorageWrite => 3, crate::meter::ResourceKind::StorageOccupancy => 4, crate::meter::ResourceKind::Output => 5, crate::meter::ResourceKind::OutputBytes => 6 } }
fn meter_len(refusal: &MeterRefusal) -> usize { match refusal { MeterRefusal::BudgetExceeded { .. } => 18, MeterRefusal::CounterOverflow { .. } => 2, MeterRefusal::FeeOverflow => 1 } }
fn meter_write(refusal: &MeterRefusal, mut write: impl FnMut(&[u8])) { match refusal { MeterRefusal::BudgetExceeded { resource, limit, attempted } => { write(&[0, resource_code(*resource)]); write(&limit.to_be_bytes()); write(&attempted.to_be_bytes()); }, MeterRefusal::CounterOverflow { resource } => write(&[1, resource_code(*resource)]), MeterRefusal::FeeOverflow => write(&[2]) } }
