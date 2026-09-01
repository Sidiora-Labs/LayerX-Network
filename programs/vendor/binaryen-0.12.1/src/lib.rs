pub extern crate binaryen_sys;

#[cfg(test)]
extern crate rand;

#[cfg(test)]
extern crate wat;

pub use binaryen_sys as ffi;

use std::ffi::CString;
use std::os::raw::c_char;
use std::rc::Rc;
use std::str::FromStr;
use std::{ptr, slice};

pub mod tools;

/// Codegen configuration.
#[derive(Default)]
pub struct CodegenConfig {
    /// 0, 1, 2 correspond to -O0, -Os, -Oz
    pub shrink_level: u32,
    /// 0, 1, 2 correspond to -O0, -O1, -O2, etc.
    pub optimization_level: u32,
    /// If set, the names section is emitted.
    pub debug_info: bool,
}

fn is_valid_pass(pass: &str) -> bool {
    ffi::passes::OptimizationPass::from_str(pass).is_ok()
}

struct InnerModule {
    raw: ffi::BinaryenModuleRef,
}

impl Drop for InnerModule {
    fn drop(&mut self) {
        unsafe { ffi::BinaryenModuleDispose(self.raw) }
    }
}

/// Modules contain lists of functions, imports, exports, function types.
pub struct Module {
    inner: Rc<InnerModule>,
}

impl Module {
    /// Create a new empty Module.
    ///
    /// This is not public since all IR-construction related operations were removed from
    /// Binaryen and thus there is not much sense in creating an empty module.
    fn new() -> Module {
        unsafe {
            let raw = ffi::BinaryenModuleCreate();
            Module::from_raw(raw)
        }
    }

    /// Deserialize a module from binary form.
    ///
    /// Returns `Err` if an invalid module is given.
    pub fn read(module: &[u8]) -> Result<Module, ()> {
        unsafe {
            let raw = ffi::BinaryenModuleSafeRead(module.as_ptr() as *const c_char, module.len());
            if raw.is_null() {
                return Err(());
            }
            Ok(Module::from_raw(raw))
        }
    }

    pub unsafe fn from_raw(raw: ffi::BinaryenModuleRef) -> Module {
        Module {
            inner: Rc::new(InnerModule { raw }),
        }
    }

    /// Run the standard optimization passes on the module.
    pub fn optimize(&mut self, codegen_config: &CodegenConfig) {
        unsafe {
            ffi::BinaryenModuleRunPassesWithSettings(
                self.inner.raw,
                std::ptr::null_mut(),
                0 as u32,
                codegen_config.shrink_level as i32,
                codegen_config.optimization_level as i32,
                codegen_config.debug_info as i32,
            )
        }
    }

    /// Run a specified set of optimization passes on the module.
    pub fn run_optimization_passes<B: AsRef<str>, I: IntoIterator<Item = B>>(
        &mut self,
        passes: I,
        codegen_config: &CodegenConfig,
    ) -> Result<(), ()> {
        let mut cstr_vec: Vec<_> = vec![];

        for pass in passes {
            if !is_valid_pass(pass.as_ref()) {
                return Err(());
            }

            cstr_vec.push(CString::new(pass.as_ref()).unwrap());
        }

        // NOTE: BinaryenModuleRunPasses expectes a mutable ptr
        let mut ptr_vec: Vec<_> = cstr_vec.iter().map(|pass| pass.as_ptr()).collect();

        unsafe {
            ffi::BinaryenModuleRunPassesWithSettings(
                self.inner.raw,
                ptr_vec.as_mut_ptr(),
                ptr_vec.len() as u32,
                codegen_config.shrink_level as i32,
                codegen_config.optimization_level as i32,
                codegen_config.debug_info as i32,
            )
        };
        Ok(())
    }

    /// Validate a module, printing errors to stdout on problems.
    ///
    /// This module is private since you can't create an invalid module through the
    /// safe public API.
    #[cfg(test)]
    fn is_valid(&self) -> bool {
        unsafe { ffi::BinaryenModuleSafeValidate(self.inner.raw) == 1 }
    }

