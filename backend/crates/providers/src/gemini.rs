use super::{apply_headers, model_from_id, ChatChunk, ChatProvider, ChatStream, Conn, RemoteModel};
use async_trait::async_trait;
use ferrochat_core::AppError;
use futures::StreamExt;
use serde_json::{json, Value};

pub struct Gemini(pub Conn);

#[async_trait]
impl ChatProvider for Gemini {
    async fn list_models(&self) -> Result<Vec<RemoteModel>, AppError> {
        let url = format!(
            "{}/models?key={}",
            self.0.base_url.trim_end_matches('/'),
            self.0.api_key
        );
        let req = apply_headers(self.0.client.get(url), &self.0.headers);
        let body: Value = req
            .send()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?
            .json()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        let data = body
            .get("models")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(data
            .iter()
            .filter_map(|m| {
                m.get("name").and_then(|v| v.as_str()).map(|name| {
                    let id = name.trim_start_matches("models/");
                    model_from_id(id)
                })
            })
            .filter(|m| m.id.starts_with("gemini") && !m.capabilities.embedding)
            .collect())
    }

    async fn chat_stream(&self, model: &str, body: &Value) -> Result<ChatStream, AppError> {
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            self.0.base_url.trim_end_matches('/'),
            model,
            self.0.api_key
        );
        let payload = to_gemini(body);
        let req = apply_headers(self.0.client.post(url).json(&payload), &self.0.headers);
        let res = req
            .send()
            .await
            .map_err(|e| AppError::BadRequest(e.to_string()))?;
        if !res.status().is_success() {
            return Err(AppError::BadRequest(res.text().await.unwrap_or_default()));
        }
        let stream = res.bytes_stream().map(|chunk| {
            let bytes = chunk.map_err(|e| AppError::Internal(e.to_string()))?;
            let text = String::from_utf8_lossy(&bytes);
            let mut out = ChatChunk::default();
            for line in text.lines() {
                let data = line.trim().strip_prefix("data:").unwrap_or("").trim();
                if data.is_empty() {
                    continue;
                }
                let v: Value = serde_json::from_str(data).unwrap_or(json!({}));
                if let Some(parts) = v
                    .pointer("/candidates/0/content/parts")
                    .and_then(|p| p.as_array())
                {
                    for part in parts {
                        if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                            if part
                                .get("thought")
                                .and_then(|t| t.as_bool())
                                .unwrap_or(false)
                            {
                                out.reasoning = Some(t.to_string());
                            } else {
                                out.content = Some(t.to_string());
                            }
                        }
                    }
                }
            }
            Ok(out)
        });
        Ok(Box::pin(stream))
    }

    async fn check(&self, model: Option<&str>) -> Result<(), AppError> {
        let model = model.unwrap_or("gemini-2.0-flash");
        let body = json!({"messages":[{"role":"user","content":"ping"}]});
        let mut stream = self.chat_stream(model, &body).await?;
        if let Some(item) = stream.next().await {
            item?;
        }
        Ok(())
    }
}

fn to_gemini(incoming: &Value) -> Value {
    let messages = incoming
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut system = String::new();
    let mut contents = Vec::new();
    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let content = msg.get("content").cloned().unwrap_or(json!(""));
        let text = content.as_str().unwrap_or("").to_string();
        if role == "system" {
            system.push_str(&text);
        } else {
            let parts =
                super::provider_content("gemini", &content).unwrap_or(json!([{"text": text}]));
            contents.push(json!({
                "role": if role == "assistant" { "model" } else { "user" },
                "parts": parts
            }));
        }
    }
    let mut body = json!({ "contents": contents });
    if !system.is_empty() {
        body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
    }
    body
}
