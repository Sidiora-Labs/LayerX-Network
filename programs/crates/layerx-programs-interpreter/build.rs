fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("TARGET").as_deref() == Ok("wasm32-unknown-unknown") {
        println!("cargo:rustc-link-arg=--strip-debug");
        println!("cargo:rustc-link-arg=--compress-relocations");
    }
}
