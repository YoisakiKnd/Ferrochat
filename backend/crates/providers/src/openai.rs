use super::{
    apply_headers, model_from_id, openai_body, ChatChunk, ChatProvider, ChatStream, Conn,
    RemoteModel, ToolCallDelta,
};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use ferrochat_core::AppError;
use futures::StreamExt;
use serde_json::{json, Value};

pub struct OpenAi(pub Conn);

#[async_trait]
impl ChatProvider for OpenAi {
    async fn list_models(&self) -> Result<Vec<RemoteModel>, AppError> {
        let url = format!("{}/models", self.0.base_url.trim_end_matches('/'));
        let mut req = self.0.client.get(&url);
        if !self.0.api_key.is_empty() {
            req = req.bearer_auth(&self.0.api_key);
        }
        req = apply_headers(req, &self.0.headers);
        let res = req
            .send()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        let status = res.status();
        let body: Value = res
            .json()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        if !status.is_success() {
            return Err(AppError::BadRequest(body.to_string()));
        }
        let data = body
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut models: Vec<RemoteModel> = data
            .iter()
            .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(model_from_id))
            .filter(|m| !m.capabilities.embedding)
            .collect();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(models)
    }

    async fn chat_stream(&self, model: &str, body: &Value) -> Result<ChatStream, AppError> {
        let url = chat_url(&self.0);
        let payload = openai_body(model, body);
        let mut req = self.0.client.post(url).json(&payload);
        if !self.0.api_key.is_empty() {
            req = req.bearer_auth(&self.0.api_key);
        }
        req = apply_headers(req, &self.0.headers);
        let res = req
            .send()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        if !res.status().is_success() {
            let text = res.text().await.unwrap_or_default();
            return Err(AppError::BadRequest(text));
        }
        let stream = res.bytes_stream().eventsource().map(|event| match event {
            Ok(ev) => {
                if ev.data == "[DONE]" {
                    return Ok(ChatChunk {
                        done: true,
                        ..Default::default()
                    });
                }
                let v: Value = serde_json::from_str(&ev.data).unwrap_or(json!({}));
                if let Some(err) = v.get("error") {
                    return Ok(ChatChunk {
                        error: Some(err.to_string()),
                        done: true,
                        ..Default::default()
                    });
                }
                let delta = v.pointer("/choices/0/delta").cloned().unwrap_or(json!({}));
                let mut chunk = ChatChunk {
                    content: delta
                        .get("content")
                        .and_then(|c| c.as_str())
                        .map(|s| s.to_string()),
                    reasoning: delta
                        .get("reasoning_content")
                        .or_else(|| delta.get("reasoning"))
                        .and_then(|c| c.as_str())
                        .map(|s| s.to_string()),
                    prompt_tokens: v.pointer("/usage/prompt_tokens").and_then(|n| n.as_i64()),
                    completion_tokens: v
                        .pointer("/usage/completion_tokens")
                        .and_then(|n| n.as_i64()),
                    ..Default::default()
                };
                if let Some(anns) = delta.get("annotations").and_then(|v| v.as_array()) {
                    for ann in anns {
                        if let Some(url) = ann.pointer("/url_citation/url").and_then(|v| v.as_str())
                        {
                            let title = ann
                                .pointer("/url_citation/title")
                                .and_then(|v| v.as_str())
                                .unwrap_or(url);
                            chunk.citations.push((title.to_string(), url.to_string()));
                        }
                    }
                }
                if let Some(calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                    for call in calls {
                        chunk.tool_calls.push(ToolCallDelta {
                            index: call.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
                            id: call
                                .get("id")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            name: call
                                .pointer("/function/name")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            arguments: call
                                .pointer("/function/arguments")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                        });
                    }
                }
                Ok(chunk)
            }
            Err(e) => Err(AppError::Internal(e.to_string())),
        });
        Ok(Box::pin(stream))
    }

    async fn check(&self, model: Option<&str>) -> Result<(), AppError> {
        if let Some(model) = model {
            let body = json!({"messages": [{"role": "user", "content": "ping"}]});
            let mut stream = self.chat_stream(model, &body).await?;
            use futures::StreamExt;
            if let Some(item) = stream.next().await {
                item?;
            }
            Ok(())
        } else {
            self.list_models().await.map(|_| ())
        }
    }
}

fn chat_url(conn: &Conn) -> String {
    let base = conn.base_url.trim_end_matches('/');
    if conn.kind == "azure" {
        let version = "2024-10-21";
        if base.contains("?") {
            format!("{base}&api-version={version}")
        } else {
            format!("{base}/chat/completions?api-version={version}")
        }
    } else {
        format!("{base}/chat/completions")
    }
}
