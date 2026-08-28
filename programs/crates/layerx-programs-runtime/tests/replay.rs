use layerx_programs_runtime::test_support::{
    add_module, code_section, export_section, func_body, function_section, import_section, module,
    padding_section, raw_section, type_section, unsigned_leb, OP_END, OP_I32_ADD, OP_LOCAL_GET,
    TYPE_I32,
};

fn imported_add(abi_module: &str, import: &str, import_arity: usize) -> Vec<u8> {
    let import_params = vec![TYPE_I32; import_arity];
    module(&[
        type_section(&[
            (&import_params, &[TYPE_I32]),
            (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
        ]),
        import_section(&[(abi_module, import, 0)]),
        function_section(&[1]),
        export_section(&[("add", 1)]),
        code_section(&[func_body(
            &[],
            &[OP_LOCAL_GET, 0, OP_LOCAL_GET, 1, OP_I32_ADD, OP_END],
        )]),
    ])
}
use layerx_programs_runtime::{
    hash_bytes, programs_differential_gate, programs_differential_gate_versioned,
    replay_recorded_execution, Deploy, Executor, HashAlgorithm, Lifecycle, LifecycleRefusal,
    ProgramId, RecordedExecution, ReplayRefusal, UpgradePolicy, ValidationLimits,
    ValidationRefusal, WasmEngine, WasmValue, ABI_V1_VERSION, ABI_V2_VERSION, ABI_VERSION,
    RUNTIME_VERSION,
};

#[test]
fn independent_engines_produce_identical_evidence() {
    let wasm = add_module();
    let evidence =
        programs_differential_gate(&wasm, "add", &[WasmValue::I32(20), WasmValue::I32(22)]);
    assert!(evidence.is_ok(), "independent runtime builds diverged");
}

#[test]
fn recorded_v1_replays_identically_after_a_simulated_upgrade() {
    let wasm = add_module();
    let record = RecordedExecution {
        runtime_version: RUNTIME_VERSION,
        abi_version: layerx_programs_runtime::ABI_V1_VERSION,
        fee_schedule_version: layerx_programs_runtime::FeeSchedule::declared().version(),
        metering_schedule_version: layerx_programs_runtime::meter::inject::GENESIS_METERING_SCHEDULE_VERSION,
        wasm: &wasm,
        export: "add",
        args: &[WasmValue::I32(20), WasmValue::I32(22)],
    };
    let before = replay_recorded_execution(&record);
    let after = replay_recorded_execution(&record);
    assert_eq!(before, after);
}

#[test]
fn mixed_v1_v2_history_selects_each_recorded_abi_and_fee_schedule() {
    let v1_wasm = imported_add("layerx_v1", "storage_delete", 2);
    let v2_wasm = imported_add("layerx_v2", "context_read", 3);
    let schedule = layerx_programs_runtime::FeeSchedule::declared().version();
    let history = [
        RecordedExecution {
            runtime_version: RUNTIME_VERSION,
            abi_version: layerx_programs_runtime::ABI_V1_VERSION,
            fee_schedule_version: schedule,
            metering_schedule_version: layerx_programs_runtime::meter::inject::GENESIS_METERING_SCHEDULE_VERSION,
            wasm: &v1_wasm,
            export: "add",
            args: &[WasmValue::I32(1), WasmValue::I32(2)],
        },
        RecordedExecution {
            runtime_version: RUNTIME_VERSION,
            abi_version: layerx_programs_runtime::ABI_V2_VERSION,
            fee_schedule_version: schedule,
            metering_schedule_version: layerx_programs_runtime::meter::inject::GENESIS_METERING_SCHEDULE_VERSION,
            wasm: &v2_wasm,
            export: "add",
            args: &[WasmValue::I32(3), WasmValue::I32(4)],
        },
    ];
    let evidence = history
        .iter()
        .map(replay_recorded_execution)
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|refusal| panic!("mixed-version replay refused: {refusal}"));
    assert_ne!(evidence[0], evidence[1]);

    let wrong_schedule = RecordedExecution {
        fee_schedule_version: schedule + 1,
        ..history[1].clone()
    };
    assert_eq!(
        replay_recorded_execution(&wrong_schedule),
        Err(ReplayRefusal::UnknownFeeScheduleVersion {
            version: schedule + 1
        })
    );
}