    /// Serialize a module into binary form.
    pub fn write(&self) -> Vec<u8> {
        unsafe {
            let write_result = ffi::BinaryenModuleAllocateAndWrite(self.inner.raw, ptr::null());

            // Create a slice from the resulting array and then copy it in vector.
            let binary_buf = if write_result.binaryBytes == 0 {
                vec![]
            } else {
                slice::from_raw_parts(write_result.binary as *const u8, write_result.binaryBytes)
                    .to_vec()
            };

            // This will free buffers in the write_result.
            ffi::BinaryenShimDisposeBinaryenModuleAllocateAndWriteResult(write_result);

            binary_buf
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! wat2wasm {
        ($x:expr) => {
            wat::parse_str($x).unwrap()
        };
    }

    #[test]
    fn module_reading() {
        // The current version of wasm is 1, thus module with the version 0 is invalid.
        let invalid_module = b"\0asm\0\0\0\0";
        let valid_module = b"\0asm\x01\0\0\0";

        assert!(Module::read(invalid_module).is_err());
        assert!(Module::read(valid_module).is_ok());
    }

    #[test]
    fn test_optimization_passes() {
        const CODE: &'static str = r#"
            (module
                (table 1 1 anyfunc)

                (type $return_i32 (func (result i32)))
                (func $test (; 0 ;) (result i32)
                    (call_indirect (type $return_i32)
                        (unreachable)
                    )
                )
            )
        "#;
        let mut module = Module::read(&wat2wasm!(CODE)).unwrap();

        assert!(module.is_valid());

        module
            .run_optimization_passes(&["vacuum", "untee"], &CodegenConfig::default())
            .expect("passes succeeded");

        assert!(module.is_valid());
    }

    #[test]
    fn test_invalid_optimization_passes() {
        let mut module = Module::new();
        assert!(module
            .run_optimization_passes(&["invalid"], &CodegenConfig::default())
            .is_err());
    }

    #[test]
    fn optimization_pass_list() {
        let pass_list = [
            "alignment-lowering",
            "asyncify",
            "avoid-reinterprets",
            "dae",
            "dae-optimizing",
            "coalesce-locals",
            "coalesce-locals-learning",
            "code-pushing",
            "code-folding",
            "const-hoisting",
            "dce",
            "directize",
            "dfo",
            "duplicate-function-elimination",
            "extract-function",
            "emit-target-features",
            "flatten",
            "fpcast-emu",
            "func-metrics",
            "generate-stack-ir",
            "inline-main",
            "inlining",
            "inlining-optimizing",
            "legalize-js-interface",
            "legalize-js-interface-minimally",
            "local-cse",
            "log-execution",
            "i64-to-i32-lowering",
            "instrument-locals",
            "instrument-memory",
            "licm",
            "limit-segments",
            "memory-packing",
            "merge-blocks",
            "merge-locals",
            "metrics",
            "minify-imports",
            "minify-imports-and-exports",
            "mod-asyncify-always-and-only-unwind",
            "mod-asyncify-never-unwind",
            "nm",
            "no-exit-runtime",
            "optimize-added-constants",
            "optimize-added-constants-propagate",
            "optimize-instructions",
            "optimize-stack-ir",
            "pick-load-signs",
            "post-assemblyscript",
            "post-assemblyscript-finalize",
            "post-emscripten",
            "precompute",
            "precompute-propagate",
            "print",
            "print-minified",
            "print-features",
            "print-full",
            "print-call-graph",
            "print-function-map",
            "print-stack-ir",
            "remove-non-js-ops",
            "remove-imports",
            "remove-memory",
            "remove-unused-brs",
            "remove-unused-module-elements",
            "remove-unused-nonfunction-module-elements",
            "remove-unused-names",
            "reorder-functions",
            "reorder-locals",
            "rereloop",
            "rse",
            "roundtrip",
            "safe-heap",
            "simplify-globals",
            "simplify-globals-optimizing",
            "simplify-locals",
            "simplify-locals-nonesting",
            "simplify-locals-notee",
            "simplify-locals-nostructure",
            "simplify-locals-notee-nostructure",
            "souperify",
            "souperify-single-use",
            "spill-pointers",
            "ssa",
            "ssa-nomerge",
            "strip",
            "strip-debug",
            "strip-dwarf",
            "strip-producers",
            "strip-target-features",
            "trap-mode-clamp",
            "trap-mode-js",
            "untee",
            "vacuum",
        ];
        for pass in pass_list.iter() {
            assert!(is_valid_pass(pass), "not a valid pass: {}", pass);
        }
    }

    #[test]
    fn test_smoke_optimize() {
        let input: Vec<u8> = vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x04, 0x01, 0x60, 0x00, 0x00,
            0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00,
            0x08, 0x01, 0x00, 0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b,
        ];
        let expected: Vec<u8> = vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x04, 0x01, 0x60, 0x00, 0x00,
            0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00,
            0x0a, 0x05, 0x01, 0x03, 0x00, 0x01, 0x0b,
        ];

        let mut module = Module::read(&input).unwrap();
        assert!(module.is_valid());
        module.optimize(&CodegenConfig::default());
        assert!(module.is_valid());
        assert_eq!(module.write(), expected);
    }
}
