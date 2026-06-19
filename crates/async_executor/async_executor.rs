use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// A boxed, sendable future.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Abstract executor for spawning async work.
///
/// Implementations wrap a concrete runtime (tokio, smol, GPUI's executor, etc.)
/// and provide a uniform interface for crates that need to spawn tasks without
/// binding to a specific runtime.
pub trait Executor: Send + Sync + 'static {
    /// Spawn a future on this executor. The returned future resolves when the
    /// spawned work completes.
    fn spawn(&self, future: BoxFuture<()>);

    /// Spawn a future and return a handle that resolves to its output.
    fn spawn_with_result<T: Send + 'static>(
        &self,
        future: BoxFuture<T>,
    ) -> BoxFuture<Option<T>>;

    /// Returns a future that completes after the given duration.
    fn timer(&self, duration: Duration) -> BoxFuture<()>;
}

/// A tokio-based [`Executor`] implementation.
///
/// Wraps a [`tokio::runtime::Handle`] so it can be used from outside a tokio
/// runtime context (e.g., passed to library code that is runtime-agnostic).
#[derive(Clone)]
pub struct TokioExecutor {
    handle: tokio::runtime::Handle,
}

impl TokioExecutor {
    /// Create a new `TokioExecutor` from an explicit runtime handle.
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self { handle }
    }

    /// Create a new `TokioExecutor` from the current tokio runtime.
    ///
    /// Panics if called outside of a tokio runtime context.
    pub fn current() -> Self {
        Self {
            handle: tokio::runtime::Handle::current(),
        }
    }
}

impl Executor for TokioExecutor {
    fn spawn(&self, future: BoxFuture<()>) {
        self.handle.spawn(future);
    }

    fn spawn_with_result<T: Send + 'static>(
        &self,
        future: BoxFuture<T>,
    ) -> BoxFuture<Option<T>> {
        let join_handle = self.handle.spawn(future);
        Box::pin(async move { join_handle.await.ok() })
    }

    fn timer(&self, duration: Duration) -> BoxFuture<()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn test_tokio_executor_spawn() {
        let executor = TokioExecutor::current();
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = flag.clone();
        executor.spawn(Box::pin(async move {
            flag_clone.store(true, Ordering::SeqCst);
        }));
        // Give the spawned task time to run.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(flag.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_tokio_executor_spawn_with_result() {
        let executor = TokioExecutor::current();
        let result = executor
            .spawn_with_result(Box::pin(async { 42u32 }))
            .await;
        assert_eq!(result, Some(42));
    }

    #[tokio::test]
    async fn test_tokio_executor_timer() {
        let executor = TokioExecutor::current();
        let start = std::time::Instant::now();
        executor.timer(Duration::from_millis(50)).await;
        assert!(start.elapsed() >= Duration::from_millis(40));
    }
}
