// ABOUTME: UniFFI bindings for gorp-agent and gorp-core.
// ABOUTME: Enables Swift/Kotlin integration for native apps.

mod error;
mod runtime;

pub use error::FfiError;

uniffi::setup_scaffolding!();
