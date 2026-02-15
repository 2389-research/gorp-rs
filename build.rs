// ABOUTME: Build script for conditional code generation and compile-time validation
// ABOUTME: Compiles protobuf definitions when coven feature is enabled, warns on missing platforms

fn main() {
    // Compile coven gateway protobuf definitions
    #[cfg(feature = "coven")]
    {
        tonic_build::compile_protos("proto/coven.proto")
            .expect("Failed to compile coven.proto. Is protoc installed?");
    }

    // Warn if no platform features are enabled
    let has_matrix = cfg!(feature = "matrix");
    let has_telegram = cfg!(feature = "telegram");
    let has_slack = cfg!(feature = "slack");
    let has_whatsapp = cfg!(feature = "whatsapp");

    if !has_matrix && !has_telegram && !has_slack && !has_whatsapp {
        println!(
            "cargo::warning=No platform features enabled. \
             Enable at least one: matrix, telegram, slack, whatsapp"
        );
    }

    // Warn if no interface features are enabled (headless-only)
    let has_gui = cfg!(feature = "gui");
    let has_tui = cfg!(feature = "tui");
    let has_admin = cfg!(feature = "admin");
    let has_coven = cfg!(feature = "coven");

    if !has_gui && !has_tui && !has_admin && !has_coven {
        println!(
            "cargo::warning=No interface features enabled. \
             The binary will run headless with CLI-only control."
        );
    }
}
