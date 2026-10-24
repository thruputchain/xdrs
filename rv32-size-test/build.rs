// Puts link/rv32.ld on the linker search path, as the SDK's rv32 runtime
// build script does, so .cargo/config.toml can name it with -T.
fn main() {
    println!("cargo:rerun-if-changed=link/rv32.ld");
    println!("cargo:rerun-if-changed=build.rs");
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-search={manifest_dir}/link");
}