#[test]
fn unknown_runtime_and_abi_artifacts_are_preserved_without_execution() {
    let wasm = add_module();
    let runtime = RecordedExecution {
        runtime_version: RUNTIME_VERSION + 1,
        abi_version: ABI_VERSION,
        fee_schedule_version: layerx_programs_runtime::FeeSchedule::declared().version(),
        metering_schedule_version: layerx_programs_runtime::meter::inject::GENESIS_METERING_SCHEDULE_VERSION,
        wasm: &wasm,
        export: "add",
        args: &[],
    };
    assert_eq!(
        replay_recorded_execution(&runtime),
        Err(ReplayRefusal::UnknownRuntimeVersion {
            version: RUNTIME_VERSION + 1,
        })
    );
    let abi = RecordedExecution {
        runtime_version: RUNTIME_VERSION,
        abi_version: ABI_VERSION + 1,
        ..runtime
    };
    assert_eq!(
        replay_recorded_execution(&abi),
        Err(ReplayRefusal::UnknownAbiVersion {
            version: ABI_VERSION + 1
        })
    );
}

/// Committed differential corpus. Each vector executes once through the legacy
/// Wasmi 0.31.2 internal-fuel reference over the original bytes and once through
/// the production instrumented/private-hook path. Canonical outputs, refusals,
/// complete resource usage, and therefore CPU repricing MUST remain identical.
struct DifferentialVector {
    invariant: &'static str,
    abi: u16,
    wasm: Vec<u8>,
    export: &'static str,
    args: Vec<WasmValue>,
    expected: ExpectedObservation,
}

enum ExpectedObservation {
    Success {
        outputs: Vec<WasmValue>,
        memory_bytes: u64,
    },
    ExecutionRefusal {
        fault_contains: &'static str,
        charged: bool,
        memory_bytes: u64,
    },
    ValidationRefusal,
}

fn vector(
    invariant: &'static str,
    abi: u16,
    wasm: Vec<u8>,
    export: &'static str,
    args: Vec<WasmValue>,
    expected: ExpectedObservation,
) -> DifferentialVector {
    DifferentialVector {
        invariant,
        abi,
        wasm,
        export,
        args,
        expected,
    }
}

fn memory_section() -> Vec<u8> {
    raw_section(5, &[1, 0, 1])
}

fn memory_and_function_exports(function: u32, function_name: &str) -> Vec<u8> {
    let mut payload = vec![2];
    payload.extend(unsigned_leb(function_name.len() as u64));
    payload.extend_from_slice(function_name.as_bytes());
    payload.push(0);
    payload.extend(unsigned_leb(u64::from(function)));
    payload.extend_from_slice(&[6]);
    payload.extend_from_slice(b"memory");
    payload.push(2);
    payload.push(0);
    raw_section(7, &payload)
}

fn active_data(bytes: &[u8]) -> Vec<u8> {
    let mut payload = vec![1, 0, 0x41, 0, OP_END];
    payload.extend(unsigned_leb(bytes.len() as u64));
    payload.extend_from_slice(bytes);
    raw_section(11, &payload)
}

fn nullary_module(instructions: &[u8]) -> Vec<u8> {
    module(&[
        type_section(&[(&[], &[TYPE_I32])]),
        function_section(&[0]),
        export_section(&[("run", 0)]),
        code_section(&[func_body(&[], instructions)]),
    ])
}

fn internal_call_module() -> Vec<u8> {
    module(&[
        type_section(&[(&[], &[TYPE_I32])]),
        function_section(&[0, 0]),
        export_section(&[("run", 1)]),
        code_section(&[
            func_body(&[], &[0x41, 40, OP_END]),
            func_body(&[], &[0x10, 0, 0x41, 2, OP_I32_ADD, OP_END]),
        ]),
    ])
}

