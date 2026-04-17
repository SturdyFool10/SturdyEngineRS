fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        println!("cargo:rustc-link-lib=slang-compiler");
        println!("cargo:rerun-if-changed=build.rs");
    }
}
