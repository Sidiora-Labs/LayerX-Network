extern crate binaryen;

use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::process;

struct Args {
    input_path: String,
    output_path: String,
    codegen_config: binaryen::CodegenConfig,
}

fn parse_args() -> Result<Args, ()> {
    let args: Vec<_> = env::args().collect();
    if args.len() < 4 {
        return Err(());
    }

    let codegen_config = match &*args[1] {
        "-O0" => binaryen::CodegenConfig {
            optimization_level: 0,
            shrink_level: 0,
            debug_info: true,
        },
        "-O1" => binaryen::CodegenConfig {
            optimization_level: 1,
            shrink_level: 0,
            debug_info: true,
        },
        "-O2" => binaryen::CodegenConfig {
            optimization_level: 2,
            shrink_level: 0,
            debug_info: true,
        },
        "-O3" => binaryen::CodegenConfig {
            optimization_level: 3,
            shrink_level: 0,
            debug_info: true,
        },
        "-O4" => binaryen::CodegenConfig {
            optimization_level: 4,
            shrink_level: 0,
            debug_info: true,
        },
        "-Os" => binaryen::CodegenConfig {
            optimization_level: 2,
            shrink_level: 1,
            debug_info: true,
        },
        "-Oz" => binaryen::CodegenConfig {
            optimization_level: 2,
            shrink_level: 2,
            debug_info: true,
        },
        _ => return Err(()),
    };
    let input_path = args[2].clone();
    let output_path = args[3].clone();

    Ok(Args {
        input_path,
        output_path,
        codegen_config,
    })
}

fn read_module(filename: &str) -> binaryen::Module {
    let mut f = File::open(filename).expect("file not found");
    let mut contents = Vec::new();
    f.read_to_end(&mut contents)
        .expect("something went wrong reading the file");

    binaryen::Module::read(&contents).expect("something went wrong parsing the file")
}

fn write_module(filename: &str, wasm: &[u8]) {
    let mut f = File::create(filename).expect("failed to create output");
    f.write_all(wasm).expect("failed to write file");
}

const USAGE: &'static str = r#"usage: wasm_opt OPT_LEVEL FILENAME

OPT_LEVEL - one of -O0, -O1, -O2, -O3, -Os, -Oz
INPUT     - path to a wasm module to optimize
OUTPUT    - path to write the optimized module
"#;

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(()) => {
            println!("{}", USAGE);
            process::exit(1);
        }
    };
    let mut module = read_module(&args.input_path);
    module.optimize(&args.codegen_config);

    let optimized_wasm = module.write();
    write_module(&args.output_path, &optimized_wasm);
}
