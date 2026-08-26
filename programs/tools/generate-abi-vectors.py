#!/usr/bin/env python3
"""Audit and generate immutable Programs ABI vectors from canonical source."""
import argparse, ast, hashlib, pathlib, re, sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
RUNTIME = ROOT / "programs/crates/layerx-programs-runtime/src"
MANIFEST = RUNTIME / "abi/manifest.rs"
ABI_MOD = RUNTIME / "abi/mod.rs"
CRATE_ROOT = RUNTIME / "lib.rs"
SDK_ABI = ROOT / "programs/sdk/rust/src/abi.rs"
OUTPUT = ROOT / "programs/tests/vectors"
FROZEN = ROOT / "programs/abi-frozen.sha256"

def rust_string(source, name):
    match = re.search(rf'^pub const {name}: &str = (".*");$', source, re.MULTILINE)
    if match is None: raise ValueError(f"{name} is absent")
    return ast.literal_eval(match.group(1))

def rust_u16(source, name):
    match = re.search(rf'^pub const {name}: u16 = ([0-9]+);$', source, re.MULTILINE)
    if match is None: raise ValueError(f"{name} is absent")
    return int(match.group(1))

def table(source, name):
    start = source.index(f"pub const {name}:")
    body = source[start:source.index("\n];", start)]
    calls = re.findall(r'host\("([^"]+)", "([^"]+)"\)', body)
    return calls or re.findall(r'HostFunction\s*\{\s*name:\s*"([^"]+)",\s*signature:\s*"([^"]+)"', body, re.DOTALL)

def manifest_surface(encoded):
    surface, module = [], None
    for item in encoded.split("\0"):
        if not item: continue
        if "(" not in item: module = item; continue
        if module is None: raise ValueError("function precedes ABI namespace")
        name, signature = item.split("(", 1)
        surface.append((module, name, "(" + signature))
    return surface

def audit_surface(runtime_source):
    v1_manifest = rust_string(runtime_source, "ABI_V1_MANIFEST")
    crate_root = CRATE_ROOT.read_text()
    current_version = rust_u16(crate_root, "ABI_VERSION")
    if current_version != 2: raise ValueError("crate-root ABI_VERSION does not identify ABI v2")
    v2_manifest = rust_string(crate_root, "ABI_MANIFEST")
    if "pub const ABI_V2_MANIFEST: &str = crate::ABI_MANIFEST;" not in runtime_source:
        raise ValueError("ABI v2 manifest is not owned by the crate-root ABI_MANIFEST")
    if "pub const ABI_V2_VERSION: u16 = crate::ABI_VERSION;" not in runtime_source:
        raise ValueError("ABI v2 version is not owned by the crate-root ABI_VERSION")
    v1 = table(ABI_MOD.read_text(), "HOST_FUNCTIONS")
    v2 = table(runtime_source, "ABI_V2_HOST_FUNCTIONS")
    expected_v1 = [("layerx_v1", name, signature) for name, signature in v1]
    expected_v2 = expected_v1 + [("layerx_v2", name, signature) for name, signature in v2]
    if manifest_surface(v1_manifest) != expected_v1: raise ValueError("ABI v1 manifest and host table diverge")
    if manifest_surface(v2_manifest) != expected_v2: raise ValueError("ABI v2 composite manifest and host tables diverge")
    type_start = runtime_source.index("const ABI_V2_FUNCTION_TYPES:")
    type_block = runtime_source[type_start:runtime_source.index("const fn function_type", type_start)]
    type_entries = re.findall(r"function_type\(([^,]+),\s*([^\)]+)\)", type_block)
    if len(type_entries) != len(v2): raise ValueError("ABI v2 function table and types diverge")
    parameter_types = {
        "I32_1": ["i32"], "I32_3": ["i32"] * 3, "I32_4": ["i32"] * 4,
        "I32_5": ["i32"] * 5, "I32_6": ["i32"] * 6, "I32_7": ["i32"] * 7,
        "I32_8": ["i32"] * 8, "I32_9": ["i32"] * 9,
        "TRANSFER": ["i64", "i64"] + ["i32"] * 8,
        "FUND": ["i64", "i64"] + ["i32"] * 6,
    }
    result_types = {"I32_RESULT": "i32", "I64_RESULT": "i64"}
    typed_signatures = []
    for params, result in type_entries:
        if params.strip() not in parameter_types or result.strip() not in result_types:
            raise ValueError("ABI v2 function type uses an undeclared shape")
        typed_signatures.append(
            "(" + ",".join(parameter_types[params.strip()]) + ")->" + result_types[result.strip()]
        )
    if typed_signatures != [signature for _, signature in v2]:
        raise ValueError("ABI v2 signatures and function types diverge")
    validate = (RUNTIME / "validate.rs").read_text()
    if "manifest::permitted_import" not in validate or "pub(crate) fn permitted_import" not in runtime_source: raise ValueError("validator does not derive its allowlist from the frozen table")
    sdk = SDK_ABI.read_text()
    if rust_string(sdk, "CANDIDATE_ABI_MANIFEST") != v2_manifest: raise ValueError("Rust SDK ABI v2 manifest diverges")
    if table(sdk, "CANDIDATE_HOST_FUNCTIONS") != v2: raise ValueError("Rust SDK ABI v2 table diverges")
    return {1: v1_manifest, current_version: v2_manifest}

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    try: manifests = audit_surface(MANIFEST.read_text())
    except (ValueError, OSError) as error:
        print(f"ABI surface drift: {error}", file=sys.stderr); return 1
    frozen = {}
    for line in FROZEN.read_text().splitlines():
        if not line or line.startswith("#"): continue
        version, digest = line.split()
        frozen[int(version)] = digest
    for version, manifest in manifests.items():
        path = OUTPUT / f"abi-v{version}.hex"
        generated = (version.to_bytes(2, "big") + manifest.encode()).hex() + "\n"
        if version in frozen:
            if not path.exists():
                print(f"frozen ABI v{version} vector is missing and cannot be recreated", file=sys.stderr); return 1
            if hashlib.sha256(path.read_bytes()).hexdigest() != frozen[version]:
                print(f"frozen ABI v{version} checksum differs from independent baseline", file=sys.stderr); return 1
            if path.read_text() != generated:
                print(f"immutable ABI v{version} surface drift; allocate a new ABI version", file=sys.stderr); return 1
        elif path.exists():
            print(f"unfrozen ABI v{version} vector exists; review and add its checksum baseline", file=sys.stderr); return 1
        elif args.check:
            print(f"new ABI v{version} has no generated vector", file=sys.stderr); return 1
        else:
            path.write_text(generated)
    return 0

if __name__ == "__main__": raise SystemExit(main())
