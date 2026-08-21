//! Build-time determinism lint for `LayerX` programs.
//!
//! A program is refused before deployment ever sees it when it reaches for a
//! clock, for randomness, for floating point, or for any host interface
//! outside the frozen `layerx_v1` surface. Every refusal is named, so a broken
//! build says which rule was violated rather than which byte offset failed.
//!
//! The lint reuses the runtime's own deterministic-subset validator for the
//! module-level rules, so the toolchain and the node can never disagree about
//! what is admissible.

use std::fmt::{self, Display};
use std::fs;
use std::path::{Path, PathBuf};

use layerx_program_sdk::{CALL_ENTRY_EXPORT, CALL_RESERVE_EXPORT, MEMORY_EXPORT};
use layerx_programs_runtime::{ValidationRefusal, WasmEngine};
use wasmparser_nostd::{ExternalKind, Parser, Payload};

const CLOCK_IMPORT_NAMES: &[&str] = &[
    "clock_time_get",
    "clock_res_get",
    "clock_gettime",
    "gettimeofday",
    "date_now",
    "now",
    "time",
    "times",
];

const RANDOMNESS_IMPORT_NAMES: &[&str] = &[
    "random_get",
    "getrandom",
    "get_random_values",
    "random",
    "sched_yield",
];

const FORBIDDEN_DEPENDENCIES: &[&str] = &[
    "chrono",
    "float-cmp",
    "fastrand",
    "getrandom",
    "instant",
    "js-sys",
    "libm",
    "mio",
    "ordered-float",
    "rand",
    "rand_core",
    "socket2",
    "time",
    "tokio",
    "wasi",
    "wasm-bindgen",
    "web-sys",
];

const FORBIDDEN_SOURCE_ITEMS: &[&str] = &[
    "std::time::",
    "SystemTime::now",
    "Instant::now",
    "getrandom",
    "thread_rng",
    "OsRng",
    "rand::",
    "libm::",
    "wasi::",
    "wasm_bindgen",
];

const FLOAT_TYPE_NAMES: &[&str] = &["f32", "f64"];

/// A named determinism rule a program violated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeterminismViolation {
    /// The module imports a clock.
    ClockImport {
        /// Module field of the refused import.
        import_module: String,
        /// Name field of the refused import.
        import_name: String,
    },
    /// The module imports a source of randomness.
    RandomnessImport {
        /// Module field of the refused import.
        import_module: String,
        /// Name field of the refused import.
        import_name: String,
    },
    /// The module imports a host interface outside the frozen ABI.
    UndeclaredHostImport {
        /// Module field of the refused import.
        import_module: String,
        /// Name field of the refused import.
        import_name: String,
    },
    /// The module declares a floating-point type.
    FloatingPointType,
    /// The module contains a floating-point instruction.
    FloatingPointInstruction,
    /// The module declares a vector type.
    VectorType,
    /// The module exceeds the declared byte-size limit.
    ModuleTooLarge {
        /// Byte size of the refused module.
        byte_size: u64,
        /// Declared byte-size limit.
        limit: u64,
    },
    /// The module declares more functions than the declared limit allows.
    TooManyFunctions {
        /// Number of functions the module declares.
        function_count: u32,
        /// Declared function-count limit.
        limit: u32,
    },
    /// The module omits an export the runtime requires.
    MissingExport {
        /// Export the runtime could not find.
        export: String,
    },
    /// The artifact is not a well-formed WASM module.
    MalformedModule {
        /// Parser's reason for refusing the bytes.
        reason: String,
    },
    /// The deterministic engine refused the module.
    RejectedByEngine {
        /// Engine's reason for refusing the module.
        reason: String,
    },
    /// The deterministic engine could not be constructed.
    EngineUnavailable {
        /// Engine's reason for refusing its declared limits.
        reason: String,
    },
    /// The SDK's frozen ABI surface no longer matches the runtime's.
    AbiDrift {
        /// Exact difference between the two surfaces.
        detail: String,
    },
    /// The project declares a dependency on a clock, randomness or float
    /// library.
    ForbiddenDependency {
        /// Manifest declaring the dependency.
        manifest: PathBuf,
        /// Dependency the programs plane refuses.
        dependency: String,
    },
    /// The project's source reaches for an ambient nondeterministic item.
    ForbiddenSourceItem {
        /// Source file naming the item.
        source: PathBuf,
        /// Item the programs plane refuses.
        item: String,
    },
    /// A path the lint had to read could not be read.
    UnreadablePath {
        /// Path the lint could not read.
        path: PathBuf,
        /// Reason the read failed.
        reason: String,
    },
}

