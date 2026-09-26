//! Frozen, version-addressed Programs ABI declarations.

use super::{AbiValueType, HostFunction, HostFunctionType};

pub const ABI_V1_VERSION: u16 = 1;
pub const ABI_V2_VERSION: u16 = 2;
pub const ABI_V3_VERSION: u16 = 3;
pub const ABI_V4_VERSION: u16 = crate::ABI_VERSION;
pub const ABI_V1_MODULE: &str = "layerx_v1";
pub const ABI_V2_MODULE: &str = "layerx_v2";
pub const ABI_V3_MODULE: &str = "layerx_v3";
pub const ABI_V4_MODULE: &str = "layerx_v4";

// This value is the originally published v1 byte string. It must never be
// regenerated from the current host linker because later linkers deliberately
// contain more functions.
pub const ABI_V1_MANIFEST: &str = "layerx_v1\0storage_read(i32,i32,i32,i32)->i32\0storage_write(i32,i32,i32,i32)->i32\0storage_delete(i32,i32)->i32\0event_emit(i32,i32,i32,i32)->i32\0program_call(i32,i32,i32,i32,i32,i32)->i32\0transfer_402(i64,i64,i32,i32,i32,i32)->i32\0receipt_read(i32,i32,i32,i32)->i32\0";

// This value is the originally published v2 byte string. It carries the
// immutable v1 namespace followed by the frozen v2 namespace and nothing else.
pub const ABI_V2_MANIFEST: &str = "layerx_v1\0storage_read(i32,i32,i32,i32)->i32\0storage_write(i32,i32,i32,i32)->i32\0storage_delete(i32,i32)->i32\0event_emit(i32,i32,i32,i32)->i32\0program_call(i32,i32,i32,i32,i32,i32)->i32\0transfer_402(i64,i64,i32,i32,i32,i32)->i32\0receipt_read(i32,i32,i32,i32)->i32\0layerx_v2\0response_write(i32,i32,i32)->i32\0program_call_response(i32,i32,i32,i32,i32,i32,i32,i32)->i64\0refusal_write(i32,i32,i32)->i32\0storage_read_scoped(i32,i32,i32,i32,i32)->i32\0storage_write_scoped(i32,i32,i32,i32,i32)->i32\0storage_delete_scoped(i32,i32,i32)->i32\0storage_drop_scoped(i32)->i32\0storage_scan_scoped(i32,i32,i32,i32,i32,i32,i32,i32,i32)->i32\0transfer_program_402(i64,i64,i32,i32,i32,i32,i32,i32,i32,i32)->i32\0fund_program_402(i64,i64,i32,i32,i32,i32,i32,i32)->i32\0context_read(i32,i32,i32)->i32\0balance_read(i32,i32,i32,i32,i32,i32)->i32\0hash(i32,i32,i32,i32)->i32\0signature_verify(i32,i32,i32,i32,i32,i32,i32)->i32\0signature_recover(i32,i32,i32,i32,i32,i32,i32)->i32\0bigint_mul_256(i32,i32,i32,i32,i32,i32)->i32\0bigint_div_256(i32,i32,i32,i32,i32,i32)->i32\0bigint_rem_256(i32,i32,i32,i32,i32,i32)->i32\0bigint_modexp_256(i32,i32,i32,i32,i32,i32,i32,i32)->i32\0";

