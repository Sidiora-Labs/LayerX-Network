fn main() {
    println!("cargo:rustc-check-cfg=cfg(slow_assertions)");
}