impl DeterminismViolation {
    /// Returns the stable rule name a broken build reports.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::ClockImport { .. } => "clock-import",
            Self::RandomnessImport { .. } => "randomness-import",
            Self::UndeclaredHostImport { .. } => "undeclared-host-import",
            Self::FloatingPointType => "floating-point-type",
            Self::FloatingPointInstruction => "floating-point-instruction",
            Self::VectorType => "vector-type",
            Self::ModuleTooLarge { .. } => "module-too-large",
            Self::TooManyFunctions { .. } => "too-many-functions",
            Self::MissingExport { .. } => "missing-export",
            Self::MalformedModule { .. } => "malformed-module",
            Self::RejectedByEngine { .. } => "rejected-by-engine",
            Self::EngineUnavailable { .. } => "engine-unavailable",
            Self::AbiDrift { .. } => "abi-drift",
            Self::ForbiddenDependency { .. } => "forbidden-dependency",
            Self::ForbiddenSourceItem { .. } => "forbidden-source-item",
            Self::UnreadablePath { .. } => "unreadable-path",
        }
    }
}

impl Display for DeterminismViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClockImport {
                import_module,
                import_name,
            } => write!(formatter, "clock import {import_module}::{import_name}"),
            Self::RandomnessImport {
                import_module,
                import_name,
            } => write!(
                formatter,
                "randomness import {import_module}::{import_name}"
            ),
            Self::UndeclaredHostImport {
                import_module,
                import_name,
            } => write!(
                formatter,
                "undeclared host import {import_module}::{import_name}"
            ),
            Self::FloatingPointType => formatter.write_str("floating-point types are forbidden"),
            Self::FloatingPointInstruction => {
                formatter.write_str("floating-point instructions are forbidden")
            }
            Self::VectorType => formatter.write_str("vector types are forbidden"),
            Self::ModuleTooLarge { byte_size, limit } => write!(
                formatter,
                "module of {byte_size} bytes exceeds declared limit {limit}"
            ),
            Self::TooManyFunctions {
                function_count,
                limit,
            } => write!(
                formatter,
                "module declares {function_count} functions exceeding declared limit {limit}"
            ),
            Self::MissingExport { export } => {
                write!(formatter, "module exports no {export}")
            }
            Self::MalformedModule { reason } => write!(formatter, "malformed module: {reason}"),
            Self::RejectedByEngine { reason } => write!(formatter, "rejected by engine: {reason}"),
            Self::EngineUnavailable { reason } => {
                write!(formatter, "deterministic engine unavailable: {reason}")
            }
            Self::AbiDrift { detail } => {
                write!(formatter, "SDK ABI drifted from runtime: {detail}")
            }
            Self::ForbiddenDependency {
                manifest,
                dependency,
            } => write!(
                formatter,
                "{} declares forbidden dependency {dependency}",
                manifest.display()
            ),
            Self::ForbiddenSourceItem { source, item } => {
                write!(formatter, "{} reaches for {item}", source.display())
            }
            Self::UnreadablePath { path, reason } => {
                write!(formatter, "could not read {}: {reason}", path.display())
            }
        }
    }
}

impl std::error::Error for DeterminismViolation {}

