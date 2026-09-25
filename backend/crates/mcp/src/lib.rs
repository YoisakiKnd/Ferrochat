//! Minimal MCP client (JSON-RPC 2.0) for stdio and Streamable HTTP.
//! Speaks the same initialize / tools/list / tools/call methods as the MCP spec.
use ferrochat_core::AppError;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::Mutex;

#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub async fn list_tools(kind: &str, config: &Value) -> Result<Vec<ToolSpec>, AppError> {
    let result = rpc(kind, config, "tools/list", json!({})).await?;
    let tools = result
        .get("tools")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(tools
        .into_iter()
        .filter_map(|t| {
            Some(ToolSpec {
                name: t.get("name")?.as_str()?.to_string(),
                description: t
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                input_schema: t
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or(json!({"type":"object"})),
            })
        })
        .collect())
}

pub async fn call_tool(
    kind: &str,
    config: &Value,
    name: &str,
    args: &Value,
) -> Result<String, AppError> {
    let result = rpc(
        kind,
        config,
        "tools/call",
        json!({ "name": name, "arguments": args }),
    )
    .await?;
    if let Some(content) = result.get("content").and_then(|v| v.as_array()) {
        let text = content
            .iter()
            .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            return Ok(text);
        }
    }
    Ok(result.to_string())
}

async fn rpc(kind: &str, config: &Value, method: &str, params: Value) -> Result<Value, AppError> {
    match kind {
        "mcp_stdio" => stdio_rpc(config, method, params).await,
        _ => http_rpc(config, method, params).await,
    }
}

async fn http_rpc(config: &Value, method: &str, params: Value) -> Result<Value, AppError> {
    let url = config.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() {
        return Err(AppError::BadRequest("MCP url is required".into()));
    }
    let client = reqwest::Client::new();
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "ferrochat", "version": "0.1.0" }
        }
    });
    let _ = client.post(url).json(&init).send().await;
    let body = json!({ "jsonrpc": "2.0", "id": 2, "method": method, "params": params });
    let res = client
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    let value: Value = res
        .json()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    if let Some(err) = value.get("error") {
        return Err(AppError::BadRequest(err.to_string()));
    }
    Ok(value.get("result").cloned().unwrap_or(value))
}

static POOL: LazyLock<Mutex<HashMap<String, Arc<Session>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct Session {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    lines: Mutex<tokio::io::Lines<BufReader<tokio::process::ChildStdout>>>,
    next_id: AtomicI64,
    last_used: Mutex<Instant>,
}

impl Session {
    async fn call(&self, method: &str, params: Value) -> Result<Value, AppError> {
        *self.last_used.lock().await = Instant::now();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let call = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.stdin
            .lock()
            .await
            .write_all(format!("{call}\n").as_bytes())
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let mut lines = self.lines.lock().await;
        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
        {
            let value: Value = serde_json::from_str(&line).unwrap_or(json!({}));
            if value.get("id").and_then(|v| v.as_i64()) == Some(id) {
                if let Some(err) = value.get("error") {
                    return Err(AppError::BadRequest(err.to_string()));
                }
                return Ok(value.get("result").cloned().unwrap_or(value));
            }
        }
        Err(AppError::BadRequest("MCP server returned no result".into()))
    }
}

async fn reap_idle_sessions() {
    let mut pool = POOL.lock().await;
    let mut stale = Vec::new();
    for (key, session) in pool.iter() {
        if session.last_used.lock().await.elapsed() > Duration::from_secs(300) {
            stale.push(key.clone());
        }
    }
    for key in stale {
        if let Some(session) = pool.remove(&key) {
            let _ = session.child.lock().await.start_kill();
        }
    }
}

async fn stdio_rpc(config: &Value, method: &str, params: Value) -> Result<Value, AppError> {
    reap_idle_sessions().await;
    let command = config.get("command").and_then(|v| v.as_str()).unwrap_or("");
    if command.is_empty() {
        return Err(AppError::BadRequest("MCP command is required".into()));
    }
    let args: Vec<String> = config
        .get("args")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let key = format!("{command} {}", args.join(" "));
    if let Some(session) = POOL.lock().await.get(&key).cloned() {
        if let Ok(value) = session.call(method, params.clone()).await {
            return Ok(value);
        }
        POOL.lock().await.remove(&key);
    }
    let mut child = tokio::process::Command::new(command)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| AppError::Internal("no stdin".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Internal("no stdout".into()))?;
    let session = Arc::new(Session {
        child: Mutex::new(child),
        stdin: Mutex::new(stdin),
        lines: Mutex::new(BufReader::new(stdout).lines()),
        next_id: AtomicI64::new(1),
        last_used: Mutex::new(Instant::now()),
    });
    let init = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "ferrochat", "version": "0.1.0" } }
    });
    session
        .stdin
        .lock()
        .await
        .write_all(format!("{init}\n").as_bytes())
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let _ = session.next_id.fetch_add(1, Ordering::Relaxed);
    let mut lines = session.lines.lock().await;
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    {
        let value: Value = serde_json::from_str(&line).unwrap_or(json!({}));
        if value.get("id").and_then(|v| v.as_i64()) == Some(1) {
            break;
        }
    }
    drop(lines);
    let value = session.call(method, params).await?;
    POOL.lock().await.insert(key, session);
    Ok(value)
}
