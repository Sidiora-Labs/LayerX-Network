# wasm-instrument

A Rust library containing a collection of wasm module instrumentations and transformations
mainly useful for wasm based block chains and smart contracts.

## Provided functionality

This is a non exhaustive list of provided functionality. Please check out the [documentation](https://docs.rs/wasm-instrument/latest/wasm_instrument/) for details.

### Gas Metering

Add gas metering to your platform by injecting the necessary code directly into the wasm module. This allows having a uniform gas metering implementation across different execution engines (interpreters, JIT compilers).

### Stack Height Limiter

Neither the wasm standard nor any sufficiently complex execution engine specifies how many items on the wasm stack are supported before the execution aborts or malfunctions. Even the same execution engine on different operating systems or host architectures could support a different number of stack items and be well within its rights.

This is the kind of indeterminism that can lead to consensus failures when used in a blockchain context.

To address this issue we can inject some code that meters the stack height at runtime and aborts the execution when it reaches a predefined limit. Choosing this limit suffciently small so that it is smaller than what any reasonably parameterized execution engine would support solves the issue: All execution engines would reach the injected limit before hitting any implementation specific limitation.

## License

`wasm-instrument` is distributed under the terms of both the MIT license and the
Apache License (Version 2.0), at your choice.

See LICENSE-APACHE, and LICENSE-MIT for details.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in `wasm-instrument` by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
