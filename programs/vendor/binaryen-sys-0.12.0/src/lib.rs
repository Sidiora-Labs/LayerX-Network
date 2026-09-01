#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

mod bindings;
pub use bindings::*;
pub mod passes;

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::ptr;

    #[test]
    fn test_fuzz() {
        let vec: Vec<u8> = vec![0, 1, 2, 3, 4, 5];
        unsafe {
            let module = translateToFuzz(vec.as_ptr() as *const i8, vec.len(), true);
            let result = BinaryenModuleValidate(module);
            assert!(result != 0);
        }
    }

    #[test]
    fn test_sanity() {
        // see https://github.com/WebAssembly/binaryen/blob/master/test/example/c-api-hello-world.c
        unsafe {
            let module = BinaryenModuleCreate();
            let mut params = [BinaryenTypeInt32(), BinaryenTypeInt32()];

            let params = BinaryenTypeCreate(params.as_mut_ptr(), 2);
            let results = BinaryenTypeInt32();

            let x = BinaryenLocalGet(module, 0, BinaryenTypeInt32());
            let y = BinaryenLocalGet(module, 1, BinaryenTypeInt32());
            let add = BinaryenBinary(module, BinaryenAddInt32(), x, y);

            let func_name = CString::new("adder").unwrap();
            let _ = BinaryenAddFunction(
                module,
                func_name.as_ptr(),
                params,
                results,
                ptr::null_mut(),
                0,
                add,
            );

            BinaryenModulePrint(module);
            BinaryenModuleDispose(module);
        }
    }
}
