use super::{ChatChunk, ChatProvider, ChatStream, RemoteModel};
use async_trait::async_trait;
use ferrochat_core::AppError;
use futures::stream;
use serde_json::Value;

pub struct Mock;

#[async_trait]
impl ChatProvider for Mock {
    async fn list_models(&self) -> Result<Vec<RemoteModel>, AppError> {
        Ok([
            "mock-model",
            "mock-gpt-4o",
            "mock-gpt-4o-mini",
            "mock-claude-3-5-sonnet",
            "mock-deepseek-reasoner",
        ]
        .into_iter()
        .map(super::model_from_id)
        .collect())
    }

    async fn chat_stream(&self, _model: &str, body: &Value) -> Result<ChatStream, AppError> {
        let messages = body.get("messages").cloned().unwrap_or(Value::Null);
        let text = if messages.to_string().contains("short chat title") {
            "Hello there".to_string()
        } else if messages.to_string().contains("short tags") {
            "notes, rust".to_string()
        } else if let Some(calls) = body.get("tools") {
            if !calls.as_array().map(|a| a.is_empty()).unwrap_or(true)
                && !messages.to_string().contains("\"role\":\"tool\"")
            {
                return Ok(Box::pin(stream::iter(vec![Ok(ChatChunk {
                    tool_calls: vec![super::ToolCallDelta {
                        index: 0,
                        id: Some("call_1".into()),
                        name: Some("echo".into()),
                        arguments: Some("{}".into()),
                    }],
                    done: true,
                    ..Default::default()
                })])));
            }
            "tool-done".to_string()
        } else if let Some(t) = body.pointer("/params/temperature") {
            format!("Hello t={t}")
        } else {
            "Hello".to_string()
        };
        let chunks = vec![
            Ok(ChatChunk {
                content: Some(text),
                ..Default::default()
            }),
            Ok(ChatChunk {
                done: true,
                ..Default::default()
            }),
        ];
        Ok(Box::pin(stream::iter(chunks)))
    }

    async fn check(&self, _model: Option<&str>) -> Result<(), AppError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use serde_json::json;

    #[tokio::test]
    async fn mock_streams_a_token_then_done() {
        let provider = Mock;
        let mut stream = provider
            .chat_stream(
                "mock-model",
                &json!({"messages":[{"role":"user","content":"hi"}]}),
            )
            .await
            .unwrap();
        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(first.content.as_deref(), Some("Hello"));
        let done = stream.next().await.unwrap().unwrap();
        assert!(done.done);
    }
}