// This value is the originally published v3 byte string. It carries the
// immutable v1 and v2 namespaces followed by the frozen v3 namespace.
pub const ABI_V3_MANIFEST: &str = "layerx_v1\0storage_read(i32,i32,i32,i32)->i32\0storage_write(i32,i32,i32,i32)->i32\0storage_delete(i32,i32)->i32\0event_emit(i32,i32,i32,i32)->i32\0program_call(i32,i32,i32,i32,i32,i32)->i32\0transfer_402(i64,i64,i32,i32,i32,i32)->i32\0receipt_read(i32,i32,i32,i32)->i32\0layerx_v2\0response_write(i32,i32,i32)->i32\0program_call_response(i32,i32,i32,i32,i32,i32,i32,i32)->i64\0refusal_write(i32,i32,i32)->i32\0storage_read_scoped(i32,i32,i32,i32,i32)->i32\0storage_write_scoped(i32,i32,i32,i32,i32)->i32\0storage_delete_scoped(i32,i32,i32)->i32\0storage_drop_scoped(i32)->i32\0storage_scan_scoped(i32,i32,i32,i32,i32,i32,i32,i32,i32)->i32\0transfer_program_402(i64,i64,i32,i32,i32,i32,i32,i32,i32,i32)->i32\0fund_program_402(i64,i64,i32,i32,i32,i32,i32,i32)->i32\0context_read(i32,i32,i32)->i32\0balance_read(i32,i32,i32,i32,i32,i32)->i32\0hash(i32,i32,i32,i32)->i32\0signature_verify(i32,i32,i32,i32,i32,i32,i32)->i32\0signature_recover(i32,i32,i32,i32,i32,i32,i32)->i32\0bigint_mul_256(i32,i32,i32,i32,i32,i32)->i32\0bigint_div_256(i32,i32,i32,i32,i32,i32)->i32\0bigint_rem_256(i32,i32,i32,i32,i32,i32)->i32\0bigint_modexp_256(i32,i32,i32,i32,i32,i32,i32,i32)->i32\0layerx_v3\0oracle_read(i32,i32,i32,i32)->i32\0";

pub const ABI_V4_MANIFEST: &str = crate::ABI_MANIFEST;

pub const ABI_V2_HOST_FUNCTIONS: [HostFunction; 19] = [
    host("response_write", "(i32,i32,i32)->i32"),
    host(
        "program_call_response",
        "(i32,i32,i32,i32,i32,i32,i32,i32)->i64",
    ),
    host("refusal_write", "(i32,i32,i32)->i32"),
    host("storage_read_scoped", "(i32,i32,i32,i32,i32)->i32"),
    host("storage_write_scoped", "(i32,i32,i32,i32,i32)->i32"),
    host("storage_delete_scoped", "(i32,i32,i32)->i32"),
    host("storage_drop_scoped", "(i32)->i32"),
    host(
        "storage_scan_scoped",
        "(i32,i32,i32,i32,i32,i32,i32,i32,i32)->i32",
    ),
    host(
        "transfer_program_402",
        "(i64,i64,i32,i32,i32,i32,i32,i32,i32,i32)->i32",
    ),
    host("fund_program_402", "(i64,i64,i32,i32,i32,i32,i32,i32)->i32"),
    host("context_read", "(i32,i32,i32)->i32"),
    host("balance_read", "(i32,i32,i32,i32,i32,i32)->i32"),
    host("hash", "(i32,i32,i32,i32)->i32"),
    host("signature_verify", "(i32,i32,i32,i32,i32,i32,i32)->i32"),
    host("signature_recover", "(i32,i32,i32,i32,i32,i32,i32)->i32"),
    host("bigint_mul_256", "(i32,i32,i32,i32,i32,i32)->i32"),
    host("bigint_div_256", "(i32,i32,i32,i32,i32,i32)->i32"),
    host("bigint_rem_256", "(i32,i32,i32,i32,i32,i32)->i32"),
    host(
        "bigint_modexp_256",
        "(i32,i32,i32,i32,i32,i32,i32,i32)->i32",
    ),
];

pub const ABI_V3_HOST_FUNCTIONS: [HostFunction; 1] =
    [host("oracle_read", "(i32,i32,i32,i32)->i32")];

pub const ABI_V4_HOST_FUNCTIONS: [HostFunction; 1] = [host("web_read", "(i32,i32,i32,i32)->i32")];

const fn host(name: &'static str, signature: &'static str) -> HostFunction {
    HostFunction { name, signature }
}

const I32: AbiValueType = AbiValueType::I32;
const I64: AbiValueType = AbiValueType::I64;
const I32_RESULT: &[AbiValueType] = &[I32];
const I64_RESULT: &[AbiValueType] = &[I64];
const I32_1: &[AbiValueType] = &[I32; 1];
const I32_3: &[AbiValueType] = &[I32; 3];
const I32_4: &[AbiValueType] = &[I32; 4];
const I32_5: &[AbiValueType] = &[I32; 5];
const I32_6: &[AbiValueType] = &[I32; 6];
const I32_7: &[AbiValueType] = &[I32; 7];
const I32_8: &[AbiValueType] = &[I32; 8];
const I32_9: &[AbiValueType] = &[I32; 9];
const TRANSFER: &[AbiValueType] = &[I64, I64, I32, I32, I32, I32, I32, I32, I32, I32];
const FUND: &[AbiValueType] = &[I64, I64, I32, I32, I32, I32, I32, I32];

