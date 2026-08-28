//! Deterministic WASM binary construction for validation and execution tests.

/// The `i32` value type code.
pub const TYPE_I32: u8 = 0x7f;
/// The `i64` value type code.
pub const TYPE_I64: u8 = 0x7e;
/// The 32-bit float value type code.
pub const TYPE_F32: u8 = 0x7d;
/// The 64-bit float value type code.
pub const TYPE_F64: u8 = 0x7c;
/// The `v128` value type code.
pub const TYPE_V128: u8 = 0x7b;

/// The `end` opcode.
pub const OP_END: u8 = 0x0b;
/// The `call` opcode.
pub const OP_CALL: u8 = 0x10;
/// The `drop` opcode.
pub const OP_DROP: u8 = 0x1a;
/// The `local.get` opcode.
pub const OP_LOCAL_GET: u8 = 0x20;
/// The `i32.const` opcode.
pub const OP_I32_CONST: u8 = 0x41;
/// The 32-bit float constant opcode.
pub const OP_F32_CONST: u8 = 0x43;
/// The `i32.add` opcode.
pub const OP_I32_ADD: u8 = 0x6a;
/// The `i32.div_s` opcode.
pub const OP_I32_DIV_S: u8 = 0x6d;

const SECTION_TYPE: u8 = 1;
const SECTION_IMPORT: u8 = 2;
const SECTION_FUNCTION: u8 = 3;
const SECTION_EXPORT: u8 = 7;
const SECTION_CODE: u8 = 10;
const SECTION_CUSTOM: u8 = 0;
const FUNC_TYPE: u8 = 0x60;
const KIND_FUNC: u8 = 0x00;

/// Encodes an unsigned integer as LEB128.
#[must_use]
pub fn unsigned_leb(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let byte = u8::try_from(value & 0x7f).unwrap_or(0);
        value >>= 7;
        if value == 0 {
            bytes.push(byte);
            return bytes;
        }
        bytes.push(byte | 0x80);
    }
}

fn name(text: &str) -> Vec<u8> {
    let mut bytes = unsigned_leb(text.len() as u64);
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

fn section(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![id];
    bytes.extend(unsigned_leb(payload.len() as u64));
    bytes.extend_from_slice(payload);
    bytes
}

/// Encodes a section from its numeric identifier and already assembled payload.
///
/// This is intentionally low-level so committed execution corpora can exercise
/// proposal instructions without adding production dependencies on a WAT parser.
#[must_use]
pub fn raw_section(id: u8, payload: &[u8]) -> Vec<u8> {
    section(id, payload)
}

/// Encodes a type section from `(params, results)` value type code pairs.
#[must_use]
pub fn type_section(types: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut payload = unsigned_leb(types.len() as u64);
    for (params, results) in types {
        payload.push(FUNC_TYPE);
        payload.extend(unsigned_leb(params.len() as u64));
        payload.extend_from_slice(params);
        payload.extend(unsigned_leb(results.len() as u64));
        payload.extend_from_slice(results);
    }
    section(SECTION_TYPE, &payload)
}

/// Encodes an import section of function imports as `(module, name, type index)`.
#[must_use]
pub fn import_section(imports: &[(&str, &str, u32)]) -> Vec<u8> {
    let mut payload = unsigned_leb(imports.len() as u64);
    for (import_module, import_name, type_index) in imports {
        payload.extend(name(import_module));
        payload.extend(name(import_name));
        payload.push(KIND_FUNC);
        payload.extend(unsigned_leb(u64::from(*type_index)));
    }
    section(SECTION_IMPORT, &payload)
}

/// Encodes a function section from type indices.
#[must_use]
pub fn function_section(type_indices: &[u32]) -> Vec<u8> {
    let mut payload = unsigned_leb(type_indices.len() as u64);
    for type_index in type_indices {
        payload.extend(unsigned_leb(u64::from(*type_index)));
    }
    section(SECTION_FUNCTION, &payload)
}

/// Encodes an export section of function exports as `(name, function index)`.
#[must_use]
pub fn export_section(exports: &[(&str, u32)]) -> Vec<u8> {
    let mut payload = unsigned_leb(exports.len() as u64);
    for (export_name, function_index) in exports {
        payload.extend(name(export_name));
        payload.push(KIND_FUNC);
        payload.extend(unsigned_leb(u64::from(*function_index)));
    }
    section(SECTION_EXPORT, &payload)
}

/// Encodes one function body from local declarations and raw instructions.
#[must_use]
pub fn func_body(locals: &[(u32, u8)], instructions: &[u8]) -> Vec<u8> {
    let mut body = unsigned_leb(locals.len() as u64);
    for (count, value_type) in locals {
        body.extend(unsigned_leb(u64::from(*count)));
        body.push(*value_type);
    }
    body.extend_from_slice(instructions);
    body
}

/// Encodes a code section from assembled function bodies.
#[must_use]
pub fn code_section(bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut payload = unsigned_leb(bodies.len() as u64);
    for body in bodies {
        payload.extend(unsigned_leb(body.len() as u64));
        payload.extend_from_slice(body);
    }
    section(SECTION_CODE, &payload)
}

/// Encodes a custom section padded with zero bytes to the given length.
#[must_use]
pub fn padding_section(padding: usize) -> Vec<u8> {
    let mut payload = name("padding");
    payload.extend(core::iter::repeat_n(0u8, padding));
    section(SECTION_CUSTOM, &payload)
}

/// Assembles a module from encoded sections.
#[must_use]
pub fn module(sections: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    for encoded in sections {
        bytes.extend_from_slice(encoded);
    }
    bytes
}

/// Builds a module exporting `add`, summing two `i32` parameters.
#[must_use]
pub fn add_module() -> Vec<u8> {
    module(&[
        type_section(&[(&[TYPE_I32, TYPE_I32], &[TYPE_I32])]),
        function_section(&[0]),
        export_section(&[("add", 0)]),
        code_section(&[func_body(
            &[],
            &[OP_LOCAL_GET, 0, OP_LOCAL_GET, 1, OP_I32_ADD, OP_END],
        )]),
    ])
}
