extern crate alloc;

#[path = "src/bindgen.rs"]
mod bindgen;

use bindgen::BindingGenerator;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const INTERFACE_PATH: &str = "LAYERX_INTERFACE_PATH";
const INTERFACE_DIGEST: &str = "LAYERX_INTERFACE_DIGEST";
const PROGRAM_CODE_HASH: &str = "LAYERX_PROGRAM_CODE_HASH";
const BINDINGS_DIR: &str = "LAYERX_BINDINGS_DIR";

fn main() {
    for name in [INTERFACE_PATH, INTERFACE_DIGEST, PROGRAM_CODE_HASH, BINDINGS_DIR] {
        println!("cargo:rerun-if-env-changed={name}");
    }

    let Some(interface_path) = optional_env(INTERFACE_PATH) else {
        refuse_partial_configuration();
        return;
    };
    let digest = required_hash(INTERFACE_DIGEST);
    let code_hash = required_hash(PROGRAM_CODE_HASH);
    println!("cargo:rerun-if-changed={interface_path}");

    let canonical = fs::read(&interface_path)
        .unwrap_or_else(|error| panic!("cannot read canonical LayerX interface {interface_path}: {error}"));
    let generator = BindingGenerator::from_interface(&canonical)
        .unwrap_or_else(|error| panic!("invalid canonical LayerX interface {interface_path}: {error}"));
    generator
        .require_digest(digest)
        .unwrap_or_else(|error| panic!("refusing stale LayerX bindings for {interface_path}: {error}"));
    generator
        .require_code_hash(code_hash)
        .unwrap_or_else(|error| panic!("refusing LayerX bindings for the wrong deployed program: {error}"));

    let output = optional_env(BINDINGS_DIR).map_or_else(out_dir, PathBuf::from);
    fs::create_dir_all(&output)
        .unwrap_or_else(|error| panic!("cannot create LayerX bindings directory {}: {error}", output.display()));
    let generated = generator.generate_all();
    write(&output.join("layerx_client.rs"), generated.rust.as_bytes());
    write(&output.join("layerx_client.ts"), generated.typescript.as_bytes());
    write(&output.join("layerx_guest.rs"), generated.guest.as_bytes());

    println!("cargo:rustc-env=LAYERX_RUST_BINDINGS={}", output.join("layerx_client.rs").display());
    println!("cargo:rustc-env=LAYERX_TYPESCRIPT_BINDINGS={}", output.join("layerx_client.ts").display());
    println!("cargo:rustc-env=LAYERX_GUEST_BINDINGS={}", output.join("layerx_guest.rs").display());
}

fn refuse_partial_configuration() {
    for name in [INTERFACE_DIGEST, PROGRAM_CODE_HASH, BINDINGS_DIR] {
        if optional_env(name).is_some() {
            panic!("{name} requires {INTERFACE_PATH}; configure all three binding inputs together");
        }
    }
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn required_hash(name: &str) -> [u8; 32] {
    let value = optional_env(name)
        .unwrap_or_else(|| panic!("{name} is required when {INTERFACE_PATH} is configured"));
    let hex = value.strip_prefix("0x").unwrap_or(&value);
    if hex.len() != 64 {
        panic!("{name} must be exactly 32 hexadecimal bytes");
    }
    let mut decoded = [0_u8; 32];
    for (index, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
        let pair = std::str::from_utf8(pair)
            .unwrap_or_else(|_| panic!("{name} must contain only ASCII hexadecimal digits"));
        decoded[index] = u8::from_str_radix(pair, 16)
            .unwrap_or_else(|_| panic!("{name} must contain only hexadecimal digits"));
    }
    decoded
}

fn out_dir() -> PathBuf {
    PathBuf::from(env::var_os("OUT_DIR").unwrap_or_else(|| panic!("Cargo did not provide OUT_DIR")))
        .join("layerx-bindings")
}

fn write(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes)
        .unwrap_or_else(|error| panic!("cannot write generated LayerX binding {}: {error}", path.display()));
}
