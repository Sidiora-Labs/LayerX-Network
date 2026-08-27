use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const BANNED_DEPENDENCIES: &[&str] = &[
    "bindgen",
    "libsqlite3-sys",
    "rusqlite",
    "sqlx-sqlite",
    "layerx-core",
    "layerx-protocol-core",
];

const PRIVATE_PATHS: &[&str] = &[
    "node.db",
    "projection.db",
    "append-only.log",
    "log/segments/",
    "state/snapshots/",
    "snapshot.bin",
];

const AMBIENT_NONDETERMINISM: &[&str] = &[
    "SystemTime::now",
    "Instant::now",
    "thread_rng(",
    "getrandom(",
    "OsRng",
];

#[derive(Debug, Eq, PartialEq)]
struct Violation {
    path: PathBuf,
    rule: &'static str,
}

fn visit_files(root: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        fs::read_dir(root).map_err(|error| format!("cannot read {}: {error}", root.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
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

fn dependency_violation(body: &str) -> bool {
    body.lines().any(|line| {
        let declaration = line.split('#').next().unwrap_or("").trim();
        BANNED_DEPENDENCIES.iter().any(|dependency| {
            declaration
                .strip_prefix(dependency)
                .is_some_and(|tail| tail.trim_start().starts_with('='))
        }) || declaration.starts_with("links = \"layerx")
    })
}

fn source_rule(
    body: &str,
    allow_c_layout: bool,
    allow_ambient_clock: bool,
) -> Option<&'static str> {
    if body.contains("include!(") && body.contains("include/layerx")
        || body.contains("include_bytes!(") && body.contains("include/layerx")
        || body.contains("bindgen::")
        || body.contains("#[link(name = \"layerx")
    {
        return Some("private-c-boundary");
    }
    if !allow_c_layout && body.contains("extern \"C\"") {
        return Some("private-c-boundary");
    }
    if !allow_c_layout && (body.contains("#[repr(C)]") || body.contains("#[repr(C,")) {
        return Some("mirrored-c-layout");
    }
    if PRIVATE_PATHS.iter().any(|private| body.contains(private)) {
        return Some("node-private-path");
    }
    if !allow_ambient_clock
        && AMBIENT_NONDETERMINISM
            .iter()
            .any(|ambient| body.contains(ambient))
    {
        return Some("ambient-nondeterminism");
    }
    None
}

fn direct_core_connection(body: &str) -> bool {
    body.contains("lni::transport::Uds")
        || body.contains("lni::transport::Tls")
        || body.contains("lni::abi::Handle")
}

fn allowlisted_paths(agent_root: &Path, name: &str) -> Result<Vec<PathBuf>, String> {
    let path = agent_root.join(name);
    let body = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let mut paths = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if line.starts_with('"') && line.ends_with("\",") {
            paths.push(agent_root.join(line.trim_matches(&['"', ','][..])));
        } else if let Some(value) = line
            .strip_prefix("path = \"")
            .and_then(|value| value.strip_suffix('"'))
        {
            paths.push(agent_root.join(value));
        }
    }
    Ok(paths)
}

fn agent_boundary_purity_check(agent_root: &Path) -> Result<(), Vec<Violation>> {
    let crate_root = agent_root.join("crates");
    let Ok(allowlist) = allowlisted_paths(agent_root, "stable-abi-allowlist.toml") else {
        return Err(vec![Violation {
            path: agent_root.join("stable-abi-allowlist.toml"),
            rule: "missing-stable-abi-allowlist",
        }]);
    };
    let Ok(unsafe_allowlist) = allowlisted_paths(agent_root, "unsafe-allowlist.toml") else {
        return Err(vec![Violation {
            path: agent_root.join("unsafe-allowlist.toml"),
            rule: "missing-unsafe-allowlist",
        }]);
    };
    let Ok(clock_allowlist) = allowlisted_paths(agent_root, "nondeterminism-allowlist.toml")
    else {
        return Err(vec![Violation {
            path: agent_root.join("nondeterminism-allowlist.toml"),
            rule: "missing-nondeterminism-allowlist",
        }]);
    };
    let mut files = Vec::new();
    if visit_files(&crate_root, &mut files).is_err() {
        return Err(vec![Violation {
            path: crate_root,
            rule: "unreadable-crate-tree",
        }]);
    }
    let mut violations = Vec::new();
    for path in files {
        let Ok(body) = fs::read_to_string(&path) else {
            violations.push(Violation {
                path,
                rule: "unreadable-source",
            });
            continue;
        };
        if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            if dependency_violation(&body) {
                violations.push(Violation {
                    path,
                    rule: "forbidden-dependency",
                });
            }
        } else {
            if let Some(rule) = source_rule(
                &body,
                allowlist.contains(&path),
                clock_allowlist.contains(&path),
            ) {
                violations.push(Violation {
                    path: path.clone(),
                    rule,
                });
            }
            let layerx_client_root = agent_root.join("crates/layerx-client");
            if direct_core_connection(&body) && !path.starts_with(layerx_client_root) {
                violations.push(Violation {
                    path: path.clone(),
                    rule: "direct-core-connection",
                });
            }
            let uses_unsafe = body.contains("unsafe {")
                || body.contains("unsafe fn")
                || body.contains("unsafe extern")
                || body.contains("extern \"C\"");
            if uses_unsafe && !unsafe_allowlist.contains(&path) {
                violations.push(Violation {
                    path,
                    rule: "unapproved-unsafe",
                });
            }
        }
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}

fn main() {
    let root = env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("agent"), PathBuf::from);
    if let Err(violations) = agent_boundary_purity_check(&root) {
        for violation in violations {
            eprintln!("{}: {}", violation.rule, violation.path.display());
        }
        std::process::exit(1);
    }
    println!("agent boundary purity passed");
}
