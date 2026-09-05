//! Frozen ABI-v1 and ABI-v2 vocabulary shared with the programs runtime.
//!
//! Every constant here mirrors `layerx-programs-runtime` exactly. The
//! determinism lint compares the two surfaces and fails a build the moment
//! they drift, so a program can never be compiled against a stale manifest.

/// Host module every program imports.
pub const ABI_MODULE: &str = "layerx_v1";

/// Current ABI version frozen by the runtime. Version-one imports remain
/// available through `ABI_MODULE` for historical modules.
pub const ABI_VERSION: u16 = 2;

/// Canonical export name the runtime invokes on a program.
pub const ENTRYPOINT: &str = "layerx_main";

/// Export a composable program provides as its program-to-program call entry
/// point. It takes the input pointer and length and returns a non-negative
/// result code.
pub const CALL_ENTRY_EXPORT: &str = "layerx_call";

/// Export a composable program provides to reserve a bounded input region in
/// its own linear memory. It takes a length and returns a pointer.
pub const CALL_RESERVE_EXPORT: &str = "layerx_reserve";

/// Export name of the linear memory every program exposes to the host.
pub const MEMORY_EXPORT: &str = "memory";

/// Frozen version-one host-function surface. Signatures use WebAssembly value
/// names, and all values are integer-only.
pub const ABI_V1_MANIFEST: &str = "layerx_v1\0storage_read(i32,i32,i32,i32)->i32\0storage_write(i32,i32,i32,i32)->i32\0storage_delete(i32,i32)->i32\0event_emit(i32,i32,i32,i32)->i32\0program_call(i32,i32,i32,i32,i32,i32)->i32\0transfer_402(i64,i64,i32,i32,i32,i32)->i32\0receipt_read(i32,i32,i32,i32)->i32\0";

