# `binaryen-rs`

[![Build Status](https://travis-ci.org/pepyakin/binaryen-rs.svg?branch=master)](https://travis-ci.org/pepyakin/binaryen-rs) 
[![crates.io](https://img.shields.io/crates/v/binaryen.svg)](https://crates.io/crates/binaryen)
[![docs.rs](https://docs.rs/binaryen/badge.svg)](https://docs.rs/binaryen/)

[Binaryen](https://github.com/WebAssembly/binaryen) bindings for Rust. They used to provide bindings for IR-construction part of the API, but now this crate is more focused on tools provided by Binaryen, such as `translate-to-fuzz` or running wasm optimization passes.

## Alternatives

For `translate-to-fuzz` like functionality, consider using the [wasm-smith](https://crates.io/crates/wasm-smith) crate.
