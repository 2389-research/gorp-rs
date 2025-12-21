use anyhow::Result;
use futures::stream::BoxStream;
use gorp_agent::traits::AgentBackend;
use gorp_agent::AgentEvent;

// Test that a simple mock can implement the trait
struct TestBackend;

impl AgentBackend for TestBackend {
    fn name(&self) -> &'static str {
        "test"
    }

    fn new_session<'a>(&'a self) -> futures::future::BoxFuture<'a, Result<String>> {
        Box::pin(async { Ok("session-1".to_string()) })
    }

    fn load_session<'a>(
        &'a self,
        _session_id: &'a str,
    ) -> futures::future::BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn prompt<'a>(
        &'a self,
        _session_id: &'a str,
        _text: &'a str,
    ) -> futures::future::BoxFuture<'a, Result<BoxStream<'a, AgentEvent>>> {
        Box::pin(async {
            let stream = futures::stream::empty();
            Ok(Box::pin(stream) as BoxStream<'a, AgentEvent>)
        })
    }

    fn cancel<'a>(&'a self, _session_id: &'a str) -> futures::future::BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn test_backend_can_create_session() {
    let backend = TestBackend;
    let session_id = backend.new_session().await.unwrap();
    assert_eq!(session_id, "session-1");
}

#[tokio::test]
async fn test_backend_returns_name() {
    let backend = TestBackend;
    assert_eq!(backend.name(), "test");
}