/// Maximum key length admitted by the version-one storage ABI.
pub const MAX_STORAGE_KEY_BYTES: usize = 256;
/// Maximum value length admitted by the version-one storage ABI.
pub const MAX_STORAGE_VALUE_BYTES: usize = 1_048_576;
/// Maximum event topic length admitted by the version-one ABI.
pub const MAX_EVENT_TOPIC_BYTES: usize = 64;
/// Maximum event payload length admitted by the version-one ABI.
pub const MAX_EVENT_DATA_BYTES: usize = 65_536;
/// Runtime-enforced aggregate event count across one activity call graph.
pub const MAX_EVENTS_PER_ACTIVITY: usize = 64;
/// Maximum call input length admitted by the version-one ABI.
pub const MAX_CALL_INPUT_BYTES: usize = 1_048_576;
/// Frozen ABI-v2 host module for response operations.
pub const V2_ABI_MODULE: &str = "layerx_v2";
/// Maximum ABI-v2 successful response payload.
pub const MAX_CALL_RESPONSE_BYTES: usize = 1_048_576;
/// Maximum seed length for an ABI-v2 program-owned account.
pub const MAX_PROGRAM_ACCOUNT_SEED_BYTES: usize = 128;
/// Maximum ABI-v2 program-refusal reason payload.
pub const MAX_REFUSAL_REASON_BYTES: usize = 4_096;
/// ABI-v2 entry return for a published refusal.
pub const V2_REFUSAL_SENTINEL: i32 = -64;
/// Exact frozen ABI-v2 manifest.
pub const V2_ABI_MANIFEST: &str = "layerx_v1\0storage_read(i32,i32,i32,i32)->i32\0storage_write(i32,i32,i32,i32)->i32\0storage_delete(i32,i32)->i32\0event_emit(i32,i32,i32,i32)->i32\0program_call(i32,i32,i32,i32,i32,i32)->i32\0transfer_402(i64,i64,i32,i32,i32,i32)->i32\0receipt_read(i32,i32,i32,i32)->i32\0layerx_v2\0response_write(i32,i32,i32)->i32\0program_call_response(i32,i32,i32,i32,i32,i32,i32,i32)->i64\0refusal_write(i32,i32,i32)->i32\0storage_read_scoped(i32,i32,i32,i32,i32)->i32\0storage_write_scoped(i32,i32,i32,i32,i32)->i32\0storage_delete_scoped(i32,i32,i32)->i32\0storage_drop_scoped(i32)->i32\0storage_scan_scoped(i32,i32,i32,i32,i32,i32,i32,i32,i32)->i32\0transfer_program_402(i64,i64,i32,i32,i32,i32,i32,i32,i32,i32)->i32\0fund_program_402(i64,i64,i32,i32,i32,i32,i32,i32)->i32\0context_read(i32,i32,i32)->i32\0balance_read(i32,i32,i32,i32,i32,i32)->i32\0hash(i32,i32,i32,i32)->i32\0signature_verify(i32,i32,i32,i32,i32,i32,i32)->i32\0signature_recover(i32,i32,i32,i32,i32,i32,i32)->i32\0bigint_mul_256(i32,i32,i32,i32,i32,i32)->i32\0bigint_div_256(i32,i32,i32,i32,i32,i32)->i32\0bigint_rem_256(i32,i32,i32,i32,i32,i32)->i32\0bigint_modexp_256(i32,i32,i32,i32,i32,i32,i32,i32)->i32\0";
pub const ABI_MANIFEST: &str = V2_ABI_MANIFEST;
/// Exact frozen ABI-v2 response extension table.
pub const V2_HOST_FUNCTIONS: [HostFunction; 19] = [
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
    HostFunction {
        name: "fund_program_402",
        signature: "(i64,i64,i32,i32,i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "context_read",
        signature: "(i32,i32,i32)->i32",
    },
    HostFunction {
        name: "balance_read",
        signature: "(i32,i32,i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "hash",
        signature: "(i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "signature_verify",
        signature: "(i32,i32,i32,i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "signature_recover",
        signature: "(i32,i32,i32,i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "bigint_mul_256",
        signature: "(i32,i32,i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "bigint_div_256",
        signature: "(i32,i32,i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "bigint_rem_256",
        signature: "(i32,i32,i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "bigint_modexp_256",
        signature: "(i32,i32,i32,i32,i32,i32,i32,i32)->i32",
    },
];
/// Maximum number of grants in one capability set.
pub const MAX_CAPABILITY_ENCODING_HEADER_BYTES: usize = 2;
pub const MAX_CAPABILITY_ENCODING_GRANT_BYTES: usize =
    1 + 32 + 2 + MAX_PROGRAM_ACCOUNT_SEED_BYTES + 32 + 32 + 32 + 16;
/// Maximum encoded capability-list length the host will read.
pub const MAX_CAPABILITY_ENCODING_BYTES: usize = 65_535;
pub const MAX_CAPABILITIES: usize = (MAX_CAPABILITY_ENCODING_BYTES
    - MAX_CAPABILITY_ENCODING_HEADER_BYTES)
    / MAX_CAPABILITY_ENCODING_GRANT_BYTES;
pub const MAX_CANONICAL_CAPABILITY_SET_BYTES: usize =
    MAX_CAPABILITY_ENCODING_HEADER_BYTES + MAX_CAPABILITIES * MAX_CAPABILITY_ENCODING_GRANT_BYTES;
/// Exact length of the encoded receipt view returned by `receipt_read`.
pub const RECEIPT_ENCODING_BYTES: usize = 116;

/// One entry of the frozen host-function surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostFunction {
    /// Name of the imported host function.
    pub name: &'static str,
    /// WebAssembly signature of the imported host function.
    pub signature: &'static str,
}

