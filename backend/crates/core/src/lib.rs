use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::path::PathBuf;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub secret_key: Option<String>,
    pub frontend_dir: Option<PathBuf>,
}

impl Config {
    pub fn from_env() -> Self {
        let data_dir = env::var("FERROCHAT_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data"));
        Self {
            host: env::var("FERROCHAT_HOST").unwrap_or_else(|_| "0.0.0.0".into()),
            port: env::var("FERROCHAT_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8080),
            data_dir,
            secret_key: env::var("FERROCHAT_SECRET_KEY")
                .ok()
                .filter(|s| !s.is_empty()),
            frontend_dir: env::var("FERROCHAT_FRONTEND_DIR")
                .ok()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
        }
    }

    pub fn database_url(&self) -> String {
        let path = self.data_dir.join("ferrochat.db");
        format!("sqlite:{}?mode=rwc", path.display())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn status(&self) -> u16 {
        match self {
            AppError::BadRequest(_) => 400,
            AppError::Unauthorized(_) => 401,
            AppError::NotFound(_) => 404,
            AppError::Internal(_) => 500,
        }
    }

    pub fn json(&self) -> Value {
        json!({ "detail": self.to_string() })
    }
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let status = axum::http::StatusCode::from_u16(self.status())
            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        (status, axum::Json(self.json())).into_response()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    pub vision: bool,
    pub reasoning: bool,
    pub tools: bool,
    pub web: bool,
    pub embedding: bool,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            vision: false,
            reasoning: false,
            tools: true,
            web: false,
            embedding: false,
        }
    }
}

pub fn infer_capabilities(model_id: &str) -> Capabilities {
    let id = model_id.to_ascii_lowercase();
    let embedding = id.contains("embed") || id.contains("embedding");
    let vision = !embedding
        && (id.contains("vision")
            || id.contains("gpt-4o")
            || id.contains("gpt-4.1")
            || id.contains("gpt-5")
            || id.contains("claude-3")
            || id.contains("claude-4")
            || id.contains("gemini")
            || id.contains("llava")
            || id.contains("pixtral")
            || id.contains("qwen-vl")
            || id.contains("qwen2-vl")
            || id.contains("qwen2.5-vl"));
    let reasoning = id.contains("reason")
        || id.contains("thinking")
        || id.contains("o1")
        || id.contains("o3")
        || id.contains("o4")
        || id.contains("deepseek-r1")
        || id.contains("qwq")
        || id.contains("gemini-2.5");
    let web = id.contains("search") || id.contains("online");
    Capabilities {
        vision,
        reasoning,
        tools: !embedding,
        web,
        embedding,
    }
}

pub fn group_name(model_id: &str) -> String {
    let id = model_id.trim();
    let lower = id.to_ascii_lowercase();
    for prefix in [
        "gpt-4.1",
        "gpt-4o",
        "gpt-4",
        "gpt-3.5",
        "gpt-5",
        "o1",
        "o3",
        "o4",
        "claude-opus",
        "claude-sonnet",
        "claude-haiku",
        "claude-3",
        "claude-4",
        "gemini-2.5",
        "gemini-2.0",
        "gemini-1.5",
        "gemini",
        "deepseek",
        "qwen",
        "llama",
        "mistral",
        "grok",
    ] {
        if lower.starts_with(prefix) {
            return prefix.to_string();
        }
    }
    id.split(['-', ':', '/']).next().unwrap_or(id).to_string()
}

pub fn permissions() -> Value {
    json!({
        "workspace": { "models": true, "knowledge": false, "prompts": true, "tools": true },
        "sharing": { "public_models": false, "public_knowledge": false, "public_prompts": false, "public_tools": false },
        "chat": {
            "controls": true, "file_upload": true, "delete": true, "edit": true,
            "stt": false, "tts": false, "call": false, "multiple_models": true,
            "temporary": true, "temporary_enforced": false
        },
        "features": {
            "direct_tool_servers": true, "web_search": false,
            "image_generation": false, "code_interpreter": false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::infer_capabilities;

    #[test]
    fn capability_rules_cover_common_models() {
        let gpt = infer_capabilities("gpt-4o");
        assert!(gpt.vision);
        assert!(!gpt.embedding);
        let embed = infer_capabilities("text-embedding-3-large");
        assert!(embed.embedding);
        assert!(!embed.vision);
        let reason = infer_capabilities("deepseek-r1");
        assert!(reason.reasoning);
    }
}

pub fn now() -> i64 {
    chrono_now()
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