fn start_module(traps: bool) -> Vec<u8> {
    let start = if traps {
        vec![0x41, 1, 0x1a, 0x00, OP_END]
    } else {
        vec![0x41, 1, 0x1a, OP_END]
    };
    module(&[
        type_section(&[(&[], &[]), (&[], &[TYPE_I32])]),
        function_section(&[0, 1]),
        export_section(&[("run", 1)]),
        raw_section(8, &[0]),
        code_section(&[
            func_body(&[], &start),
            func_body(&[], &[0x41, 42, OP_END]),
        ]),
    ])
}

fn loop_module() -> Vec<u8> {
    module(&[
        type_section(&[(&[], &[TYPE_I32])]),
        function_section(&[0]),
        export_section(&[("run", 0)]),
        code_section(&[func_body(
            &[(1, TYPE_I32)],
            &[
                0x41, 3, 0x21, 0, 0x02, 0x40, 0x03, 0x40, 0x20, 0, 0x41, 1, 0x6b, 0x22,
                0, 0x0d, 0, OP_END, OP_END, 0x20, 0, OP_END,
            ],
        )]),
    ])
}

fn memory_module(instructions: &[u8], data: Option<&[u8]>) -> Vec<u8> {
    let mut sections = vec![
        type_section(&[(&[], &[TYPE_I32])]),
        function_section(&[0]),
        memory_section(),
        memory_and_function_exports(0, "run"),
        code_section(&[func_body(&[], instructions)]),
    ];
    if let Some(bytes) = data {
        sections.push(active_data(bytes));
    }
    module(&sections)
}

fn memory_grow_module() -> Vec<u8> {
    module(&[
        type_section(&[(&[TYPE_I32], &[TYPE_I32])]),
        function_section(&[0]),
        memory_section(),
        memory_and_function_exports(0, "run"),
        code_section(&[func_body(
            &[],
            &[OP_LOCAL_GET, 0, 0x40, 0, OP_END],
        )]),
    ])
}

fn hash_module() -> Vec<u8> {
    module(&[
        type_section(&[
            (&[TYPE_I32, TYPE_I32, TYPE_I32, TYPE_I32], &[TYPE_I32]),
            (&[], &[TYPE_I32]),
        ]),
        import_section(&[("layerx_v2", "hash", 0)]),
        function_section(&[1]),
        memory_section(),
        memory_and_function_exports(1, "run"),
        code_section(&[func_body(
            &[],
            &[0x41, 1, 0x41, 0, 0x41, 3, 0x41, 0xc0, 0, 0x10, 0, OP_END],
        )]),
        active_data(b"abc"),
    ])
}

fn table_copy_module() -> Vec<u8> {
    module(&[
        type_section(&[(&[], &[TYPE_I32])]),
        function_section(&[0]),
        // One MVP funcref table with two entries. Reference-types instructions
        // remain disabled; table.copy itself belongs to bulk memory/table.
        raw_section(4, &[1, 0x70, 0, 2]),
        export_section(&[("run", 0)]),
        // Active element segment initializes both source and destination slots.
        raw_section(9, &[1, 0, 0x41, 0, OP_END, 2, 0, 0]),
        code_section(&[func_body(
            &[],
            &[
                0x41, 1, 0x41, 0, 0x41, 1, 0xfc, 0x0e, 0, 0, 0x41, 42, OP_END,
            ],
        )]),
    ])
}

