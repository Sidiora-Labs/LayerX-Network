use layerx_programs::naming::reference_interface;
use layerx_programs_runtime::ProgramId;
use std::{env, error::Error, fs};

fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<_> = env::args().collect();
    if arguments.len() != 4 {
        return Err("usage: naming_interface WASM PROGRAM_HEX INTERFACE_OUT".into());
    }
    let mut program = [0; 32];
    if arguments[2].len() != 64 {
        return Err("program id must have 64 lowercase hex digits".into());
    }
    for (slot, pair) in program
        .iter_mut()
        .zip(arguments[2].as_bytes().chunks_exact(2))
    {
        if !pair
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            return Err("noncanonical program id".into());
        }
        *slot = u8::from_str_radix(std::str::from_utf8(pair)?, 16)?;
    }
    let program = ProgramId::new(program)?;
    let interface = reference_interface(&fs::read(&arguments[1])?, program)?;
    fs::write(&arguments[3], interface.canonical_encoding())?;
    for byte in interface.digest().as_bytes() {
        print!("{byte:02x}");
    }
    println!();
    Ok(())
}
