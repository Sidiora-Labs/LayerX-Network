//! Human-run interpreter-versus-compiled release gate over the production ABI-v2 executor.

use std::{env, fs, path::PathBuf, process, time::Instant};

use layerx_programs_runtime::{
    AuthorizationContext, AuthorizedExecutionRequest, CandidateActivityOutcome,
    CandidateAuthorizedExecutionRecord, Capability, CapabilitySet, CompositionContext, Executor,
    FeeSchedule, MeteredUsage, PrincipalId, ProgramId, ResourceBudget, Storage,
    UnavailableReceiptOracle, WasmEngine, CALL_ENTRY_EXPORT,
};

const PUBLISHED_TIME_MULTIPLIER_BPS: u128 = 120_000;
const PUBLISHED_FEE_MULTIPLIER_BPS: u128 = 120_000;
const TOLERANCE_BPS: u128 = 1_500;
const BPS: u128 = 10_000;
const DEFAULT_SAMPLES: usize = 31;
const SUCCESS_VECTORS: &[u8] =
    include_bytes!("../crates/layerx-programs-interpreter/vectors/v1-arithmetic.hex");

#[derive(Clone, Copy)]
struct Workload {
    name: &'static str,
    vector: usize,
    compiled_input: &'static [u8],
}

const WORKLOADS: [Workload; 4] = [
    Workload { name: "arithmetic-store", vector: 0, compiled_input: &[0] },
    Workload { name: "integer-suite", vector: 2, compiled_input: &[1] },
    Workload { name: "bounded-control", vector: 3, compiled_input: &[2] },
    Workload { name: "storage-control-transfer", vector: 1, compiled_input: &[3] },
];

fn required_path(name: &str) -> PathBuf {
    env::var_os(name).map(PathBuf::from).unwrap_or_else(|| panic!("{name} must name a built ABI-v2 Wasm artifact"))
}

fn decode_vectors() -> Vec<Vec<u8>> {
    fn nibble(byte: u8) -> u8 {
        match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            _ => panic!("non-hex interpreter vector"),
        }
    }
    std::str::from_utf8(SUCCESS_VECTORS)
        .unwrap_or_else(|error| panic!("interpreter vectors are not UTF-8: {error}"))
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            assert_eq!(line.len() % 2, 0, "odd-length interpreter vector");
            line.as_bytes().chunks_exact(2).map(|pair| (nibble(pair[0]) << 4) | nibble(pair[1])).collect()
        })
        .collect()
}

fn execute(module: &layerx_programs_runtime::CompiledModule, input: &[u8]) -> (Storage, CandidateAuthorizedExecutionRecord) {
    let program = ProgramId::new([0x35; 32]).unwrap_or_else(|error| panic!("program id: {error}"));
    let principal = PrincipalId::new([0x53; 32]).unwrap_or_else(|error| panic!("principal id: {error}"));
    let capabilities = CapabilitySet::new([
        Capability::StorageRead,
        Capability::StorageWrite,
        Capability::Transfer402 { asset: [1; 32], to: [2; 32], maximum_amount: 100 },
    ])
        .unwrap_or_else(|error| panic!("capabilities: {error}"));
    let mut storage = Storage::new();
    let record = Executor::new(
        ResourceBudget::new_complete(10_000_000, 16 * 1_024 * 1_024, 1_048_576, 1_048_576, 64, 1_048_576, 4_096),
        FeeSchedule::declared(),
    )
    .execute_authorized_candidate(
        &mut storage,
        AuthorizedExecutionRequest {
            module,
            program,
            authorization: AuthorizationContext::new(principal, capabilities),
            receipts: &UnavailableReceiptOracle,
            entrypoint: CALL_ENTRY_EXPORT,
            calldata: input,
            composition: CompositionContext::isolated(),
            response_capacity: 4,
        },
    )
    .unwrap_or_else(|error| panic!("production executor refused benchmark input: {error}"));
    (storage, record)
}

fn assert_equivalent(
    workload: &str,
    interpreted: &(Storage, CandidateAuthorizedExecutionRecord),
    compiled: &(Storage, CandidateAuthorizedExecutionRecord),
) {
    assert_eq!(interpreted.0, compiled.0, "{workload}: committed state differs");
    let left = interpreted.1.receipt_projection();
    let right = compiled.1.receipt_projection();
    assert_eq!(left.root_program(), right.root_program(), "{workload}: receipt program differs");
    assert_eq!(left.abi_revision(), right.abi_revision(), "{workload}: receipt ABI differs");
    assert_eq!(left.runtime_version(), right.runtime_version(), "{workload}: runtime version differs");
    assert_eq!(left.fee_schedule_version(), right.fee_schedule_version(), "{workload}: fee schedule differs");
    assert_eq!(left.metering_schedule_version(), right.metering_schedule_version(), "{workload}: metering schedule differs");
    assert_eq!(left.graph_evidence(), right.graph_evidence(), "{workload}: call graph differs");
    assert_eq!(left.outcome(), right.outcome(), "{workload}: receipt outcome differs modulo cost");
    match (interpreted.1.outcome(), compiled.1.outcome()) {
        (CandidateActivityOutcome::Success { effects: left, .. }, CandidateActivityOutcome::Success { effects: right, .. }) => {
            assert_eq!(left, right, "{workload}: effects differ");
        }
        _ => panic!("{workload}: representative workload did not succeed"),
    }
}