fn differential_vectors() -> Vec<DifferentialVector> {
    vec![
        vector(
            "V1 function entry, locals, arithmetic, and ordinary return",
            ABI_V1_VERSION,
            add_module(),
            "add",
            vec![WasmValue::I32(20), WasmValue::I32(22)],
            ExpectedObservation::Success { outputs: vec![WasmValue::I32(42)], memory_bytes: 0 },
        ),
        vector(
            "V2 executes inherited import-free code under its recorded ABI",
            ABI_V2_VERSION,
            add_module(),
            "add",
            vec![WasmValue::I32(-7), WasmValue::I32(7)],
            ExpectedObservation::Success { outputs: vec![WasmValue::I32(0)], memory_bytes: 0 },
        ),
        vector(
            "successful start-function work is charged before the export",
            ABI_V1_VERSION,
            start_module(false),
            "run",
            vec![],
            ExpectedObservation::Success { outputs: vec![WasmValue::I32(42)], memory_bytes: 0 },
        ),
        vector(
            "a start-function trap preserves the same refusal and charged prefix",
            ABI_V1_VERSION,
            start_module(true),
            "run",
            vec![],
            ExpectedObservation::ExecutionRefusal { fault_contains: "unreachable", charged: true, memory_bytes: 0 },
        ),
        vector(
            "a trap after arithmetic retains the same pre-trap CPU charge",
            ABI_V1_VERSION,
            nullary_module(&[0x41, 20, 0x41, 22, 0x6a, 0x1a, 0x00, OP_END]),
            "run",
            vec![],
            ExpectedObservation::ExecutionRefusal { fault_contains: "unreachable", charged: true, memory_bytes: 0 },
        ),
        vector(
            "an internal call charges both function entries and the call boundary",
            ABI_V1_VERSION,
            internal_call_module(),
            "run",
            vec![],
            ExpectedObservation::Success { outputs: vec![WasmValue::I32(42)], memory_bytes: 0 },
        ),
        vector(
            "loop backedges and br_if charge identically on every taken edge",
            ABI_V1_VERSION,
            loop_module(),
            "run",
            vec![],
            ExpectedObservation::Success { outputs: vec![WasmValue::I32(0)], memory_bytes: 0 },
        ),
        vector(
            "br_table selector handling and destination charging are identical",
            ABI_V1_VERSION,
            nullary_module(&[
                0x02, TYPE_I32, 0x41, 42, 0x41, 0, 0x0e, 1, 0, 0, OP_END, OP_END,
            ]),
            "run",
            vec![],
            ExpectedObservation::Success { outputs: vec![WasmValue::I32(42)], memory_bytes: 0 },
        ),
        vector(
            "memory store and load static/dynamic charges preserve the result",
            ABI_V1_VERSION,
            memory_module(
                &[0x41, 0, 0x41, 42, 0x36, 2, 0, 0x41, 0, 0x28, 2, 0, OP_END],
                None,
            ),
            "run",
            vec![],
            ExpectedObservation::Success { outputs: vec![WasmValue::I32(42)], memory_bytes: 65_536 },
        ),
        vector(
            "successful memory.grow commits its lazy size-dependent charge",
            ABI_V1_VERSION,
            memory_grow_module(),
            "run",
            vec![WasmValue::I32(1)],
            ExpectedObservation::Success { outputs: vec![WasmValue::I32(1)], memory_bytes: 131_072 },
        ),
        vector(
            "failed memory.grow returns minus one without committing a growth charge",
            ABI_V1_VERSION,
            memory_grow_module(),
            "run",
            vec![WasmValue::I32(-1)],
            ExpectedObservation::Success { outputs: vec![WasmValue::I32(-1)], memory_bytes: 65_536 },
        ),
        vector(
            "successful bulk memory.copy charges its requested byte length lazily",
            ABI_V1_VERSION,
            memory_module(
                &[
                    0x41, 8, 0x41, 0, 0x41, 3, 0xfc, 10, 0, 0, 0x41, 8, 0x2d, 0, 0,
                    OP_END,
                ],
                Some(b"abc"),
            ),
            "run",
            vec![],
            ExpectedObservation::Success { outputs: vec![WasmValue::I32(97)], memory_bytes: 65_536 },
        ),
        vector(
            "trapping bulk memory.copy does not commit its lazy success charge",
            ABI_V1_VERSION,
            memory_module(
                &[
                    0x41, 0xff, 0xff, 3, 0x41, 0, 0x41, 3, 0xfc, 10, 0, 0, 0x41, 0,
                    OP_END,
                ],
                Some(b"abc"),
            ),
            "run",
            vec![],
            ExpectedObservation::ExecutionRefusal { fault_contains: "out of bounds", charged: true, memory_bytes: 65_536 },
        ),
        vector(
            "successful table.copy charges its requested element count lazily",
            ABI_V1_VERSION,
            table_copy_module(),
            "run",
            vec![],
            ExpectedObservation::Success { outputs: vec![WasmValue::I32(42)], memory_bytes: 0 },
        ),
        vector(
            "V2 real hash import charges guest instructions and host CPU once",
            ABI_V2_VERSION,
            hash_module(),
            "run",
            vec![],
            ExpectedObservation::Success { outputs: vec![WasmValue::I32(0)], memory_bytes: 65_536 },
        ),
        vector(
            "missing exports produce the same typed execution refusal",
            ABI_V1_VERSION,
            add_module(),
            "absent",
            vec![],
            ExpectedObservation::ExecutionRefusal { fault_contains: "unknown export", charged: false, memory_bytes: 0 },
        ),
        vector(
            "malformed bytes produce the same validation refusal class",
            ABI_V1_VERSION,
            vec![0x00, 0x61, 0x73, 0x6d, 0x01],
            "add",
            vec![],
            ExpectedObservation::ValidationRefusal,
        ),
        vector(
            "V1 refuses a V2-only import before instantiation",
            ABI_V1_VERSION,
            hash_module(),
            "run",
            vec![],
            ExpectedObservation::ValidationRefusal,
        ),
    ]
}

