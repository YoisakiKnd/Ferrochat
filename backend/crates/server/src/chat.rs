use crate::auth::CurrentUser;
use crate::App;
use ferrochat_core::AppError;
use ferrochat_mcp::{self, ToolSpec};
use ferrochat_providers::{build, next_key, ChatChunk, Conn};
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

pub async fn run(app: Arc<App>, user: CurrentUser, form: Value) -> Result<Value, AppError> {
    let task_id = Uuid::new_v4().to_string();
    let cancel = tokio_util::sync::CancellationToken::new();
    app.tasks
        .lock()
        .unwrap()
        .insert(task_id.clone(), cancel.clone());
    if let Some(chat_id) = form.get("chat_id").and_then(|v| v.as_str()) {
        app.chat_tasks
            .lock()
            .unwrap()
            .entry(chat_id.to_string())
            .or_default()
            .push(task_id.clone());
    }
    let app2 = app.clone();
    let form_for_task = form.clone();
    let task_for_cleanup = task_id.clone();
    tokio::spawn(async move {
        if let Err(err) = drive(app2.clone(), &user, &form_for_task, &cancel).await {
            emit(
                &app2,
                &form_for_task,
                json!({"type": "chat:completion", "data": {"error": {"message": err.to_string()}}}),
            );
            emit(
                &app2,
                &form_for_task,
                json!({"type": "chat:completion", "data": {"done": true, "content": ""}}),
            );
        }
        app2.tasks.lock().unwrap().remove(&task_for_cleanup);
    });
    Ok(json!({"status": true, "task_id": task_id}))
}

