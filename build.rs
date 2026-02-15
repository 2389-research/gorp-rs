// ABOUTME: Build script for conditional code generation
// ABOUTME: Compiles protobuf definitions when coven feature is enabled

fn main() {
    #[cfg(feature = "coven")]
    {
        tonic_build::compile_protos("proto/coven.proto")
            .expect("Failed to compile coven.proto. Is protoc installed?");
    }
}