/// Checks that the SDK still speaks exactly the runtime's frozen ABI.
#[must_use]
pub fn abi_surface_violations() -> Vec<DeterminismViolation> {
    let guest_v1: Vec<_> = layerx_program_sdk::HOST_FUNCTIONS
        .iter()
        .map(|function| (function.name, function.signature))
        .collect();
    let host_v1: Vec<_> = layerx_programs_runtime::HOST_FUNCTIONS
        .iter()
        .map(|function| (function.name, function.signature))
        .collect();
    let guest_candidate: Vec<_> = layerx_program_sdk::CANDIDATE_HOST_FUNCTIONS
        .iter()
        .map(|function| (function.name, function.signature))
        .collect();
    let host_candidate: Vec<_> = layerx_programs_runtime::abi::response::CANDIDATE_HOST_FUNCTIONS
        .iter()
        .map(|function| (function.name, function.signature))
        .collect();
    let mut violations = surface_violations(
        layerx_program_sdk::ABI_MODULE,
        layerx_programs_runtime::ABI_MODULE,
        &guest_v1,
        &host_v1,
        layerx_program_sdk::CANDIDATE_ABI_MODULE,
        layerx_programs_runtime::abi::response::CANDIDATE_ABI_MODULE,
        layerx_program_sdk::MAX_CALL_RESPONSE_BYTES,
        layerx_programs_runtime::MAX_CALL_RESPONSE_BYTES,
        &guest_candidate,
        &host_candidate,
    );
    violations.extend(candidate_constant_violations(
        layerx_program_sdk::CANDIDATE_ABI_MANIFEST,
        layerx_programs_runtime::abi::response::CANDIDATE_ABI_MANIFEST,
        layerx_program_sdk::MAX_REFUSAL_REASON_BYTES,
        layerx_programs_runtime::MAX_REFUSAL_REASON_BYTES,
        layerx_program_sdk::CANDIDATE_REFUSAL_SENTINEL,
        layerx_programs_runtime::CANDIDATE_REFUSAL_SENTINEL,
        layerx_program_sdk::REFUSAL_CLASS_MANIFEST,
        layerx_programs_runtime::REFUSAL_CLASS_MANIFEST,
    ));
    violations
}

#[allow(clippy::too_many_arguments)]
fn candidate_constant_violations(
    guest_manifest: &str,
    host_manifest: &str,
    guest_reason_maximum: usize,
    host_reason_maximum: usize,
    guest_sentinel: i32,
    host_sentinel: i32,
    guest_classes: &str,
    host_classes: &str,
) -> Vec<DeterminismViolation> {
    let mut violations = Vec::new();
    for (matches, detail) in [
        (
            guest_manifest == host_manifest,
            "candidate manifest differs",
        ),
        (
            guest_reason_maximum == host_reason_maximum,
            "candidate refusal-reason maximum differs",
        ),
        (
            guest_sentinel == host_sentinel,
            "candidate refusal sentinel differs",
        ),
        (
            guest_classes == host_classes,
            "candidate refusal classes differ",
        ),
    ] {
        if !matches {
            violations.push(DeterminismViolation::AbiDrift {
                detail: detail.to_string(),
            });
        }
    }
    violations
}

