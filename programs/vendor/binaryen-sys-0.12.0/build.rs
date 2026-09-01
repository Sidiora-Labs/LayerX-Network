extern crate cc;
extern crate cmake;
extern crate heck;
extern crate regex;

use heck::CamelCase;
use regex::Regex;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, PartialEq, Debug)]
struct Pass {
    id: String,
    name: String,
    description: String,
}

fn read_passes() -> Vec<Pass> {
    let re = Regex::new(r#"registerPass\(\s*"([^"]+)",\s*("[^"]+"\s*)+,\s*[^)]+\s*\);"#).unwrap();

    let mut passes: Vec<Pass> = vec![];

    let input = std::fs::read_to_string("binaryen/src/passes/pass.cpp").expect("Couldn't open pass.cpp");
    for caps in re.captures_iter(&input) {
        let name = caps.get(1).unwrap().as_str();
        let description = caps.get(2).unwrap().as_str().replace("\"", "");

        passes.push(Pass {
            id: name.to_camel_case(),
            name: name.to_string(),
            description: description.to_string(),
        });
    }

    passes
}

fn gen_passes() {
    let passes: Vec<Pass> = read_passes();

    let ids: Vec<String> = passes
        .iter()
        .map(|pass| {
            format!(
                "/// {}\n{}",
                pass.description.to_string(),
                pass.id.to_string()
            )
        })
        .collect();

    let fromstrs: Vec<String> = passes
        .iter()
        .map(|pass| {
            format!(
                r#""{}" => Ok(OptimizationPass::{})"#,
                pass.name.to_string(),
                pass.id.to_string()
            )
        })
        .collect();

    let descriptions: Vec<String> = passes
        .iter()
        .map(|pass| {
            format!(
                r#"OptimizationPass::{} => "{}""#,
                pass.id.to_string(),
                pass.description.to_string()
            )
        })
        .collect();

    let output = format!(r#"
        use std::str::FromStr;

        #[derive(Eq, PartialEq, Debug)]
        pub enum OptimizationPass {{
            {ids}
        }}

        impl FromStr for OptimizationPass {{
            type Err = ();
            fn from_str(s: &str) -> Result<Self, Self::Err> {{
                match s {{
                    {fromstrs},
                    _ => Err(()),
                }}
            }}
        }}

        trait OptimizationPassDescription {{
            fn description(&self) -> &'static str;
        }}

        impl OptimizationPassDescription for OptimizationPass {{
            fn description(&self) -> &'static str {{
                match self {{
                    {descriptions}
                }}
            }}
        }}

        #[cfg(test)]
        mod tests {{
            use super::*;

            #[test]
            fn test_from_str() {{
                assert_eq!(OptimizationPass::{test_id}, OptimizationPass::from_str("{test_name}").expect("from_str expected to work"));
            }}

            #[test]
            fn test_description() {{
                assert_eq!(OptimizationPass::{test_id}.description(), "{test_description}");
            }}
        }}
    "#,
    ids = ids.join(",\n"),
    fromstrs = fromstrs.join(",\n"),
    descriptions = descriptions.join(",\n"),
    test_id = passes[0].id.to_string(),
    test_name = passes[0].name.to_string(),
    test_description = passes[0].description.to_string()
    );

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    fs::write(out_path.join("passes.rs"), output).expect("Unable to write passes.rs");
}

fn main() {
    if !Path::new("binaryen/.git").exists() {
        let _ = Command::new("git")
            .args(&["submodule", "update", "--init"])
            .status();
    }

    gen_passes();

    let target = env::var("TARGET").ok();
    if target.map_or(false, |target| target.contains("emscripten")) {
        let mut build_wasm_binaryen_args = vec![];
        if get_debug() {
            build_wasm_binaryen_args.push("-g");
        }

        let _ = Command::new("./build-binaryen-bc.sh")
            .args(&build_wasm_binaryen_args)
            .status()
            .unwrap();

        println!(
            "cargo:rustc-link-search=native={}",
            env::var("OUT_DIR").unwrap()
        );
        println!("cargo:rustc-link-lib=static=binaryen-c");
        return;
    }

    let dst = cmake::Config::new("binaryen")
        .define("BUILD_STATIC_LIB", "ON")
        .define("ENABLE_WERROR", "OFF")
        .build();

    println!("cargo:rustc-link-search=native={}/build/lib", dst.display());
    println!("cargo:rustc-link-lib=static=binaryen");

    // We need to link against C++ std lib
    if let Some(cpp_stdlib) = get_cpp_stdlib() {
        println!("cargo:rustc-link-lib={}", cpp_stdlib);
    }

    let mut cfg = cc::Build::new();
    cfg.file("Shim.cpp")
        .include("binaryen/src")
        .cpp_link_stdlib(None)
        .warnings(false)
        .cpp(true)
        .flag("-std=c++14")
        .compile("binaryen_shim");
}

// See https://github.com/alexcrichton/gcc-rs/blob/88ac58e25/src/lib.rs#L1197
fn get_cpp_stdlib() -> Option<String> {
    env::var("TARGET").ok().and_then(|target| {
        if target.contains("msvc") {
            None
        } else if target.contains("darwin") {
            Some("c++".to_string())
        } else if target.contains("freebsd") {
            Some("c++".to_string())
        } else if target.contains("musl") {
            Some("static=stdc++".to_string())
        } else {
            Some("stdc++".to_string())
        }
    })
}

// See https://github.com/alexcrichton/gcc-rs/blob/10871a0e40/src/lib.rs#L1501
fn get_debug() -> bool {
    match env::var("DEBUG").ok() {
        Some(s) => s != "false",
        None => false,
    }
}
