use crate::auth::CurrentUser;
use crate::App;
use ferrochat_core::AppError;
use ferrochat_mcp::{self, ToolSpec};
use ferrochat_providers::{build, next_key, ChatChunk, Conn};
use ferrochat_search::{self, Hit, Query};
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

/// Opening tag the backend injects when a model streams reasoning. The
/// `done="false"` marker is rewritten to `done="true" duration="N"` once the
/// thinking block closes; the frontend `Collapsible` keys its spinner off it.
const REASONING_OPEN: &str =
    "\n<details type=\"reasoning\" done=\"false\">\n<summary>Thinking...</summary>\n";
const DONE_FALSE: &str = "done=\"false\"";

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
    attach_document_excerpts(&app, form, &mut messages).await;
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
    let mut native_search = false;
    if form
        .pointer("/features/web_search")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let (native, sources) =
            prepare_web_search(&app, form, &mut messages, &adapter, &model_id, &provider_id).await;
        native_search = native;
        for source in sources {
            emit(
                &app,
                form,
                json!({
                    "type": "source",
                    "data": {
                        "source": {"name": source["name"], "url": source["url"]},
                        "document": [source["snippet"]],
                        "metadata": [{"source": source["url"], "name": source["name"]}]
                    }
                }),
            );
        }
    }
    let mut tools = load_tools(&app, form, extra_tools).await;
    let mut tool_schemas = openai_tools(&tools);
    let mut full = String::new();
    let mut reasoning = String::new();
    let mut reasoning_span: Option<(usize, Instant)> = None;
    let mut reported_prompt: Option<i64> = None;
    let mut reported_completion: Option<i64> = None;

    if let Some((summary, covered)) =
        compress_messages(&adapter, &model_id, form, &mut messages).await
    {
        emit(
            &app,
            form,
            json!({"type": "chat:summary", "data": {"summary": summary, "summary_count": covered}}),
        );
    }
    let prompt_tokens = estimate_tokens(&messages);
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
        body["provider_id"] = json!(provider_id);
        if native_search {
            body["native_search"] = json!(true);
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
            if chunk.prompt_tokens.is_some() {
                reported_prompt = chunk.prompt_tokens;
            }
            if chunk.completion_tokens.is_some() {
                reported_completion = chunk.completion_tokens;
            }
            apply_chunk(
                &app,
                form,
                &chunk,
                &mut full,
                &mut reasoning,
                &mut reasoning_span,
            );
            for (title, url) in &chunk.citations {
                emit(
                    &app,
                    form,
                    json!({
                        "type": "source",
                        "data": {
                            "source": {"name": title, "url": url},
                            "document": [title],
                            "metadata": [{"source": url, "name": title}]
                        }
                    }),
                );
            }
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
        finalize_reasoning(&mut full, &mut reasoning_span);
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
    let estimated = reported_prompt.is_none() || reported_completion.is_none();
    let prompt_tokens = reported_prompt.unwrap_or(prompt_tokens);
    let completion_tokens = reported_completion.unwrap_or_else(|| {
        (full.chars().count() as i64 / 4).max(if full.is_empty() { 0 } else { 1 })
    });
    let stored = app
        .db
        .model_params(&provider_id, &model_id)
        .await
        .unwrap_or(json!({}));
    let input_price = stored["input_price"]
        .as_f64()
        .unwrap_or_else(|| price_of(form, "input_price"));
    let output_price = stored["output_price"]
        .as_f64()
        .unwrap_or_else(|| price_of(form, "output_price"));
    let cost = (prompt_tokens as f64 * input_price + completion_tokens as f64 * output_price)
        / 1_000_000.0;
    let usage = json!({
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "total_tokens": prompt_tokens + completion_tokens,
        "cost": cost,
        "estimated": estimated
    });
    let _ = app
        .db
        .record_usage(&user.id, &requested, prompt_tokens, completion_tokens, cost)
        .await;
    emit(
        &app,
        form,
        json!({"type": "chat:completion", "data": {"done": true, "content": full, "title": title, "usage": usage}}),
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
    if form
        .get("follow_up")
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
    {
        if let Some(questions) = follow_ups(&adapter, &model_id, &full).await {
            emit(
                &app,
                form,
                json!({"type": "chat:follow_ups", "data": questions}),
            );
        }
    }
    if form
        .get("memory")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
        && form
            .get("memory_suggest")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    {
        if let Some(fact) = suggest_memory(&adapter, &model_id, &full).await {
            emit(
                &app,
                form,
                json!({"type": "chat:memory_suggestion", "data": fact}),
            );
        }
    }
    Ok(())
}

/// Close the open reasoning block, if any: rewrite its `done="false"` marker
/// to `done="true" duration="N"` in `full` and append the closing tag. Returns
/// the closing delta so the caller can stream it. Byte offsets recorded at open
/// time stay valid because everything after them is appended, never inserted.
fn finalize_reasoning(full: &mut String, span: &mut Option<(usize, Instant)>) -> Option<String> {
    let (done_at, started) = span.take()?;
    let secs = started.elapsed().as_secs();
    if full.get(done_at..done_at + DONE_FALSE.len()) == Some(DONE_FALSE) {
        full.replace_range(
            done_at..done_at + DONE_FALSE.len(),
            &format!("done=\"true\" duration=\"{secs}\""),
        );
    }
    let end = "\n</details>\n";
    full.push_str(end);
    Some(end.to_string())
}

fn apply_chunk(
    app: &App,
    form: &Value,
    chunk: &ChatChunk,
    full: &mut String,
    reasoning: &mut String,
    span: &mut Option<(usize, Instant)>,
) {
    if let Some(text) = &chunk.reasoning {
        if span.is_none() {
            let done_at = full.len() + REASONING_OPEN.find(DONE_FALSE).unwrap_or(0);
            full.push_str(REASONING_OPEN);
            emit(
                app,
                form,
                json!({"type":"chat:completion","data":{"choices":[{"delta":{"content": REASONING_OPEN}}]}}),
            );
            *span = Some((done_at, Instant::now()));
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
        if let Some(end) = finalize_reasoning(full, span) {
            emit(
                app,
                form,
                json!({"type":"chat:completion","data":{"choices":[{"delta":{"content": end}}]}}),
            );
        }
        full.push_str(text);
        emit(
            app,
            form,
            json!({"type":"chat:completion","data":{"choices":[{"delta":{"content": text}}]}}),
        );
    }
}

pub async fn stream_direct(
    app: Arc<App>,
    user: CurrentUser,
    form: Value,
    tx: tokio::sync::mpsc::Sender<String>,
) -> Result<(), AppError> {
    let _ = user;
    let requested = form
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let (provider_id, model_id, _system, _tools) = resolve(&app, &requested).await?;
    let provider = app.db.provider(&provider_id).await?;
    let tick = app
        .key_tick
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let cooled = app
        .db
        .cooled_fingerprints(&provider_id)
        .await
        .unwrap_or_default();
    let api_key = next_key_skip(provider["api_keys"].as_str().unwrap_or(""), tick, &cooled);
    let conn = Conn {
        kind: provider["type"].as_str().unwrap_or("openai").to_string(),
        base_url: provider["base_url"].as_str().unwrap_or("").to_string(),
        api_key,
        headers: provider["headers"].clone(),
        client: shared_client(),
    };
    let adapter = build(conn);
    let mut body = form.clone();
    body["provider_id"] = json!(provider_id);
    if form
        .pointer("/features/web_search")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        let mut messages = body.get("messages").cloned().unwrap_or(json!([]));
        let (_native, sources) = prepare_web_search(
            &app,
            &form,
            &mut messages,
            &adapter,
            &model_id,
            &provider_id,
        )
        .await;
        body["messages"] = messages;
        if !sources.is_empty() {
            let _ = tx
                .send(format!("data: {}\n\n", json!({"sources": sources})))
                .await;
        }
    }
    let mut stream = adapter.chat_stream(&model_id, &body).await?;
    while let Some(item) = stream.next().await {
        let chunk = item?;
        if let Some(text) = chunk.content {
            let data = json!({"choices":[{"delta":{"content": text}}]});
            if tx.send(format!("data: {data}\n\n")).await.is_err() {
                break;
            }
        }
    }
    let _ = tx.send("data: [DONE]\n\n".to_string()).await;
    Ok(())
}

async fn suggest_memory(
    adapter: &Box<dyn ferrochat_providers::ChatProvider>,
    model: &str,
    content: &str,
) -> Option<String> {
    let prompt = json!({"messages":[
        {"role":"system","content":"If the conversation contains one durable fact about the user, reply with that fact in one sentence. Otherwise reply NONE."},
        {"role":"user","content": content.chars().take(1200).collect::<String>()}
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
    let raw = raw.trim().to_string();
    if raw.is_empty() || raw.eq_ignore_ascii_case("NONE") || raw.len() > 240 {
        None
    } else {
        Some(raw)
    }
}

async fn follow_ups(
    adapter: &Box<dyn ferrochat_providers::ChatProvider>,
    model: &str,
    content: &str,
) -> Option<Vec<String>> {
    let prompt = json!({"messages":[
        {"role":"system","content":"Reply with exactly 3 short follow-up questions the user might ask next. One question per line. No numbering."},
        {"role":"user","content": content.chars().take(1200).collect::<String>()}
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
    let questions: Vec<String> = raw
        .lines()
        .map(|line| {
            line.trim()
                .trim_start_matches(|c: char| {
                    c.is_ascii_digit() || c == '.' || c == '-' || c == ' '
                })
                .to_string()
        })
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty() && line.len() < 160)
        .take(3)
        .collect();
    if questions.is_empty() {
        None
    } else {
        Some(questions)
    }
}

fn price_of(form: &Value, key: &str) -> f64 {
    form.pointer(&format!("/model_item/info/params/{key}"))
        .or_else(|| form.pointer(&format!("/model_item/params/{key}")))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

async fn compress_messages(
    adapter: &Box<dyn ferrochat_providers::ChatProvider>,
    model_id: &str,
    form: &Value,
    messages: &mut Value,
) -> Option<(String, i64)> {
    let limit = form
        .get("recent_messages")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if limit <= 0 {
        return None;
    }
    let Some(arr) = messages.as_array_mut() else {
        return None;
    };
    let system: Vec<Value> = arr
        .iter()
        .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("system"))
        .cloned()
        .collect();
    let mut rest: Vec<Value> = arr
        .iter()
        .filter(|m| m.get("role").and_then(|v| v.as_str()) != Some("system"))
        .cloned()
        .collect();
    let covered = form
        .get("summary_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
        .max(0) as usize;
    let prior = form
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let dropped_prior = if covered > 0 && covered < rest.len() {
        rest.drain(..covered);
        true
    } else {
        false
    };
    if rest.len() as i64 <= limit {
        if !prior.is_empty() || dropped_prior {
            arr.clear();
            arr.extend(system);
            if !prior.is_empty() {
                arr.push(json!({"role": "system", "content": format!("Earlier conversation summary:\n{prior}")}));
            }
            arr.extend(rest);
        }
        return None;
    }
    let drop_count = rest.len() - limit as usize;
    let dropped: Vec<Value> = rest.drain(..drop_count).collect();
    let auto = form
        .get("auto_summary")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let fresh = if auto {
        summarize_dropped(adapter, model_id, &dropped).await
    } else {
        None
    };
    let fresh = fresh.unwrap_or_else(|| snippet_summary(&dropped));
    let summary = if prior.is_empty() {
        fresh
    } else {
        format!("{prior}\n{fresh}")
    };
    arr.clear();
    arr.extend(system);
    arr.push(
        json!({"role": "system", "content": format!("Earlier conversation summary:\n{summary}")}),
    );
    arr.extend(rest);
    Some((summary, (covered + drop_count) as i64))
}

async fn summarize_dropped(
    adapter: &Box<dyn ferrochat_providers::ChatProvider>,
    model_id: &str,
    dropped: &[Value],
) -> Option<String> {
    let text = snippet_summary(dropped);
    let prompt = json!({"messages":[
        {"role":"system","content":"Summarize this conversation in one short paragraph. Keep facts, names, and decisions."},
        {"role":"user","content": text}
    ]});
    let mut stream = adapter.chat_stream(model_id, &prompt).await.ok()?;
    let mut raw = String::new();
    while let Some(item) = stream.next().await {
        let chunk = item.ok()?;
        if let Some(part) = chunk.content {
            raw.push_str(&part);
        }
    }
    let raw = raw.trim().to_string();
    if raw.is_empty() {
        None
    } else {
        Some(raw)
    }
}

fn snippet_summary(dropped: &[Value]) -> String {
    let mut note = String::new();
    for message in dropped.iter().take(12) {
        let role = message.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let content = message
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let snippet: String = content.chars().take(180).collect();
        note.push_str(&format!("{role}: {snippet}\n"));
    }
    note
}

#[allow(dead_code)]
fn trim_messages(messages: &mut Value, limit: i64) {
    if limit <= 0 {
        return;
    }
    let Some(arr) = messages.as_array_mut() else {
        return;
    };
    let system: Vec<Value> = arr
        .iter()
        .filter(|m| m.get("role").and_then(|v| v.as_str()) == Some("system"))
        .cloned()
        .collect();
    let rest: Vec<Value> = arr
        .iter()
        .filter(|m| m.get("role").and_then(|v| v.as_str()) != Some("system"))
        .cloned()
        .collect();
    if rest.len() as i64 <= limit {
        return;
    }
    let drop_count = rest.len() - limit as usize;
    let mut note = String::from("Earlier conversation:\n");
    for message in rest.iter().take(drop_count).take(12) {
        let role = message.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let content = message
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let snippet: String = content.chars().take(180).collect();
        note.push_str(&format!("{role}: {snippet}\n"));
    }
    arr.clear();
    arr.extend(system);
    arr.push(json!({"role": "system", "content": note}));
    arr.extend(rest.into_iter().skip(drop_count));
}

fn estimate_tokens(messages: &Value) -> i64 {
    let text = messages.to_string();
    (text.chars().count() as i64 / 4).max(1)
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

async fn prepare_web_search(
    app: &App,
    form: &Value,
    messages: &mut Value,
    adapter: &Box<dyn ferrochat_providers::ChatProvider>,
    model_id: &str,
    provider_id: &str,
) -> (bool, Vec<Value>) {
    let cfg = app
        .db
        .config_get("web_search")
        .await
        .ok()
        .flatten()
        .unwrap_or(json!({}));
    if !ferrochat_search::configured(&cfg) && !cfg["prefer_native"].as_bool().unwrap_or(false) {
        emit(
            app,
            form,
            json!({"type":"status","data":{"action":"web_search","description":"Web search is not configured","done":true}}),
        );
        return (false, Vec::new());
    }
    let user_text = last_user_text(messages);
    let model_web = app
        .db
        .provider_model_web(provider_id, model_id)
        .await
        .unwrap_or(false);
    let prefer_native =
        cfg["prefer_native"].as_bool().unwrap_or(false) || cfg["engine"].as_str() == Some("native");
    if prefer_native && model_web {
        emit(
            app,
            form,
            json!({"type":"status","data":{"action":"web_search","description":"Searching with the model","done":true}}),
        );
        return (true, Vec::new());
    }
    if !ferrochat_search::configured(&cfg) || cfg["engine"].as_str() == Some("native") {
        emit(
            app,
            form,
            json!({"type":"status","data":{"action":"web_search","description":"This model has no built-in search and no external engine is configured","done":true}}),
        );
        return (false, Vec::new());
    }
    emit(
        app,
        form,
        json!({"type":"status","data":{"action":"web_search","description":"Searching the web","done":false}}),
    );
    let query_text = search_query(adapter, model_id, &user_text).await;
    let count = cfg.get("count").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let query = Query {
        engine: cfg
            .get("engine")
            .and_then(|v| v.as_str())
            .unwrap_or("searxng")
            .to_string(),
        url: cfg
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        api_key: cfg
            .get("api_key")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        cx: cfg
            .get("cx")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        count,
        text: query_text,
    };
    let mut hits = ferrochat_search::search(&query).await.unwrap_or_default();
    for url in urls_in(&user_text).into_iter().take(3) {
        if hits.iter().any(|hit| hit.url == url) {
            continue;
        }
        hits.insert(
            0,
            Hit {
                title: url.clone(),
                url,
                snippet: String::new(),
            },
        );
    }
    let fetch_pages = cfg
        .get("fetch_content")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if fetch_pages {
        for hit in hits.iter_mut().take(count) {
            if let Some(text) = ferrochat_search::fetch_text(&hit.url).await {
                hit.snippet = text;
            }
        }
    }
    let urls: Vec<&str> = hits.iter().map(|hit| hit.url.as_str()).collect();
    emit(
        app,
        form,
        json!({"type":"status","data":{"action":"web_search","description":"Searched {{count}} sites","done":true,"urls": urls}}),
    );
    let sources: Vec<Value> = hits
        .iter()
        .enumerate()
        .map(|(idx, hit)| {
            json!({"name": hit.title, "url": hit.url, "snippet": hit.snippet, "index": idx + 1})
        })
        .collect();
    if !hits.is_empty() {
        let mut block =
            String::from("Use the numbered web results below. Cite them as [1], [2].\n");
        for (idx, hit) in hits.iter().take(count).enumerate() {
            block.push_str(&format!(
                "[{}] {} ({})\n{}\n",
                idx + 1,
                hit.title,
                hit.url,
                hit.snippet.chars().take(1200).collect::<String>()
            ));
        }
        if let Some(arr) = messages.as_array_mut() {
            arr.insert(0, json!({"role": "system", "content": block}));
        }
    }
    (false, sources)
}

async fn search_query(
    adapter: &Box<dyn ferrochat_providers::ChatProvider>,
    model_id: &str,
    user_text: &str,
) -> String {
    let prompt = json!({
        "messages": [{
            "role": "user",
            "content": format!("Turn this into one short web search query. Output only the query.\n\n{user_text}")
        }]
    });
    let Ok(mut stream) = adapter.chat_stream(model_id, &prompt).await else {
        return user_text.chars().take(200).collect();
    };
    let mut query = String::new();
    while let Some(item) = stream.next().await {
        let Ok(chunk) = item else { break };
        if let Some(text) = chunk.content {
            query.push_str(&text);
        }
        if chunk.done || query.len() > 180 {
            break;
        }
    }
    let query = query.trim().trim_matches('"').to_string();
    if query.is_empty() {
        user_text.chars().take(200).collect()
    } else {
        query
    }
}

fn last_user_text(messages: &Value) -> String {
    messages
        .as_array()
        .and_then(|items| {
            items.iter().rev().find_map(|msg| {
                if msg.get("role").and_then(|v| v.as_str()) == Some("user") {
                    msg.get("content")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
        })
        .unwrap_or_default()
}

fn urls_in(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter_map(|word| {
            let word = word.trim_matches(|c: char| "()[]<>.,".contains(c));
            if word.starts_with("http://") || word.starts_with("https://") {
                Some(word.to_string())
            } else {
                None
            }
        })
        .collect()
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
            if file_id(file).is_some() {
                continue;
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

fn file_id(file: &Value) -> Option<&str> {
    file.get("id")
        .or_else(|| file.pointer("/file/id"))
        .and_then(|v| v.as_str())
        .filter(|id| !id.is_empty())
}

async fn attach_document_excerpts(app: &App, form: &Value, messages: &mut Value) {
    let Some(list) = form.get("files").and_then(|v| v.as_array()) else {
        return;
    };
    let ids: Vec<String> = list
        .iter()
        .filter_map(|file| file_id(file).map(str::to_string))
        .collect();
    if ids.is_empty() {
        return;
    }
    let query = messages
        .as_array()
        .and_then(|arr| {
            arr.iter().rev().find_map(|m| {
                if m.get("role").and_then(|v| v.as_str()) == Some("user") {
                    m.get("content").and_then(|v| v.as_str())
                } else {
                    None
                }
            })
        })
        .unwrap_or("");
    let hits = app
        .db
        .search_passages(&ids, query, 6)
        .await
        .unwrap_or_default();
    if hits.is_empty() {
        return;
    }
    let mut block = String::from("\n\nDocument excerpts:\n");
    for (idx, (file_id, filename, page, body)) in hits.iter().enumerate() {
        let place = if *page > 0 {
            format!("{filename} p.{page}")
        } else {
            filename.clone()
        };
        block.push_str(&format!("[{}] {place}\n{body}\n\n", idx + 1));
        let url = format!("/api/v1/files/{file_id}/content");
        emit(
            app,
            form,
            json!({
                "type": "source",
                "data": {
                    "source": {"name": place, "url": url},
                    "document": [body],
                    "metadata": [{"source": url, "name": place, "page": page}]
                }
            }),
        );
    }
    if let Some(arr) = messages.as_array_mut() {
        if let Some(last) = arr
            .iter_mut()
            .rev()
            .find(|m| m.get("role").and_then(|v| v.as_str()) == Some("user"))
        {
            if let Some(content) = last.get_mut("content").and_then(|v| v.as_str()) {
                last["content"] = json!(format!("{content}{block}"));
            }
        }
    }
}

fn file_text(file: &Value, data_dir: &std::path::Path) -> Option<String> {
    let inline = file
        .pointer("/file/data/content")
        .or_else(|| file.pointer("/file/meta/content"))
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

#[cfg(test)]
mod tests {
    use super::{compress_messages, finalize_reasoning, trim_messages, DONE_FALSE, REASONING_OPEN};
    use ferrochat_providers::{build, Conn};
    use serde_json::json;
    use std::time::Instant;

    #[test]
    fn closed_reasoning_block_is_marked_done_with_duration() {
        let mut full = String::new();
        let done_at = full.len() + REASONING_OPEN.find(DONE_FALSE).unwrap();
        full.push_str(REASONING_OPEN);
        full.push_str("先想一下中文问候");
        let mut span = Some((done_at, Instant::now()));
        assert_eq!(
            finalize_reasoning(&mut full, &mut span).as_deref(),
            Some("\n</details>\n")
        );
        assert!(span.is_none());
        assert!(!full.contains(DONE_FALSE));
        assert!(full.contains("done=\"true\" duration=\""));
        assert!(full.ends_with("先想一下中文问候\n</details>\n"));
    }

    #[test]
    fn closing_without_an_open_block_changes_nothing() {
        let mut full = "plain answer".to_string();
        let mut span = None;
        assert!(finalize_reasoning(&mut full, &mut span).is_none());
        assert_eq!(full, "plain answer");
    }

    #[test]
    fn sequential_reasoning_blocks_each_keep_their_own_marker() {
        let mut full = String::new();
        let mut span = None;
        for round in 0..2 {
            let done_at = full.len() + REASONING_OPEN.find(DONE_FALSE).unwrap();
            full.push_str(REASONING_OPEN);
            full.push_str(&format!("r{round}"));
            span = Some((done_at, Instant::now()));
            finalize_reasoning(&mut full, &mut span);
            full.push_str(&format!("answer{round}"));
        }
        assert!(!full.contains(DONE_FALSE));
        assert_eq!(full.matches("done=\"true\"").count(), 2);
        assert_eq!(full.matches("</details>").count(), 2);
    }

    #[test]
    fn keeps_recent_messages_and_summarizes_the_rest() {
        let mut messages = json!([
            {"role":"system","content":"Be brief."},
            {"role":"user","content":"one"},
            {"role":"assistant","content":"two"},
            {"role":"user","content":"three"},
            {"role":"assistant","content":"four"}
        ]);
        trim_messages(&mut messages, 2);
        let roles: Vec<&str> = messages
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["system", "system", "user", "assistant"]);
        assert!(messages[1]["content"]
            .as_str()
            .unwrap()
            .contains("Earlier conversation"));
        assert!(messages[1]["content"].as_str().unwrap().contains("one"));
        assert_eq!(messages[2]["content"], "three");
        assert_eq!(messages[3]["content"], "four");
    }

    #[tokio::test]
    async fn reuses_a_stored_summary_for_messages_already_covered() {
        let adapter = build(Conn {
            kind: "mock".into(),
            base_url: String::new(),
            api_key: String::new(),
            headers: json!({}),
            client: reqwest::Client::new(),
        });
        let form = json!({
            "recent_messages": 2,
            "auto_summary": true,
            "summary": "old facts",
            "summary_count": 2
        });
        let mut messages = json!([
            {"role":"user","content":"one"},
            {"role":"assistant","content":"two"},
            {"role":"user","content":"three"},
            {"role":"assistant","content":"four"},
            {"role":"user","content":"five"}
        ]);
        let (summary, count) = compress_messages(&adapter, "mock-model", &form, &mut messages)
            .await
            .expect("overflow still needs a summary");
        assert!(summary.starts_with("old facts"));
        assert_eq!(count, 3);
        let text = messages.to_string();
        assert!(text.contains("Earlier conversation summary"));
        assert!(text.contains("five"));
    }
}