const ABI_V2_FUNCTION_TYPES: [HostFunctionType; 19] = [
    function_type(I32_3, I32_RESULT),
    function_type(I32_8, I64_RESULT),
    function_type(I32_3, I32_RESULT),
    function_type(I32_5, I32_RESULT),
    function_type(I32_5, I32_RESULT),
    function_type(I32_3, I32_RESULT),
    function_type(I32_1, I32_RESULT),
    function_type(I32_9, I32_RESULT),
    function_type(TRANSFER, I32_RESULT),
    function_type(FUND, I32_RESULT),
    function_type(I32_3, I32_RESULT),
    function_type(I32_6, I32_RESULT),
    function_type(I32_4, I32_RESULT),
    function_type(I32_7, I32_RESULT),
    function_type(I32_7, I32_RESULT),
    function_type(I32_6, I32_RESULT),
    function_type(I32_6, I32_RESULT),
    function_type(I32_6, I32_RESULT),
    function_type(I32_8, I32_RESULT),
];

const ABI_V3_FUNCTION_TYPES: [HostFunctionType; 1] = [function_type(I32_4, I32_RESULT)];

const ABI_V4_FUNCTION_TYPES: [HostFunctionType; 1] = [function_type(I32_4, I32_RESULT)];

const fn function_type(
    params: &'static [AbiValueType],
    results: &'static [AbiValueType],
) -> HostFunctionType {
    HostFunctionType { params, results }
}

pub(crate) fn v2_function_type(name: &str) -> Option<HostFunctionType> {
    ABI_V2_HOST_FUNCTIONS
        .iter()
        .position(|function| function.name == name)
        .map(|index| ABI_V2_FUNCTION_TYPES[index])
}

pub(crate) fn v3_function_type(name: &str) -> Option<HostFunctionType> {
    ABI_V3_HOST_FUNCTIONS
        .iter()
        .position(|function| function.name == name)
        .map(|index| ABI_V3_FUNCTION_TYPES[index])
}

pub(crate) fn v4_function_type(name: &str) -> Option<HostFunctionType> {
    ABI_V4_HOST_FUNCTIONS
        .iter()
        .position(|function| function.name == name)
        .map(|index| ABI_V4_FUNCTION_TYPES[index])
}

/// Returns the exact permitted import declaration for a recorded ABI. V2
/// inherits the immutable v1 namespace and adds only the v2 namespace; V3
/// inherits both and adds only the v3 namespace; V4 inherits all three and
/// adds only the v4 namespace.
pub(crate) fn permitted_import(version: u16, module: &str, name: &str) -> Option<HostFunctionType> {
    if module == ABI_V1_MODULE {
        let index = super::HOST_FUNCTIONS
            .iter()
            .position(|function| function.name == name)?;
        return (version == ABI_V1_VERSION
            || version == ABI_V2_VERSION
            || version == ABI_V3_VERSION
            || version == ABI_V4_VERSION)
            .then_some(super::HOST_FUNCTION_TYPES[index]);
    }
    if module == ABI_V2_MODULE {
        return (version == ABI_V2_VERSION
            || version == ABI_V3_VERSION
            || version == ABI_V4_VERSION)
            .then(|| v2_function_type(name))
            .flatten();
    }
    if module == ABI_V3_MODULE {
        return (version == ABI_V3_VERSION || version == ABI_V4_VERSION)
            .then(|| v3_function_type(name))
            .flatten();
    }
    (version == ABI_V4_VERSION && module == ABI_V4_MODULE)
        .then(|| v4_function_type(name))
        .flatten()
}

#[must_use]
pub const fn manifest(version: u16) -> Option<&'static str> {
    match version {
        ABI_V1_VERSION => Some(ABI_V1_MANIFEST),
        ABI_V2_VERSION => Some(ABI_V2_MANIFEST),
        ABI_V3_VERSION => Some(ABI_V3_MANIFEST),
        ABI_V4_VERSION => Some(ABI_V4_MANIFEST),
        _ => None,
    }
}