#[allow(clippy::too_many_arguments)]
fn surface_violations(
    guest_module: &str,
    host_module: &str,
    guest_v1: &[(&str, &str)],
    host_v1: &[(&str, &str)],
    guest_candidate_module: &str,
    host_candidate_module: &str,
    guest_response_maximum: usize,
    host_response_maximum: usize,
    guest_candidate: &[(&str, &str)],
    host_candidate: &[(&str, &str)],
) -> Vec<DeterminismViolation> {
    let mut violations = Vec::new();
    if guest_module != host_module {
        violations.push(DeterminismViolation::AbiDrift {
            detail: format!("host module {guest_module} is not {host_module}"),
        });
    }
    if layerx_program_sdk::ABI_MANIFEST != layerx_programs_runtime::ABI_MANIFEST {
        violations.push(DeterminismViolation::AbiDrift {
            detail: "frozen host-function manifest differs".to_string(),
        });
    }
    compare_function_tables("v1", guest_v1, host_v1, &mut violations);
    if guest_candidate_module != host_candidate_module {
        violations.push(DeterminismViolation::AbiDrift {
            detail: format!(
                "candidate host module {guest_candidate_module} is not {host_candidate_module}"
            ),
        });
    }
    if guest_response_maximum != host_response_maximum {
        violations.push(DeterminismViolation::AbiDrift {
            detail: format!(
                "candidate response maximum {guest_response_maximum} is not {host_response_maximum}"
            ),
        });
    }
    compare_function_tables(
        "candidate",
        guest_candidate,
        host_candidate,
        &mut violations,
    );
    violations
}

fn compare_function_tables(
    label: &str,
    guest: &[(&str, &str)],
    host: &[(&str, &str)],
    violations: &mut Vec<DeterminismViolation>,
) {
    if guest.len() != host.len() {
        violations.push(DeterminismViolation::AbiDrift {
            detail: format!(
                "{label} host-function count {} is not {}",
                guest.len(),
                host.len()
            ),
        });
    }
    for ((guest_name, guest_signature), (host_name, host_signature)) in guest.iter().zip(host) {
        if guest_name != host_name || guest_signature != host_signature {
            violations.push(DeterminismViolation::AbiDrift {
                detail: format!("{guest_name}{guest_signature} is not {host_name}{host_signature}"),
            });
        }
    }
}

/// Lints one compiled program artifact.
#[must_use]
pub fn lint_artifact(wasm: &[u8]) -> Vec<DeterminismViolation> {
    let mut violations = import_and_export_violations(wasm, false);
    violations.extend(engine_violations(wasm, false));
    violations
}

/// Lints an explicitly candidate-qualified compiled program artifact.
#[must_use]
pub fn lint_candidate_artifact(wasm: &[u8]) -> Vec<DeterminismViolation> {
    let mut violations = import_and_export_violations(wasm, true);
    violations.extend(engine_violations(wasm, true));
    violations
}

/// Lints the source and manifests of one program project.
#[must_use]
pub fn lint_sources(project: &Path) -> Vec<DeterminismViolation> {
    let mut violations = Vec::new();
    let mut files = Vec::new();
    if let Err(violation) = visit_files(project, &mut files) {
        violations.push(violation);
        return violations;
    }
    files.sort();
    for path in files {
        let body = match fs::read_to_string(&path) {
            Ok(body) => body,
            Err(error) => {
                violations.push(DeterminismViolation::UnreadablePath {
                    path,
                    reason: error.to_string(),
                });
                continue;
            }
        };
        if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            manifest_violations(&path, &body, &mut violations);
        } else {
            source_violations(&path, &body, &mut violations);
        }
    }
    violations
}

/// Lints one program project end to end: frozen ABI surface, project source
/// and manifests, and the compiled artifact.
#[must_use]
pub fn lint_project(project: &Path, artifact: Option<&Path>) -> Vec<DeterminismViolation> {
    let mut violations = abi_surface_violations();
    violations.extend(lint_sources(project));
    let path = match artifact.map(Path::to_path_buf).map_or_else(
        || discover_artifact(project),
        Ok::<PathBuf, DeterminismViolation>,
    ) {
        Ok(path) => path,
        Err(violation) => {
            violations.push(violation);
            return violations;
        }
    };
    match fs::read(&path) {
        Ok(wasm) => violations.extend(lint_artifact(&wasm)),
        Err(error) => violations.push(DeterminismViolation::UnreadablePath {
            path,
            reason: error.to_string(),
        }),
    }
    violations
}

