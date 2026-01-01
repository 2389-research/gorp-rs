// ABOUTME: UniFFI build script for generating scaffolding code.
// ABOUTME: Processes the UDL file to create FFI bindings at compile time.

fn main() {
    let udl_path = "src/gorp_ffi.udl";

    if !std::path::Path::new(udl_path).exists() {
        panic!(
            "UniFFI UDL file not found at '{}'. \
            This file defines the FFI interface. \
            Run 'cargo build' from the gorp-ffi directory.",
            udl_path
        );
    }

    uniffi::generate_scaffolding(udl_path).expect("Failed to generate UniFFI scaffolding");
}