async fn drive(
    app: Arc<App>,
    user: &CurrentUser,
    form: &Value,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<(), AppError> {
    let requested = form
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let (provider_id, model_id, system, extra_tools) = resolve(&app, &requested).await?;
    let provider = app.db.provider(&provider_id).await?;
    if !provider["enabled"].as_bool().unwrap_or(false) {
        return Err(AppError::BadRequest(format!(
            "provider {provider_id} is disabled"
        )));
    }
    let tick = app
        .key_tick
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let cooled = app
        .db
        .cooled_fingerprints(&provider_id)
        .await
        .unwrap_or_default();
    let api_key = next_key_skip(provider["api_keys"].as_str().unwrap_or(""), tick, &cooled);
    let key_fp = fingerprint(&api_key);
    let conn = Conn {
        kind: provider["type"].as_str().unwrap_or("openai").to_string(),
        base_url: provider["base_url"].as_str().unwrap_or("").to_string(),
        api_key,
        headers: provider["headers"].clone(),
        client: shared_client(),
    };
    let adapter = build(conn);
    let vision = app
        .db
        .provider_model_vision(&provider_id, &model_id)
        .await
        .unwrap_or(true);
    let mut messages = fold_context(
        form.get("messages").cloned().unwrap_or(json!([])),
        form.get("files"),
        vision,
        &app.data_dir,
    );
    let defaults = app
        .db
        .model_params(&provider_id, &model_id)
        .await
        .unwrap_or(json!({}));
    let has_system = messages
        .as_array()
        .map(|a| a.iter().any(|m| m["role"] == "system"))
        .unwrap_or(false);
    let system = system.or_else(|| {
        (!has_system)
            .then(|| defaults["system"].as_str().filter(|s| !s.is_empty()))
            .flatten()
            .map(String::from)
    });
    if let Some(system) = system {
        if let Some(arr) = messages.as_array_mut() {
            arr.insert(
                0,
                json!({"role": "system", "content": substitute(&system, user)}),
            );
        }
    }
    let mut tools = load_tools(&app, form, extra_tools).await;
    let mut tool_schemas = openai_tools(&tools);
    let mut full = String::new();
    let mut reasoning = String::new();
    let mut reasoning_open = false;

    for _round in 0..5 {
        if cancel.is_cancelled() {
            break;
        }
        let mut body = form.clone();
        body["messages"] = messages.clone();
        body["params"] = merge_params(&defaults, body.get("params"));
        if !tool_schemas.is_empty() {
            body["tools"] = json!(tool_schemas);
        }
        let mut stream = adapter.chat_stream(&model_id, &body).await?;
        let mut calls: Vec<Accum> = Vec::new();
        while let Some(item) = stream.next().await {
            if cancel.is_cancelled() {
                break;
            }
            let chunk = item?;
            if let Some(err) = chunk.error {
                let _ = app.db.mark_key_error(&provider_id, &key_fp, &err, 60).await;
                return Err(AppError::BadRequest(err));
            }
            apply_chunk(
                &app,
                form,
                &chunk,
                &mut full,
                &mut reasoning,
                &mut reasoning_open,
            );
            for delta in chunk.tool_calls {
                let slot = if delta.index >= calls.len() {
                    calls.resize(delta.index + 1, Accum::default());
                    &mut calls[delta.index]
                } else {
                    &mut calls[delta.index]
                };
                if let Some(id) = delta.id {
                    slot.id = id;
                }
                if let Some(name) = delta.name {
                    slot.name = name;
                }
                if let Some(args) = delta.arguments {
                    slot.arguments.push_str(&args);
                }
            }
            if chunk.done {
                break;
            }
        }
        if reasoning_open {
            full.push_str("\n</details>\n");
            reasoning_open = false;
        }
        let pending: Vec<Accum> = calls.into_iter().filter(|c| !c.name.is_empty()).collect();
        if pending.is_empty() || tools.is_empty() {
            break;
        }
        let mut assistant_calls = Vec::new();
        for (i, call) in pending.iter().enumerate() {
            assistant_calls.push(json!({
                "id": if call.id.is_empty() { format!("call_{i}") } else { call.id.clone() },
                "type": "function",
                "function": { "name": call.name, "arguments": call.arguments }
            }));
        }
        if let Some(arr) = messages.as_array_mut() {
            arr.push(json!({"role": "assistant", "content": full, "tool_calls": assistant_calls}));
        }
        for (i, call) in pending.iter().enumerate() {
            emit_status(&app, form, &format!("Calling {}", call.name));
            let args: Value = serde_json::from_str(&call.arguments).unwrap_or(json!({}));
            let output = match find_tool(&tools, &call.name) {
                Some((kind, cfg, name)) => ferrochat_mcp::call_tool(kind, cfg, name, &args)
                    .await
                    .unwrap_or_else(|e| e.to_string()),
                None => format!("unknown tool {}", call.name),
            };
            if let Some(arr) = messages.as_array_mut() {
                arr.push(json!({
                    "role": "tool",
                    "tool_call_id": if call.id.is_empty() { format!("call_{i}") } else { call.id.clone() },
                    "content": output
                }));
            }
        }
        tool_schemas.clear();
        tools.clear();
    }

    let title = maybe_title(&app, &adapter, &model_id, form, &full).await;
    emit(
        &app,
        form,
        json!({"type": "chat:completion", "data": {"done": true, "content": full, "title": title}}),
    );
    if let (Some(chat_id), Some(title)) = (
        form.get("chat_id").and_then(|v| v.as_str()),
        title.as_deref(),
    ) {
        if form
            .pointer("/background_tasks/title_generation")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            let _ = app.db.set_chat_title(chat_id, title).await;
            emit(&app, form, json!({"type": "chat:title", "data": title}));
        }
    }
    if form
        .pointer("/background_tasks/tags_generation")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        if let Some(chat_id) = form.get("chat_id").and_then(|v| v.as_str()) {
            if let Some(tags) = maybe_tags(&adapter, &model_id, &full).await {
                let mut saved = Vec::new();
                for tag in tags {
                    if let Ok(list) = app.db.add_tag(&user.id, chat_id, &tag).await {
                        saved = list;
                    }
                }
                emit(&app, form, json!({"type": "chat:tags", "data": saved}));
            }
        }
    }
    Ok(())
}

fn apply_chunk(
    app: &App,
    form: &Value,
    chunk: &ChatChunk,
    full: &mut String,
    reasoning: &mut String,
    open: &mut bool,
) {
    if let Some(text) = &chunk.reasoning {
        if !*open {
            let start =
                "\n<details type=\"reasoning\" done=\"false\">\n<summary>Thinking...</summary>\n";
            full.push_str(start);
            emit(
                app,
                form,
                json!({"type":"chat:completion","data":{"choices":[{"delta":{"content": start}}]}}),
            );
            *open = true;
        }
        reasoning.push_str(text);
        full.push_str(text);
        emit(
            app,
            form,
            json!({"type":"chat:completion","data":{"choices":[{"delta":{"content": text}}]}}),
        );
    }
    if let Some(text) = &chunk.content {
        if *open {
            let end = "\n</details>\n";
            full.push_str(end);
            emit(
                app,
                form,
                json!({"type":"chat:completion","data":{"choices":[{"delta":{"content": end}}]}}),
            );
            *open = false;
        }
        full.push_str(text);
        emit(
            app,
            form,
            json!({"type":"chat:completion","data":{"choices":[{"delta":{"content": text}}]}}),
        );
    }
}