/// Locates the single release artifact a program project produced.
///
/// # Errors
///
/// Names the directory when no artifact, or more than one, is present.
pub fn discover_artifact(project: &Path) -> Result<PathBuf, DeterminismViolation> {
    let directory = project.join("target/wasm32-unknown-unknown/release");
    let entries =
        fs::read_dir(&directory).map_err(|error| DeterminismViolation::UnreadablePath {
            path: directory.clone(),
            reason: error.to_string(),
        })?;
    let mut artifacts: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "wasm")
        })
        .collect();
    artifacts.sort();
    match artifacts.as_slice() {
        [artifact] => Ok(artifact.clone()),
        [] => Err(DeterminismViolation::UnreadablePath {
            path: directory,
            reason: "no .wasm artifact was produced".to_string(),
        }),
        _ => Err(DeterminismViolation::UnreadablePath {
            path: directory,
            reason: "multiple .wasm artifacts were produced".to_string(),
        }),
    }
}

fn permitted_import(import_module: &str, import_name: &str, candidate: bool) -> bool {
    (import_module == layerx_programs_runtime::ABI_MODULE
        && layerx_programs_runtime::HOST_FUNCTIONS
            .iter()
            .any(|function| function.name == import_name))
        || (candidate
            && import_module == layerx_programs_runtime::abi::response::CANDIDATE_ABI_MODULE
            && layerx_programs_runtime::abi::response::CANDIDATE_HOST_FUNCTIONS
                .iter()
                .any(|function| function.name == import_name))
}

fn classify_import(import_module: &str, import_name: &str) -> DeterminismViolation {
    if CLOCK_IMPORT_NAMES.contains(&import_name) {
        return DeterminismViolation::ClockImport {
            import_module: import_module.to_string(),
            import_name: import_name.to_string(),
        };
    }
    if RANDOMNESS_IMPORT_NAMES.contains(&import_name) {
        return DeterminismViolation::RandomnessImport {
            import_module: import_module.to_string(),
            import_name: import_name.to_string(),
        };
    }
    DeterminismViolation::UndeclaredHostImport {
        import_module: import_module.to_string(),
        import_name: import_name.to_string(),
    }
}

fn import_and_export_violations(wasm: &[u8], candidate: bool) -> Vec<DeterminismViolation> {
    let mut violations = Vec::new();
    let mut memory = false;
    let mut entrypoint = false;
    let mut reservation = false;
    for payload in Parser::new(0).parse_all(wasm) {
        let payload = match payload {
            Ok(payload) => payload,
            Err(error) => {
                violations.push(DeterminismViolation::MalformedModule {
                    reason: error.to_string(),
                });
                return violations;
            }
        };
        match payload {
            Payload::ImportSection(reader) => {
                for entry in reader {
                    match entry {
                        Ok(import) => {
                            if !permitted_import(import.module, import.name, candidate) {
                                violations.push(classify_import(import.module, import.name));
                            }
                        }
                        Err(error) => violations.push(DeterminismViolation::MalformedModule {
                            reason: error.to_string(),
                        }),
                    }
                }
            }
            Payload::ExportSection(reader) => {
                for entry in reader {
                    match entry {
                        Ok(export) => {
                            if export.kind == ExternalKind::Memory && export.name == MEMORY_EXPORT {
                                memory = true;
                            }
                            if export.kind == ExternalKind::Func && export.name == CALL_ENTRY_EXPORT
                            {
                                entrypoint = true;
                            }
                            if export.kind == ExternalKind::Func
                                && export.name == CALL_RESERVE_EXPORT
                            {
                                reservation = true;
                            }
                        }
                        Err(error) => violations.push(DeterminismViolation::MalformedModule {
                            reason: error.to_string(),
                        }),
                    }
                }
            }
            _ => {}
        }
    }
    if !memory {
        violations.push(DeterminismViolation::MissingExport {
            export: MEMORY_EXPORT.to_string(),
        });
    }
    if !entrypoint {
        violations.push(DeterminismViolation::MissingExport {
            export: CALL_ENTRY_EXPORT.to_string(),
        });
    }
    if !reservation {
        violations.push(DeterminismViolation::MissingExport {
            export: CALL_RESERVE_EXPORT.to_string(),
        });
    }
    violations
}