fn take<const N: usize>(bytes: &[u8], cursor: &mut usize) -> [u8; N] {
    let end = cursor.checked_add(N).expect("evidence cursor overflow");
    let value: [u8; N] = bytes
        .get(*cursor..end)
        .expect("truncated differential evidence")
        .try_into()
        .expect("fixed-width evidence field");
    *cursor = end;
    value
}

#[derive(Debug, PartialEq, Eq)]
struct DecodedUsage {
    cpu_fuel: u64,
    memory_bytes: u64,
    storage_read_bytes: u64,
    storage_write_bytes: u64,
    output_values: u32,
    output_bytes: u64,
    occupancy_byte_batches: u128,
    occupancy_fee_units: u128,
    fee_units: u128,
}

fn decode_full_usage(evidence: &[u8], cursor: &mut usize) -> DecodedUsage {
    DecodedUsage {
        cpu_fuel: u64::from_be_bytes(take(evidence, cursor)),
        memory_bytes: u64::from_be_bytes(take(evidence, cursor)),
        storage_read_bytes: u64::from_be_bytes(take(evidence, cursor)),
        storage_write_bytes: u64::from_be_bytes(take(evidence, cursor)),
        output_values: u32::from_be_bytes(take(evidence, cursor)),
        output_bytes: u64::from_be_bytes(take(evidence, cursor)),
        occupancy_byte_batches: u128::from_be_bytes(take(evidence, cursor)),
        occupancy_fee_units: u128::from_be_bytes(take(evidence, cursor)),
        fee_units: u128::from_be_bytes(take(evidence, cursor)),
    }
}