/// The seven host functions a version-one program may import.
pub const HOST_FUNCTIONS: [HostFunction; 7] = [
    HostFunction {
        name: "storage_read",
        signature: "(i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "storage_write",
        signature: "(i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "storage_delete",
        signature: "(i32,i32)->i32",
    },
    HostFunction {
        name: "event_emit",
        signature: "(i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "program_call",
        signature: "(i32,i32,i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "transfer_402",
        signature: "(i64,i64,i32,i32,i32,i32)->i32",
    },
    HostFunction {
        name: "receipt_read",
        signature: "(i32,i32,i32,i32)->i32",
    },
];

/// Compatibility spelling retained for one release.
pub const CANDIDATE_ABI_MODULE: &str = V2_ABI_MODULE;
/// Compatibility spelling retained for one release.
pub const CANDIDATE_REFUSAL_SENTINEL: i32 = V2_REFUSAL_SENTINEL;
/// Compatibility spelling retained for one release.
pub const CANDIDATE_ABI_MANIFEST: &str = V2_ABI_MANIFEST;
/// Compatibility spelling retained for one release.
pub const CANDIDATE_HOST_FUNCTIONS: [HostFunction; 19] = V2_HOST_FUNCTIONS;

#[cfg(test)]
mod tests {
    use super::V2_HOST_FUNCTIONS;

    #[test]
    fn compatibility_constants_preserve_frozen_v2_bytes() {
        assert_eq!(super::CANDIDATE_ABI_MODULE, super::V2_ABI_MODULE);
        assert_eq!(super::CANDIDATE_ABI_MANIFEST, super::V2_ABI_MANIFEST);
        assert_eq!(super::ABI_MANIFEST, super::V2_ABI_MANIFEST);
        assert_eq!(
            super::CANDIDATE_REFUSAL_SENTINEL,
            super::V2_REFUSAL_SENTINEL
        );
        assert_eq!(super::CANDIDATE_HOST_FUNCTIONS, super::V2_HOST_FUNCTIONS);
    }

    #[test]
    fn v2_response_table_has_exact_names_and_signatures() {
        let expected = [
            ("response_write", "(i32,i32,i32)->i32"),
            (
                "program_call_response",
                "(i32,i32,i32,i32,i32,i32,i32,i32)->i64",
            ),
            ("refusal_write", "(i32,i32,i32)->i32"),
            ("storage_read_scoped", "(i32,i32,i32,i32,i32)->i32"),
            ("storage_write_scoped", "(i32,i32,i32,i32,i32)->i32"),
            ("storage_delete_scoped", "(i32,i32,i32)->i32"),
            ("storage_drop_scoped", "(i32)->i32"),
            (
                "storage_scan_scoped",
                "(i32,i32,i32,i32,i32,i32,i32,i32,i32)->i32",
            ),
            (
                "transfer_program_402",
                "(i64,i64,i32,i32,i32,i32,i32,i32,i32,i32)->i32",
            ),
            ("fund_program_402", "(i64,i64,i32,i32,i32,i32,i32,i32)->i32"),
            ("context_read", "(i32,i32,i32)->i32"),
            ("balance_read", "(i32,i32,i32,i32,i32,i32)->i32"),
            ("hash", "(i32,i32,i32,i32)->i32"),
            ("signature_verify", "(i32,i32,i32,i32,i32,i32,i32)->i32"),
            ("signature_recover", "(i32,i32,i32,i32,i32,i32,i32)->i32"),
            ("bigint_mul_256", "(i32,i32,i32,i32,i32,i32)->i32"),
            ("bigint_div_256", "(i32,i32,i32,i32,i32,i32)->i32"),
            ("bigint_rem_256", "(i32,i32,i32,i32,i32,i32)->i32"),
            (
                "bigint_modexp_256",
                "(i32,i32,i32,i32,i32,i32,i32,i32)->i32",
            ),
        ];
        assert_eq!(V2_HOST_FUNCTIONS.len(), expected.len());
        for (actual, expected) in V2_HOST_FUNCTIONS.iter().zip(expected) {
            assert_eq!((actual.name, actual.signature), expected);
        }
    }
}