fn engine_violations(wasm: &[u8], candidate: bool) -> Vec<DeterminismViolation> {
    let engine = match WasmEngine::declared() {
        Ok(engine) => engine,
        Err(error) => {
            return vec![DeterminismViolation::EngineUnavailable {
                reason: error.to_string(),
            }]
        }
    };
    let validated = if candidate {
        engine.validate_candidate_v2(wasm)
    } else {
        engine.validate(wasm)
    };
    match validated {
        Ok(_) => Vec::new(),
        Err(refusal) => vec![from_validation(&refusal)],
    }
}

fn from_validation(refusal: &ValidationRefusal) -> DeterminismViolation {
    match refusal {
        ValidationRefusal::ModuleTooLarge { byte_size, limit } => {
            DeterminismViolation::ModuleTooLarge {
                byte_size: *byte_size,
                limit: *limit,
            }
        }
        ValidationRefusal::TooManyFunctions {
            function_count,
            limit,
        } => DeterminismViolation::TooManyFunctions {
            function_count: *function_count,
            limit: *limit,
        },
        ValidationRefusal::ForbiddenImport {
            import_module,
            import_name,
        } => classify_import(import_module, import_name),
        ValidationRefusal::WrongImportKind { import_name }
        | ValidationRefusal::WrongImportSignature { import_name } => {
            DeterminismViolation::RejectedByEngine {
                reason: format!("invalid LayerX ABI import {import_name}"),
            }
        }
        ValidationRefusal::ForbiddenFloatType => DeterminismViolation::FloatingPointType,
        ValidationRefusal::ForbiddenFloatInstruction => {
            DeterminismViolation::FloatingPointInstruction
        }
        ValidationRefusal::ForbiddenVectorType => DeterminismViolation::VectorType,
        ValidationRefusal::MalformedModule { reason } => DeterminismViolation::MalformedModule {
            reason: reason.clone(),
        },
        ValidationRefusal::RejectedByEngine { reason } => DeterminismViolation::RejectedByEngine {
            reason: reason.clone(),
        },
    }
}

fn visit_files(root: &Path, output: &mut Vec<PathBuf>) -> Result<(), DeterminismViolation> {
    let entries = fs::read_dir(root).map_err(|error| DeterminismViolation::UnreadablePath {
        path: root.to_path_buf(),
        reason: error.to_string(),
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| DeterminismViolation::UnreadablePath {
            path: root.to_path_buf(),
            reason: error.to_string(),
        })?;
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|name| name == "target" || name == ".git")
            {
                continue;
            }
            visit_files(&path, output)?;
        } else if path.file_name().is_some_and(|name| name == "Cargo.toml")
            || path.extension().is_some_and(|extension| extension == "rs")
        {
            output.push(path);
        }
    }
    Ok(())
}

fn manifest_violations(path: &Path, body: &str, violations: &mut Vec<DeterminismViolation>) {
    for line in body.lines() {
        let declaration = line.split('#').next().unwrap_or("").trim();
        for dependency in FORBIDDEN_DEPENDENCIES {
            let declared = declaration
                .strip_prefix(*dependency)
                .is_some_and(|tail| tail.trim_start().starts_with('='));
            if declared {
                violations.push(DeterminismViolation::ForbiddenDependency {
                    manifest: path.to_path_buf(),
                    dependency: (*dependency).to_string(),
                });
            }
        }
    }
}

fn source_violations(path: &Path, body: &str, violations: &mut Vec<DeterminismViolation>) {
    let code = strip_line_comments(body);
    for item in FORBIDDEN_SOURCE_ITEMS {
        if code.contains(item) {
            violations.push(DeterminismViolation::ForbiddenSourceItem {
                source: path.to_path_buf(),
                item: (*item).to_string(),
            });
        }
    }
    for name in FLOAT_TYPE_NAMES {
        if contains_word(&code, name) {
            violations.push(DeterminismViolation::ForbiddenSourceItem {
                source: path.to_path_buf(),
                item: (*name).to_string(),
            });
        }
    }
}