fn assert_expected_observation(vector: &DifferentialVector, evidence: &[u8]) {
    match &vector.expected {
        ExpectedObservation::Success { outputs, memory_bytes } => {
            assert_eq!(evidence.first(), Some(&0x30), "{}: expected success", vector.invariant);
            let mut cursor = 1;
            let domain = b"LXP/program-execution/v2\0";
            assert_eq!(
                evidence.get(cursor..cursor + domain.len()),
                Some(domain.as_slice()),
                "{}: wrong evidence domain",
                vector.invariant,
            );
            cursor += domain.len();
            assert_eq!(u16::from_be_bytes(take(evidence, &mut cursor)), RUNTIME_VERSION);
            assert_eq!(u16::from_be_bytes(take(evidence, &mut cursor)), vector.abi);
            assert_eq!(
                u32::from_be_bytes(take(evidence, &mut cursor)),
                layerx_programs_runtime::meter::inject::GENESIS_METERING_SCHEDULE_VERSION,
            );
            let count = u128::from_be_bytes(take(evidence, &mut cursor));
            assert_eq!(count, outputs.len() as u128, "{}: wrong output count", vector.invariant);
            let mut observed_outputs = Vec::with_capacity(outputs.len());
            for _ in 0..outputs.len() {
                let tag = take::<1>(evidence, &mut cursor)[0];
                observed_outputs.push(match tag {
                    1 => WasmValue::I32(i32::from_be_bytes(take(evidence, &mut cursor))),
                    2 => WasmValue::I64(i64::from_be_bytes(take(evidence, &mut cursor))),
                    other => panic!("{}: unknown output tag {other}", vector.invariant),
                });
            }
            assert_eq!(&observed_outputs, outputs, "{}: wrong outputs", vector.invariant);
            let record_cpu = u64::from_be_bytes(take(evidence, &mut cursor));
            let record_memory = u64::from_be_bytes(take(evidence, &mut cursor));
            let record_storage_read = u64::from_be_bytes(take(evidence, &mut cursor));
            let record_storage_write = u64::from_be_bytes(take(evidence, &mut cursor));
            let record_output_values = u32::from_be_bytes(take(evidence, &mut cursor));
            let record_fee_units = u128::from_be_bytes(take(evidence, &mut cursor));
            let usage = decode_full_usage(evidence, &mut cursor);
            assert_eq!(usage.cpu_fuel, record_cpu, "{}: duplicated CPU disagreed", vector.invariant);
            assert_eq!(usage.memory_bytes, record_memory, "{}: duplicated memory disagreed", vector.invariant);
            assert_eq!(usage.storage_read_bytes, record_storage_read, "{}: duplicated reads disagreed", vector.invariant);
            assert_eq!(usage.storage_write_bytes, record_storage_write, "{}: duplicated writes disagreed", vector.invariant);
            assert_eq!(usage.output_values, record_output_values, "{}: duplicated outputs disagreed", vector.invariant);
            assert_eq!(usage.fee_units, record_fee_units, "{}: duplicated fees disagreed", vector.invariant);
            assert!(usage.cpu_fuel > 0, "{}: successful work was uncharged", vector.invariant);
            assert_eq!(usage.memory_bytes, *memory_bytes, "{}: wrong memory high-water mark", vector.invariant);
            assert_eq!(usage.storage_read_bytes, 0, "{}: unexpected storage read", vector.invariant);
            assert_eq!(usage.storage_write_bytes, 0, "{}: unexpected storage write", vector.invariant);
            assert_eq!(usage.output_values, outputs.len() as u32, "{}: wrong output usage", vector.invariant);
            assert_eq!(usage.output_bytes, 0, "{}: unexpected output bytes", vector.invariant);
            assert_eq!(usage.occupancy_byte_batches, 0, "{}: unexpected occupancy", vector.invariant);
            assert_eq!(usage.occupancy_fee_units, 0, "{}: unexpected occupancy fee", vector.invariant);
            assert_eq!(cursor, evidence.len(), "{}: trailing success evidence", vector.invariant);
        }
        ExpectedObservation::ExecutionRefusal { fault_contains, charged, memory_bytes } => {
            assert_eq!(evidence.first(), Some(&0x31), "{}: expected execution refusal", vector.invariant);
            let mut cursor = 1;
            let fault_len = u64::from_be_bytes(take(evidence, &mut cursor)) as usize;
            let fault_end = cursor.checked_add(fault_len).expect("fault length overflow");
            let fault = core::str::from_utf8(
                evidence.get(cursor..fault_end).expect("truncated refusal fault"),
            )
            .expect("refusal fault is UTF-8");
            cursor = fault_end;
            assert!(
                fault.contains(fault_contains),
                "{}: refusal `{fault}` did not contain `{fault_contains}`",
                vector.invariant,
            );
            let cpu_fuel = u64::from_be_bytes(take(evidence, &mut cursor));
            if *charged {
                assert!(cpu_fuel > 0, "{}: charged prefix was lost", vector.invariant);
            } else {
                assert_eq!(cpu_fuel, 0, "{}: refusal charged unexpected CPU", vector.invariant);
            }
            let exhaustion = take::<1>(evidence, &mut cursor)[0];
            if exhaustion == 1 {
                let length = u64::from_be_bytes(take(evidence, &mut cursor)) as usize;
                let end = cursor.checked_add(length).expect("exhaustion length overflow");
                let _reason = core::str::from_utf8(
                    evidence.get(cursor..end).expect("truncated exhaustion reason"),
                )
                .expect("exhaustion reason is UTF-8");
                cursor = end;
            } else {
                assert_eq!(exhaustion, 0, "{}: invalid exhaustion tag", vector.invariant);
            }
            assert_eq!(exhaustion, 0, "{}: unexpected resource exhaustion", vector.invariant);
            let raw_cpu = u64::from_be_bytes(take(evidence, &mut cursor));
            let raw_memory = u64::from_be_bytes(take(evidence, &mut cursor));
            let raw_storage_read = u64::from_be_bytes(take(evidence, &mut cursor));
            let raw_storage_write = u64::from_be_bytes(take(evidence, &mut cursor));
            let raw_output_values = u32::from_be_bytes(take(evidence, &mut cursor));
            let raw_output_bytes = u64::from_be_bytes(take(evidence, &mut cursor));
            assert_eq!(raw_cpu, cpu_fuel, "{}: refusal CPU snapshots disagreed", vector.invariant);
            assert_eq!(raw_memory, *memory_bytes, "{}: wrong refusal memory high-water mark", vector.invariant);
            assert_eq!(raw_storage_read, 0, "{}: refusal read storage", vector.invariant);
            assert_eq!(raw_storage_write, 0, "{}: refusal wrote storage", vector.invariant);
            assert_eq!(raw_output_values, 0, "{}: refusal returned values", vector.invariant);
            assert_eq!(raw_output_bytes, 0, "{}: refusal returned bytes", vector.invariant);
            let finalization = take::<1>(evidence, &mut cursor)[0];
            if finalization == 1 {
                let usage = decode_full_usage(evidence, &mut cursor);
                assert_eq!(usage.cpu_fuel, raw_cpu, "{}: finalized CPU disagreed", vector.invariant);
                assert_eq!(usage.memory_bytes, raw_memory, "{}: finalized memory disagreed", vector.invariant);
                assert_eq!(usage.storage_read_bytes, raw_storage_read, "{}: finalized reads disagreed", vector.invariant);
                assert_eq!(usage.storage_write_bytes, raw_storage_write, "{}: finalized writes disagreed", vector.invariant);
                assert_eq!(usage.output_values, raw_output_values, "{}: finalized outputs disagreed", vector.invariant);
                assert_eq!(usage.output_bytes, raw_output_bytes, "{}: finalized output bytes disagreed", vector.invariant);
                assert_eq!(usage.occupancy_byte_batches, 0, "{}: refusal recorded occupancy", vector.invariant);
                assert_eq!(usage.occupancy_fee_units, 0, "{}: refusal recorded occupancy fee", vector.invariant);
            } else {
                assert_eq!(finalization, 0, "{}: invalid finalization tag", vector.invariant);
                let length = u64::from_be_bytes(take(evidence, &mut cursor)) as usize;
                let end = cursor.checked_add(length).expect("finalization reason overflow");
                let reason = core::str::from_utf8(
                    evidence.get(cursor..end).expect("truncated finalization reason"),
                )
                .expect("finalization reason is UTF-8");
                cursor = end;
                panic!("{}: non-exhausted refusal failed usage finalization: {reason}", vector.invariant);
            }
            assert_eq!(cursor, evidence.len(), "{}: trailing refusal evidence", vector.invariant);
        }
        ExpectedObservation::ValidationRefusal => {
            assert_eq!(evidence.first(), Some(&0x11), "{}: expected validation refusal", vector.invariant);
            assert!(evidence.len() > 1, "{}: empty validation reason", vector.invariant);
        }
    }
}