fn emit(app: &App, form: &Value, data: Value) {
    let sid = form
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let payload = json!({
        "chat_id": form.get("chat_id"),
        "message_id": form.get("id"),
        "data": data,
    });
    if let Some(socket) = app.sockets.lock().unwrap().get(sid) {
        let _ = socket.emit("chat-events", &payload);
    }
}

fn emit_status(app: &App, form: &Value, description: &str) {
    emit(
        app,
        form,
        json!({"type": "status", "data": {"description": description, "done": false}}),
    );
}

async fn resolve(
    app: &App,
    requested: &str,
) -> Result<(String, String, Option<String>, Vec<String>), AppError> {
    if let Ok(preset) = app.db.workspace_model(requested).await {
        let base = preset["base_model_id"]
            .as_str()
            .unwrap_or(requested)
            .to_string();
        let system = preset
            .pointer("/params/system")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let tools = preset
            .pointer("/meta/toolIds")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let (provider, model) = split_model(&base)?;
        return Ok((provider, model, system, tools));
    }
    let (provider, model) = split_model(requested)?;
    Ok((provider, model, None, Vec::new()))
}

fn split_model(id: &str) -> Result<(String, String), AppError> {
    let (provider, model) = id.split_once(':').ok_or_else(|| {
        AppError::BadRequest(format!("model id must be provider:model, got {id}"))
    })?;
    Ok((provider.to_string(), model.to_string()))
}

struct BoundTool {
    kind: String,
    config: Value,
    name: String,
    description: String,
    schema: Value,
}

async fn load_tools(app: &App, form: &Value, extra: Vec<String>) -> Vec<BoundTool> {
    let mut ids: Vec<String> = form
        .get("tool_ids")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    ids.extend(extra);
    if ids.is_empty() {
        return Vec::new();
    }
    let servers = app.db.list_tool_servers().await.unwrap_or_default();
    let mut out = Vec::new();
    for server in servers {
        if !server["enabled"].as_bool().unwrap_or(false) {
            continue;
        }
        let sid = server["id"].as_str().unwrap_or("");
        let kind = server["type"].as_str().unwrap_or("mcp_http");
        let config = server["config"].clone();
        let specs = ferrochat_mcp::list_tools(kind, &config)
            .await
            .unwrap_or_default();
        for spec in specs {
            let id = format!("{sid}:{}", spec.name);
            if ids
                .iter()
                .any(|wanted| wanted == &id || wanted == &spec.name)
            {
                out.push(bound(kind, config.clone(), spec));
            }
        }
    }
    out
}

fn bound(kind: &str, config: Value, spec: ToolSpec) -> BoundTool {
    BoundTool {
        kind: kind.to_string(),
        config,
        name: spec.name,
        description: spec.description,
        schema: spec.input_schema,
    }
}

fn openai_tools(tools: &[BoundTool]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": { "name": t.name, "description": t.description, "parameters": t.schema }
            })
        })
        .collect()
}

fn find_tool<'a>(tools: &'a [BoundTool], name: &str) -> Option<(&'a str, &'a Value, &'a str)> {
    tools
        .iter()
        .find(|t| t.name == name)
        .map(|t| (t.kind.as_str(), &t.config, t.name.as_str()))
}

fn substitute(system: &str, user: &CurrentUser) -> String {
    let date = chrono_date();
    system
        .replace("{{CURRENT_DATE}}", &date)
        .replace("{{USER_NAME}}", &user.name)
        .replace("{{USER_EMAIL}}", &user.email)
}