fn strip_line_comments(body: &str) -> String {
    let mut code = String::with_capacity(body.len());
    for line in body.lines() {
        let statement = line.split("//").next().unwrap_or("");
        code.push_str(statement);
        code.push('\n');
    }
    code
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn contains_word(body: &str, word: &str) -> bool {
    let bytes = body.as_bytes();
    let mut cursor = 0;
    while let Some(offset) = body.get(cursor..).and_then(|tail| tail.find(word)) {
        let start = cursor.saturating_add(offset);
        let end = start.saturating_add(word.len());
        let leading = start == 0
            || bytes
                .get(start.saturating_sub(1))
                .is_none_or(|byte| !is_identifier_byte(*byte));
        let trailing = bytes.get(end).is_none_or(|byte| !is_identifier_byte(*byte));
        if leading && trailing {
            return true;
        }
        cursor = end;
    }
    false
}

#[cfg(test)]
mod abi_surface_tests {
    use super::{
        candidate_constant_violations, permitted_import, surface_violations, DeterminismViolation,
    };

    fn details(violations: &[DeterminismViolation]) -> Vec<&str> {
        violations
            .iter()
            .filter_map(|violation| match violation {
                DeterminismViolation::AbiDrift { detail } => Some(detail.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn equal_prefix_with_an_extra_host_function_is_abi_drift() {
        let guest_v1 = [("storage_read", "(i32)->i32")];
        let host_v1 = [
            ("storage_read", "(i32)->i32"),
            ("storage_write", "(i32)->i32"),
        ];
        let violations = surface_violations(
            "layerx_v1",
            "layerx_v1",
            &guest_v1,
            &host_v1,
            "layerx_v2_candidate",
            "layerx_v2_candidate",
            8,
            8,
            &[],
            &[],
        );

        assert!(details(&violations).contains(&"v1 host-function count 1 is not 2"));
    }

    #[test]
    fn candidate_function_name_or_signature_mismatch_is_abi_drift() {
        let guest_candidate = [("response_write", "(i32,i32,i32)->i32")];
        let host_candidate = [("response_read", "(i32,i32,i32)->i32")];
        let violations = surface_violations(
            "layerx_v1",
            "layerx_v1",
            &[],
            &[],
            "layerx_v2_candidate",
            "layerx_v2_candidate",
            8,
            8,
            &guest_candidate,
            &host_candidate,
        );

        assert!(details(&violations)
            .contains(&"response_write(i32,i32,i32)->i32 is not response_read(i32,i32,i32)->i32",));
    }

    #[test]
    fn candidate_manifest_bound_sentinel_and_classes_are_all_parity_gated() {
        for violations in [
            candidate_constant_violations("guest", "host", 8, 8, -64, -64, "classes", "classes"),
            candidate_constant_violations(
                "manifest", "manifest", 7, 8, -64, -64, "classes", "classes",
            ),
            candidate_constant_violations(
                "manifest", "manifest", 8, 8, -65, -64, "classes", "classes",
            ),
            candidate_constant_violations("manifest", "manifest", 8, 8, -64, -64, "guest", "host"),
        ] {
            assert_eq!(violations.len(), 1);
            assert_eq!(violations[0].name(), "abi-drift");
        }
    }

    #[test]
    fn candidate_imports_require_explicit_revision_and_exact_declaration() {
        assert!(!permitted_import(
            "layerx_v2_candidate",
            "refusal_write",
            false
        ));
        assert!(permitted_import(
            "layerx_v2_candidate",
            "refusal_write",
            true
        ));
        assert!(!permitted_import("layerx_v2_candidate", "undeclared", true));
    }
}