#[test]
fn differential_gate_agrees_on_every_committed_vector() {
    for (index, vector) in differential_vectors().into_iter().enumerate() {
        match programs_differential_gate_versioned(
            vector.abi,
            &vector.wasm,
            vector.export,
            &vector.args,
        ) {
            Ok(evidence) => assert_expected_observation(&vector, &evidence),
            Err(mismatch) => panic!(
                "vector {index} ({}) diverged across builds: {mismatch:?}",
                vector.invariant
            ),
        }
    }
}

#[test]
fn differential_gate_evidence_is_reproducible_per_vector() {
    for (index, vector) in differential_vectors().into_iter().enumerate() {
        let first = programs_differential_gate_versioned(
            vector.abi,
            &vector.wasm,
            vector.export,
            &vector.args,
        );
        let second = programs_differential_gate_versioned(
            vector.abi,
            &vector.wasm,
            vector.export,
            &vector.args,
        );
        assert_eq!(
            first, second,
            "vector {index} ({}) was not reproducible",
            vector.invariant
        );
    }
}

fn engine_with(limits: ValidationLimits) -> WasmEngine {
    match WasmEngine::new(limits) {
        Ok(engine) => engine,
        Err(refusal) => panic!("engine construction refused: {refusal}"),
    }
}

fn limits(
    max_module_bytes: u64,
    max_functions: u32,
    max_value_stack_height: u32,
    max_call_depth: u32,
) -> ValidationLimits {
    match ValidationLimits::new(
        max_module_bytes,
        max_functions,
        max_value_stack_height,
        max_call_depth,
    ) {
        Ok(limits) => limits,
        Err(refusal) => panic!("limit construction refused: {refusal}"),
    }
}

