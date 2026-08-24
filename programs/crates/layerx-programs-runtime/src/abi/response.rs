//! Bounded successful response transport for the candidate ABI revision.

use core::fmt::{self, Display};

use crate::meter::MeterRefusal;

use super::{AbiValueType, HostFunction, HostFunctionType};

/// Explicitly non-current module carrying candidate response operations.
pub const CANDIDATE_ABI_MODULE: &str = "layerx_v2_candidate";
pub const CANDIDATE_ABI_MANIFEST: &str = "layerx_v2_candidate\0response_write(i32,i32,i32)->i32\0program_call_response(i32,i32,i32,i32,i32,i32,i32,i32)->i64\0refusal_write(i32,i32,i32)->i32\0storage_read_scoped(i32,i32,i32,i32,i32)->i32\0storage_write_scoped(i32,i32,i32,i32,i32)->i32\0storage_delete_scoped(i32,i32,i32)->i32\0storage_drop_scoped(i32)->i32\0storage_scan_scoped(i32,i32,i32,i32,i32,i32,i32,i32,i32)->i32\0transfer_program_402(i64,i64,i32,i32,i32,i32,i32,i32,i32,i32)->i32\0";

/// Exact qualification-only response extension table.
pub const CANDIDATE_HOST_FUNCTIONS: [HostFunction; 9] = [
    HostFunction {
        name: "response_write",
        signature: "(i32,i32,i32)->i32",
    },
    HostFunction {
        name: "program_call_response",
        signature: "(i32,i32,i32,i32,i32,i32,i32,i32)->i64",
    },
    HostFunction {
        name: "refusal_write",
        signature: "(i32,i32,i32)->i32",
    },
    HostFunction {
        name: "storage_read_scoped",
        signature: "(i32,i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "storage_write_scoped",
        signature: "(i32,i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "storage_delete_scoped",
        signature: "(i32,i32,i32)->i32",
    },
    HostFunction {
        name: "storage_drop_scoped",
        signature: "(i32)->i32",
    },
    HostFunction {
        name: "storage_scan_scoped",
        signature: "(i32,i32,i32,i32,i32,i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "transfer_program_402",
        signature: "(i64,i64,i32,i32,i32,i32,i32,i32,i32,i32)->i32",
    },
];

const RESPONSE_WRITE_PARAMS: &[AbiValueType] =
    &[AbiValueType::I32, AbiValueType::I32, AbiValueType::I32];
const PROGRAM_CALL_RESPONSE_PARAMS: &[AbiValueType] = &[AbiValueType::I32; 8];
const STORAGE_SCOPED_PARAMS: &[AbiValueType] = &[AbiValueType::I32; 5];
const STORAGE_SCAN_SCOPED_PARAMS: &[AbiValueType] = &[AbiValueType::I32; 9];
const PROGRAM_TRANSFER_PARAMS: &[AbiValueType] = &[
    AbiValueType::I64,
    AbiValueType::I64,
    AbiValueType::I32,
    AbiValueType::I32,
    AbiValueType::I32,
    AbiValueType::I32,
    AbiValueType::I32,
    AbiValueType::I32,
    AbiValueType::I32,
    AbiValueType::I32,
];
const I32_RESULT: &[AbiValueType] = &[AbiValueType::I32];
const I64_RESULT: &[AbiValueType] = &[AbiValueType::I64];
const RESPONSE_WRITE_TYPE: HostFunctionType = HostFunctionType {
    params: RESPONSE_WRITE_PARAMS,
    results: I32_RESULT,
};
const PROGRAM_CALL_RESPONSE_TYPE: HostFunctionType = HostFunctionType {
    params: PROGRAM_CALL_RESPONSE_PARAMS,
    results: I64_RESULT,
};
const REFUSAL_WRITE_TYPE: HostFunctionType = HostFunctionType {
    params: RESPONSE_WRITE_PARAMS,
    results: I32_RESULT,
};
const STORAGE_SCOPED_TYPE: HostFunctionType = HostFunctionType {
    params: STORAGE_SCOPED_PARAMS,
    results: I32_RESULT,
};
const STORAGE_SCAN_SCOPED_TYPE: HostFunctionType = HostFunctionType {
    params: STORAGE_SCAN_SCOPED_PARAMS,
    results: I32_RESULT,
};
const STORAGE_DELETE_SCOPED_TYPE: HostFunctionType = HostFunctionType {
    params: RESPONSE_WRITE_PARAMS,
    results: I32_RESULT,
};
const STORAGE_DROP_SCOPED_TYPE: HostFunctionType = HostFunctionType {
    params: &[AbiValueType::I32],
    results: I32_RESULT,
};
const PROGRAM_TRANSFER_TYPE: HostFunctionType = HostFunctionType {
    params: PROGRAM_TRANSFER_PARAMS,
    results: I32_RESULT,
};

pub(crate) fn candidate_function_type(name: &str) -> Option<&'static HostFunctionType> {
    match name {
        "response_write" => Some(&RESPONSE_WRITE_TYPE),
        "program_call_response" => Some(&PROGRAM_CALL_RESPONSE_TYPE),
        "refusal_write" => Some(&REFUSAL_WRITE_TYPE),
        "storage_read_scoped" | "storage_write_scoped" => Some(&STORAGE_SCOPED_TYPE),
        "storage_delete_scoped" => Some(&STORAGE_DELETE_SCOPED_TYPE),
        "storage_drop_scoped" => Some(&STORAGE_DROP_SCOPED_TYPE),
        "storage_scan_scoped" => Some(&STORAGE_SCAN_SCOPED_TYPE),
        "transfer_program_402" => Some(&PROGRAM_TRANSFER_TYPE),
        _ => None,
    }
}

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
