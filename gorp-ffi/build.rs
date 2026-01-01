// ABOUTME: UniFFI build script for generating scaffolding code.
// ABOUTME: Processes the UDL file to create FFI bindings at compile time.

fn main() {
    uniffi::generate_scaffolding("src/gorp_ffi.udl").unwrap();
}