fn program_id(byte: u8) -> ProgramId {
    match ProgramId::new([byte; 32]) {
        Ok(program) => program,
        Err(refusal) => panic!("program id refused: {refusal}"),
    }
}

fn deploy_activity(byte: u8, wasm: Vec<u8>) -> Deploy {
    let code_hash = hash_bytes(HashAlgorithm::Sha256, &wasm)
        .unwrap_or_else(|error| panic!("program code hash refused: {error}"));
    Deploy {
        program: program_id(byte),
        code_hash,
        wasm,
        abi_version: ABI_VERSION,
        upgrade_policy: UpgradePolicy::Immutable,
    }
}

#[test]
fn oversized_module_is_refused_at_deploy_time_with_a_typed_result() {
    let mut lifecycle = Lifecycle::new(engine_with(limits(64, 16, 1_024, 16)), Executor::declared());
    let wasm = module(&[padding_section(128)]);
    let refusal = lifecycle.deploy(deploy_activity(1, wasm.clone()));
    assert_eq!(
        refusal,
        Err(LifecycleRefusal::Validation(
            ValidationRefusal::ModuleTooLarge {
                byte_size: wasm.len() as u64,
                limit: 64,
            }
        ))
    );
    assert_eq!(
        lifecycle.diagnostics().len(),
        1,
        "the refused module must be preserved for diagnosis"
    );
}

#[test]
fn function_count_over_limit_is_refused_at_deploy_time_with_a_typed_result() {
    let mut lifecycle =
        Lifecycle::new(engine_with(limits(65_536, 4, 1_024, 16)), Executor::declared());
    let bodies: Vec<Vec<u8>> = (0..5).map(|_| func_body(&[], &[OP_END])).collect();
    let wasm = module(&[
        type_section(&[(&[], &[])]),
        function_section(&[0, 0, 0, 0, 0]),
        code_section(&bodies),
    ]);
    let refusal = lifecycle.deploy(deploy_activity(2, wasm));
    assert_eq!(
        refusal,
        Err(LifecycleRefusal::Validation(
            ValidationRefusal::TooManyFunctions {
                function_count: 5,
                limit: 4,
            }
        ))
    );
    assert_eq!(lifecycle.diagnostics().len(), 1);
}

#[test]
fn declared_validation_limits_expose_every_named_bound() {
    let declared = ValidationLimits::declared();
    assert!(declared.max_module_bytes() > 0);
    assert!(declared.max_functions() > 0);
    assert!(declared.max_value_stack_height() > 0);
    assert!(declared.max_call_depth() > 0);
}

#[test]
fn a_valid_module_deploys_under_declared_limits() {
    let mut lifecycle = match Lifecycle::declared() {
        Ok(lifecycle) => lifecycle,
        Err(refusal) => panic!("declared lifecycle refused: {refusal}"),
    };
    let receipt = match lifecycle.deploy(deploy_activity(3, add_module())) {
        Ok(receipt) => receipt,
        Err(refusal) => panic!("valid module refused at deploy: {refusal}"),
    };
    assert_eq!(receipt.version(), 1);
    assert!(lifecycle.diagnostics().is_empty());
}