fn chrono_date() -> String {
    let secs = ferrochat_core::now();
    let days = secs.div_euclid(86400);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

async fn maybe_title(
    app: &App,
    adapter: &Box<dyn ferrochat_providers::ChatProvider>,
    model: &str,
    form: &Value,
    content: &str,
) -> Option<String> {
    let _ = app;
    if !form
        .pointer("/background_tasks/title_generation")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return None;
    }
    let prompt = json!({"messages":[
        {"role":"system","content":"Reply with a short chat title, 3 to 6 words, no quotes."},
        {"role":"user","content": content.chars().take(500).collect::<String>()}
    ]});
    let mut stream = adapter.chat_stream(model, &prompt).await.ok()?;
    let mut title = String::new();
    while let Some(item) = stream.next().await {
        if let Ok(chunk) = item {
            if let Some(text) = chunk.content {
                title.push_str(&text);
            }
        }
    }
    let title = title.trim().trim_matches('"').trim().to_string();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

#[derive(Default, Clone)]
struct Accum {
    id: String,
    name: String,
    arguments: String,
}

fn next_key_skip(keys: &str, tick: usize, cooled: &[String]) -> String {
    let parts: Vec<&str> = keys
        .split([',', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return String::new();
    }
    let live: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|key| !cooled.iter().any(|fp| fp == &fingerprint(key)))
        .collect();
    let pool = if live.is_empty() { parts } else { live };
    let chosen = next_key(&pool.join(","), tick);
    chosen
}

fn merge_params(defaults: &Value, request: Option<&Value>) -> Value {
    let mut merged = defaults.as_object().cloned().unwrap_or_default();
    merged.remove("system");
    if let Some(request) = request.and_then(|v| v.as_object()) {
        for (k, v) in request {
            if !v.is_null() {
                merged.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(merged)
}

fn fingerprint(key: &str) -> String {
    let bytes = key.as_bytes();
    let hash = bytes
        .iter()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(*b as u64));
    format!("{hash:x}")
}

fn shared_client() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new).clone()
}

fn fold_context(
    messages: Value,
    files: Option<&Value>,
    vision: bool,
    data_dir: &std::path::Path,
) -> Value {
    let mut messages = messages;
    let mut extra = String::new();
    let mut images = Vec::new();
    if let Some(list) = files.and_then(|v| v.as_array()) {
        for file in list {
            if vision {
                if let Some(url) = file.get("url").and_then(|v| v.as_str()) {
                    let kind = file.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if kind == "image" || url.starts_with("data:image") {
                        images.push(url.to_string());
                    }
                }
            }
            if let Some(text) = file_text(file, data_dir) {
                extra.push_str("\n\n");
                extra.push_str(&text);
            }
        }
    }
    if let Some(arr) = messages.as_array_mut() {
        if !extra.is_empty() {
            if let Some(last) = arr
                .iter_mut()
                .rev()
                .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("user"))
            {
                if let Some(content) = last.get_mut("content") {
                    if let Some(text) = content.as_str() {
                        if images.is_empty() {
                            *content = json!(format!("{text}{extra}"));
                        } else {
                            let mut parts =
                                vec![json!({"type":"text","text": format!("{text}{extra}")})];
                            for url in &images {
                                parts.push(json!({"type":"image_url","image_url":{"url": url}}));
                            }
                            *content = json!(parts);
                        }
                    }
                }
            }
        }
        if !vision {
            for msg in arr.iter_mut() {
                if let Some(parts) = msg.get("content").and_then(|v| v.as_array()) {
                    let text = parts
                        .iter()
                        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("\n");
                    msg["content"] = json!(text);
                }
            }
        }
    }
    messages
}

fn file_text(file: &Value, data_dir: &std::path::Path) -> Option<String> {
    let inline = file
        .pointer("/file/data/content")
        .or_else(|| file.get("content"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let from_disk = file.get("path").and_then(|v| v.as_str()).and_then(|path| {
        let path = std::path::Path::new(path);
        if path.starts_with(data_dir) {
            std::fs::read_to_string(path).ok()
        } else {
            None
        }
    });
    let text = inline.or(from_disk)?;
    let kind = file.get("type").and_then(|v| v.as_str()).unwrap_or("file");
    if kind == "image" {
        return None;
    }
    const LIMIT: usize = 20_000;
    if text.chars().count() > LIMIT {
        Some(format!(
            "{}\n[truncated]",
            text.chars().take(LIMIT).collect::<String>()
        ))
    } else {
        Some(text)
    }
}

async fn maybe_tags(
    adapter: &Box<dyn ferrochat_providers::ChatProvider>,
    model: &str,
    content: &str,
) -> Option<Vec<String>> {
    let prompt = json!({"messages":[
        {"role":"system","content":"Reply with up to 3 short tags separated by commas, no extra text."},
        {"role":"user","content": content.chars().take(500).collect::<String>()}
    ]});
    let mut stream = adapter.chat_stream(model, &prompt).await.ok()?;
    let mut raw = String::new();
    while let Some(item) = stream.next().await {
        if let Ok(chunk) = item {
            if let Some(text) = chunk.content {
                raw.push_str(&text);
            }
        }
    }
    let tags: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty() && s.len() < 40)
        .take(3)
        .collect();
    if tags.is_empty() {
        None
    } else {
        Some(tags)
    }
}
