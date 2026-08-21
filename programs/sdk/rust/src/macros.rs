/// Defines the canonical `layerx_main` export over a typed handler.
///
/// The handler takes the integer argument the runtime passes and returns
/// either an integer result or a [`ProgramError`](crate::ProgramError). A
/// refusal surfaces as the frozen negative host status, so a successful
/// handler returns a non-negative value.
#[macro_export]
macro_rules! program {
    ($handler:path) => {
        #[allow(unsafe_code)]
        #[no_mangle]
        pub extern "C" fn layerx_main(input: i64) -> i64 {
            let handler: fn(i64) -> ::core::result::Result<i64, $crate::ProgramError> = $handler;
            match handler(input) {
                ::core::result::Result::Ok(value) => value,
                ::core::result::Result::Err(error) => $crate::ProgramError::status(error),
            }
        }
    };
}

/// Defines the canonical byte entry used identically by activities and nested calls.
///
/// The handler takes the input bytes the caller wrote into the SDK's declared
/// reservation and returns either a [`CallResult`](crate::CallResult) or a
/// [`ProgramError`](crate::ProgramError). A refusal surfaces as the frozen
/// negative host status, which the composition layer treats as a refusal of
/// the whole call graph.
#[macro_export]
macro_rules! entrypoint {
    ($handler:path) => {
        #[allow(unsafe_code)]
        #[no_mangle]
        pub extern "C" fn layerx_reserve(length: i32) -> i32 {
            $crate::entry::reserve_call_input(length)
        }

        #[allow(unsafe_code)]
        #[no_mangle]
        pub extern "C" fn layerx_call(input_pointer: i32, input_length: i32) -> i32 {
            let handler: fn(
                &[u8],
            )
                -> ::core::result::Result<$crate::CallResult, $crate::ProgramError> = $handler;
            match $crate::entry::with_call_input(input_pointer, input_length, handler) {
                ::core::result::Result::Ok(result) => match result {
                    ::core::result::Result::Ok(result) => $crate::CallResult::code(result),
                    ::core::result::Result::Err(error) => $crate::ProgramError::code(error),
                },
                ::core::result::Result::Err(error) => $crate::ProgramError::code(error),
            }
        }
    };
}

/// Compatibility alias for the canonical [`entrypoint!`](crate::entrypoint) macro.
#[macro_export]
macro_rules! callable {
    ($handler:path) => {
        $crate::entrypoint!($handler);
    };
}

/// Defines a candidate entry that synchronously publishes bounded response bytes.
#[macro_export]
macro_rules! response_entrypoint {
    ($handler:path) => {
        #[allow(unsafe_code)]
        #[no_mangle]
        pub extern "C" fn layerx_reserve(length: i32) -> i32 {
            $crate::entry::reserve_call_input(length)
        }

        #[allow(unsafe_code)]
        #[no_mangle]
        pub extern "C" fn layerx_call(input_pointer: i32, input_length: i32) -> i32 {
            match $crate::entry::with_call_input(
                input_pointer,
                input_length,
                |input| match $handler(input) {
                    ::core::result::Result::Ok(response) => {
                        match $crate::call::publish_response(response.result, response.bytes) {
                            ::core::result::Result::Ok(()) => response.result.code(),
                            ::core::result::Result::Err(error) => error.code(),
                        }
                    }
                    ::core::result::Result::Err(error) => error.code(),
                },
            ) {
                ::core::result::Result::Ok(code) => code,
                ::core::result::Result::Err(error) => error.code(),
            }
        }
    };
}

/// Defines a candidate entry that publishes either a successful response or a typed refusal.
#[macro_export]
macro_rules! failure_entrypoint {
    ($handler:path) => {
        #[allow(unsafe_code)]
        #[no_mangle]
        pub extern "C" fn layerx_reserve(length: i32) -> i32 {
            $crate::entry::reserve_call_input(length)
        }

        #[allow(unsafe_code)]
        #[no_mangle]
        pub extern "C" fn layerx_call(input_pointer: i32, input_length: i32) -> i32 {
            match $crate::entry::with_call_input(
                input_pointer,
                input_length,
                |input| match $handler(input) {
                    ::core::result::Result::Ok(response) => {
                        match $crate::call::publish_response(response.result, response.bytes) {
                            ::core::result::Result::Ok(()) => response.result.code(),
                            ::core::result::Result::Err(error) => error.code(),
                        }
                    }
                    ::core::result::Result::Err(refusal) => {
                        match $crate::call::publish_refusal(refusal) {
                            ::core::result::Result::Ok(()) => $crate::CANDIDATE_REFUSAL_SENTINEL,
                            ::core::result::Result::Err(error) => error.code(),
                        }
                    }
                },
            ) {
                ::core::result::Result::Ok(code) => code,
                ::core::result::Result::Err(error) => error.code(),
            }
        }
    };
}

/// Defines the panic handler a `no_std` program needs.
///
/// A panic traps the guest through the WebAssembly `unreachable` instruction,
/// which the runtime reports as an unreachable-execution fault and which
/// discards every staged write and effect.
#[macro_export]
macro_rules! trap_on_panic {
    () => {
        #[panic_handler]
        fn layerx_program_panic(_information: &::core::panic::PanicInfo<'_>) -> ! {
            ::core::arch::wasm32::unreachable()
        }
    };
}
