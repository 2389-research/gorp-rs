// ABOUTME: Embedded Tokio runtime for FFI async operations.
// ABOUTME: Lazy-initialized, lives for app lifetime.

use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::runtime::Runtime;

static RUNTIME: Lazy<Arc<Runtime>> = Lazy::new(|| {
    Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("gorp-ffi")
            .build()
            .expect("Failed to create Tokio runtime"),
    )
});

/// Get reference to the shared runtime
pub fn runtime() -> &'static Runtime {
    &RUNTIME
}

/// Block on an async operation from sync FFI context
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    RUNTIME.block_on(f)
}

/// Spawn an async task for background execution
pub fn spawn<F>(f: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    RUNTIME.spawn(f)
}
