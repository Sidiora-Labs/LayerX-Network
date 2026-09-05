use std::env;
use std::path::PathBuf;
use std::process::Command;

fn run(mut command: Command, label: &str) {
    let status = command.status().unwrap_or_else(|error| {
        panic!("could not run {label}: {error}");
    });
    assert!(status.success(), "{label} failed with {status}");
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| {
        panic!("Cargo did not provide CARGO_MANIFEST_DIR");
    }));
    let root = manifest
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|error| {
            panic!("could not resolve repository root: {error}");
        });
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap_or_else(|| {
        panic!("Cargo did not provide OUT_DIR");
    }));
    let object = out.join("emulator_core.o");
    let archive = out.join("liblayerx_emulator_core.a");

    let mut make = Command::new("make");
    make.current_dir(&root).env("OPT_LEVEL", "-O2").arg("build");
    run(make, "LayerX core build");

    let mut compiler = Command::new(env::var_os("CC").unwrap_or_else(|| "cc".into()));
    compiler
        .current_dir(&root)
        .args([
            "-std=c17",
            "-pedantic",
            "-Werror",
            "-Wall",
            "-Wextra",
            "-Wconversion",
            "-Wshadow",
            "-Wvla",
            "-fno-strict-aliasing",
            "-ffp-contract=off",
            "-O2",
            "-Iinclude",
            "-c",
        ])
        .arg(format!(
            "-DLAYERX_EMULATOR_HISTORY_SCHEMA=\"{}\"",
            root.join("migrations/0007_history_index.sql").display()
        ))
        .arg(manifest.join("core/emulator_core.c"))
        .arg("-o")
        .arg(&object);
    run(compiler, "emulator core bridge build");

    let mut archiver = Command::new(env::var_os("AR").unwrap_or_else(|| "ar".into()));
    archiver.args(["rcsD"]).arg(&archive).arg(&object);
    run(archiver, "emulator core bridge archive");

    println!(
        "cargo:rerun-if-changed={}",
        manifest.join("core/emulator_core.c").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest.join("core/emulator_core.h").display()
    );
    println!("cargo:rerun-if-changed={}", root.join("include").display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("migrations/0007_history_index.sql").display()
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!(
        "cargo:rustc-link-search=native={}",
        root.join("build").display()
    );
    println!("cargo:rustc-link-lib=static=layerx_emulator_core");
    println!("cargo:rustc-link-lib=static=layerx");
    println!("cargo:rustc-link-lib=crypto");
    println!("cargo:rustc-link-lib=sqlite3");
    println!("cargo:rustc-link-lib=pthread");
}