fn median(values: &mut [u128]) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn timed(samples: usize, mut operation: impl FnMut()) -> u128 {
    let mut elapsed = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        operation();
        elapsed.push(start.elapsed().as_nanos());
    }
    median(&mut elapsed)
}

fn print_usage(route: &str, workload: &str, usage: MeteredUsage) {
    println!(
        "route={route} workload={workload} cpu_fuel={} memory_bytes={} storage_read_bytes={} storage_write_bytes={} output_values={} output_bytes={} occupancy_byte_batches={} occupancy_fee_units={} fee_units={}",
        usage.cpu_fuel, usage.memory_bytes, usage.storage_read_bytes, usage.storage_write_bytes,
        usage.output_values, usage.output_bytes, usage.occupancy_byte_batches,
        usage.occupancy_fee_units, usage.fee_units
    );
}

fn programs_interpreter_bench() {
    let samples = env::var("LAYERX_INTERPRETER_BENCH_SAMPLES")
        .ok().and_then(|value| value.parse::<usize>().ok()).unwrap_or(DEFAULT_SAMPLES);
    assert!(samples >= 3 && samples % 2 == 1, "sample count must be an odd integer of at least three");
    let interpreter = fs::read(required_path("LAYERX_INTERPRETER_WASM"))
        .unwrap_or_else(|error| panic!("read interpreter artifact: {error}"));
    let compiled = fs::read(required_path("LAYERX_COMPILED_EQUIVALENT_WASM"))
        .unwrap_or_else(|error| panic!("read compiled-equivalent artifact: {error}"));
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("declared engine: {error}"));
    let interpreter = engine.validate_v2(&interpreter).unwrap_or_else(|error| panic!("validate interpreter: {error}"));
    let compiled = engine.validate_v2(&compiled).unwrap_or_else(|error| panic!("validate compiled equivalent: {error}"));
    let vectors = decode_vectors();
    let mut interpreted_total = 0_u128;
    let mut compiled_total = 0_u128;
    let mut interpreted_fee_units = 0_u128;
    let mut compiled_fee_units = 0_u128;
    for workload in WORKLOADS {
        let script = vectors.get(workload.vector).unwrap_or_else(|| panic!("missing vector {}", workload.vector));
        let interpreted_record = execute(&interpreter, script);
        let compiled_record = execute(&compiled, workload.compiled_input);
        assert_equivalent(workload.name, &interpreted_record, &compiled_record);
        print_usage("interpreted", workload.name, interpreted_record.1.execution().usage());
        print_usage("compiled", workload.name, compiled_record.1.execution().usage());
        interpreted_fee_units = interpreted_fee_units.checked_add(interpreted_record.1.execution().usage().fee_units).unwrap_or_else(|| panic!("interpreted fee overflow"));
        compiled_fee_units = compiled_fee_units.checked_add(compiled_record.1.execution().usage().fee_units).unwrap_or_else(|| panic!("compiled fee overflow"));
        let interpreted_ns = timed(samples, || { let _ = execute(&interpreter, script); });
        let compiled_ns = timed(samples, || { let _ = execute(&compiled, workload.compiled_input); });
        interpreted_total = interpreted_total.checked_add(interpreted_ns).unwrap_or_else(|| panic!("interpreted duration overflow"));
        compiled_total = compiled_total.checked_add(compiled_ns).unwrap_or_else(|| panic!("compiled duration overflow"));
        println!("timing workload={} interpreted_median_ns={} compiled_median_ns={}", workload.name, interpreted_ns, compiled_ns);
    }
    assert_ne!(compiled_total, 0, "compiled benchmark clock resolution is insufficient");
    assert_ne!(compiled_fee_units, 0, "compiled workload consumed zero protocol fee units");
    let observed_time_bps = interpreted_total.checked_mul(BPS).unwrap_or_else(|| panic!("time ratio overflow")) / compiled_total;
    let observed_fee_bps = interpreted_fee_units.checked_mul(BPS).unwrap_or_else(|| panic!("fee ratio overflow")) / compiled_fee_units;
    let time_gate_bps = PUBLISHED_TIME_MULTIPLIER_BPS.checked_mul(BPS + TOLERANCE_BPS).unwrap_or_else(|| panic!("time gate overflow")) / BPS;
    let fee_gate_bps = PUBLISHED_FEE_MULTIPLIER_BPS.checked_mul(BPS + TOLERANCE_BPS).unwrap_or_else(|| panic!("fee gate overflow")) / BPS;
    println!("interpreter_time_overhead observed_bps={observed_time_bps} published_bps={PUBLISHED_TIME_MULTIPLIER_BPS} tolerance_bps={TOLERANCE_BPS} gate_bps={time_gate_bps} samples={samples}");
    println!("interpreter_fee_overhead observed_bps={observed_fee_bps} published_bps={PUBLISHED_FEE_MULTIPLIER_BPS} tolerance_bps={TOLERANCE_BPS} gate_bps={fee_gate_bps}");
    if observed_time_bps > time_gate_bps || observed_fee_bps > fee_gate_bps {
        eprintln!("interpreter time or protocol-fee overhead exceeded its published multiplier plus tolerance");
        process::exit(1);
    }
}

fn main() {
    programs_interpreter_bench();
}
