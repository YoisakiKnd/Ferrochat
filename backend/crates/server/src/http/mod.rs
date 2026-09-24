use crate::auth::{self, CurrentUser};
use crate::App;
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use ferrochat_core::{permissions, AppError, VERSION};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

mod chats;
mod files;
mod providers;

use chats::*;
use files::*;
use providers::*;

pub fn router() -> axum::Router<Arc<App>> {
    use axum::routing::{delete, get, post};
    axum::Router::new()
        .route("/api/config", get(config))
        .route("/api/version", get(version))
        .route("/api/version/updates", get(version_updates))
        .route("/api/changelog", get(changelog))
        .route("/api/models", get(public_models))
        .route("/api/models/base", get(public_models))
        .route("/api/chat/completions", post(chat_completions))
        .route("/api/chat/completed", post(chat_completed))
        .route("/api/tasks/stop/{id}", post(stop_task))
        .route("/api/tasks/chat/{chat_id}", get(chat_tasks))
        .route("/api/webhook", get(webhook).post(webhook_set))
        .route("/api/v1/auths/", get(session))
        .route("/api/v1/auths/signin", post(signin))
        .route("/api/v1/auths/signup", post(signup))
        .route("/api/v1/auths/signout", get(signout))
        .route(
            "/api/v1/auths/admin/config",
            get(admin_config).post(admin_config_set),
        )
        .route("/api/v1/auths/admin/details", get(admin_details))
        .route(
            "/api/v1/auths/admin/config/ldap",
            get(empty_object).post(empty_object),
        )
        .route(
            "/api/v1/auths/admin/config/ldap/server",
            get(empty_object).post(empty_object),
        )
        .route("/api/v1/auths/update/profile", post(update_profile))
        .route("/api/v1/auths/update/password", post(update_password))
        .route("/api/v1/users/user/settings", get(get_settings))
        .route("/api/v1/users/user/settings/update", post(set_settings))
        .route("/api/v1/configs/banners", get(banners))
        .route("/api/v1/channels/", get(empty_list))
        .route("/api/v1/memories/", get(empty_list))
        .route("/api/v1/functions/", get(empty_list))
        .route("/api/v1/knowledge/", get(empty_list))
        .route("/api/v1/knowledge/list", get(empty_list))
        .route(
            "/api/v1/auths/api_key",
            get(empty_api_key).post(empty_api_key).delete(empty_api_key),
        )
        .route("/ollama/api/version", get(ollama_version))
        .route("/api/v1/configs/export", get(export_config))
        .route("/api/v1/chats/", get(list_chats))
        .route("/api/v1/chats/new", post(new_chat))
        .route("/api/v1/chats/search", get(search_chats))
        .route("/api/v1/chats/pinned", get(pinned))
        .route("/api/v1/chats/all/tags", get(all_tags))
        .route("/api/v1/chats/import", post(import_chat))
        .route(
            "/api/v1/chats/{id}",
            get(get_chat).post(update_chat).delete(delete_chat),
        )
        .route("/api/v1/chats/{id}/pin", post(pin_chat))
        .route("/api/v1/chats/{id}/archive", post(archive_chat))
        .route("/api/v1/chats/{id}/clone", post(clone_chat))
        .route("/api/v1/chats/share/{share_id}", get(shared_chat))
        .route(
            "/api/v1/chats/{id}/share",
            post(share_chat).delete(unshare_chat),
        )
        .route("/api/v1/chats/{id}/folder", post(chat_folder))
        .route(
            "/api/v1/chats/{id}/tags",
            get(chat_tags).post(add_chat_tag).delete(noop_tags),
        )
        .route("/api/v1/folders/", get(list_folders).post(create_folder))
        .route(
            "/api/v1/folders/{id}",
            get(get_folder).delete(delete_folder),
        )
        .route("/api/v1/folders/{id}/update", post(rename_folder))
        .route("/api/v1/prompts/", get(list_prompts))
        .route("/api/v1/prompts/list", get(list_prompts))
        .route("/api/v1/groups/", get(empty_list))
        .route("/api/v1/prompts/create", post(create_prompt))
        .route(
            "/api/v1/prompts/command/{command}/update",
            post(update_prompt),
        )
        .route(
            "/api/v1/prompts/command/{command}",
            delete(delete_prompt).get(get_prompt),
        )
        .route(
            "/api/v1/prompts/command/{command}/delete",
            delete(delete_prompt),
        )
        .route("/api/v1/models/", get(list_models))
        .route("/api/v1/models/base", get(base_models))
        .route("/api/v1/models/create", post(create_model))
        .route("/api/v1/models/model", get(get_model))
        .route("/api/v1/models/model/update", post(update_model))
        .route("/api/v1/models/model/toggle", post(toggle_model))
        .route("/api/v1/models/model/delete", delete(delete_model))
        .route("/api/v1/tools/", get(list_tools))
        .route(
            "/api/v1/tool-servers",
            get(list_tool_servers).post(upsert_tool_server),
        )
        .route("/api/v1/tool-servers/{id}", delete(delete_tool_server))
        .route(
            "/api/v1/providers",
            get(list_providers).post(upsert_provider),
        )
        .route(
            "/api/v1/providers/{id}",
            get(get_provider).delete(delete_provider),
        )
        .route("/api/v1/providers/{id}/remote-models", get(remote_models))
        .route("/api/v1/providers/{id}/check", post(check_provider))
        .route(
            "/api/v1/providers/{id}/models",
            get(provider_models).post(add_provider_model),
        )
        .route(
            "/api/v1/providers/{id}/models/batch",
            post(add_provider_models_batch),
        )
        .route("/api/v1/models/managed", get(managed_models))
        .route("/api/v1/models/managed/reorder", post(reorder_models))
        .route("/api/v1/models/managed/default", post(set_default_model))
        .route(
            "/api/v1/providers/{id}/models/{model_id}",
            post(patch_provider_model).delete(delete_provider_model),
        )
        .route("/api/v1/providers/{id}/keys", get(provider_keys))
        .route("/api/v1/providers/export", get(export_providers))
        .route("/api/v1/providers/import", post(import_providers))
        .route("/api/v1/files/", post(upload_file))
        .route("/api/v1/files/{id}/content", get(file_content))
        .route(
            "/api/v1/tasks/config",
            get(task_config).post(task_config_set),
        )
}

pub(crate) fn ok(value: Value) -> Response {
    (StatusCode::OK, Json(value)).into_response()
}
pub(crate) fn fail(err: AppError) -> Response {
    auth::err(err)
}

async fn config(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let authed = session_user(&app, &headers).await.ok();
    let count = app.db.user_count().await.unwrap_or(0);
    let mut body = json!({
        "status": true,
        "name": "Ferrochat",
        "version": VERSION,
        "default_locale": "",
        "oauth": { "providers": {} },
        "features": {
            "auth": true,
            "auth_trusted_header": false,
            "enable_ldap": false,
            "enable_api_key": false,
            "enable_signup": count == 0,
            "enable_login_form": true,
            "enable_websocket": true
        }
    });
    if count == 0 {
        body["onboarding"] = json!(true);
    }
    if authed.is_some() {
        let features = body["features"].as_object_mut().unwrap();
        features.insert("enable_direct_connections".into(), json!(false));
        features.insert("enable_channels".into(), json!(false));
        features.insert("enable_web_search".into(), json!(false));
        features.insert("enable_code_execution".into(), json!(false));
        features.insert("enable_code_interpreter".into(), json!(false));
        features.insert("enable_image_generation".into(), json!(false));
        features.insert("enable_autocomplete_generation".into(), json!(false));
        features.insert("enable_community_sharing".into(), json!(false));
        features.insert("enable_message_rating".into(), json!(false));
        features.insert("enable_user_webhooks".into(), json!(false));
        features.insert("enable_admin_export".into(), json!(true));
        features.insert("enable_admin_chat_access".into(), json!(false));
        features.insert("enable_google_drive_integration".into(), json!(false));
        features.insert("enable_onedrive_integration".into(), json!(false));
        body["default_models"] = app
            .db
            .config_get("default_models")
            .await
            .ok()
            .flatten()
            .unwrap_or(json!(""));
        body["default_prompt_suggestions"] = json!([]);
        body["user_count"] = json!(count);
        body["permissions"] = permissions();
        body["file"] = json!({"max_size": 10_000_000, "max_count": 5});
        body["audio"] = json!({"tts": {"engine": "", "voice": "", "split_on": "punctuation"}, "stt": {"engine": ""}});
    }
    ok(body)
}

async fn session_user(app: &App, headers: &HeaderMap) -> Result<CurrentUser, AppError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .or_else(|| {
            headers
                .get("cookie")
                .and_then(|v| v.to_str().ok())
                .and_then(|c| {
                    c.split(';').find_map(|p| {
                        p.trim()
                            .strip_prefix("ferrochat_token=")
                            .map(|s| s.to_string())
                    })
                })
        })
        .ok_or_else(|| AppError::Unauthorized("not authenticated".into()))?;
    let id = app.keys.user_id(&token)?;
    let value = app.db.user_by_id(&id).await?;
    Ok(CurrentUser {
        id: value["id"].as_str().unwrap_or_default().to_string(),
        email: value["email"].as_str().unwrap_or_default().to_string(),
        name: value["name"].as_str().unwrap_or_default().to_string(),
        role: value["role"].as_str().unwrap_or("user").to_string(),
        profile_image_url: value["profile_image_url"]
            .as_str()
            .unwrap_or("/user.png")
            .to_string(),
    })
}

async fn version() -> Response {
    ok(json!({"version": VERSION}))
}
async fn version_updates(user: CurrentUser) -> Response {
    let _ = user;
    ok(json!({"current": VERSION, "latest": VERSION}))
}

async fn public_models(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    let _ = user;
    match merged_models(&app).await {
        Ok(data) => ok(json!({"data": data})),
        Err(e) => fail(e),
    }
}

async fn merged_models(app: &App) -> Result<Vec<Value>, AppError> {
    let mut models = app.db.enabled_models().await?;
    for preset in app.db.list_workspace_models().await? {
        if !preset["is_active"].as_bool().unwrap_or(true) {
            continue;
        }
        models.push(json!({
            "id": preset["id"],
            "name": preset["name"],
            "object": "model",
            "created": preset["created_at"],
            "owned_by": "preset",
            "preset": true,
            "info": preset,
            "tags": [],
            "provider": {"id": "workspace", "name": "Workspace"}
        }));
    }
    Ok(models)
}

async fn chat_completions(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    match crate::chat::run(app, user, body).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
async fn chat_completed(user: CurrentUser, Json(body): Json<Value>) -> Response {
    let _ = user;
    ok(body)
}
async fn stop_task(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let _ = user;
    if let Some(token) = app.tasks.lock().unwrap().get(&id) {
        token.cancel();
    }
    ok(json!({"status": true}))
}
async fn chat_tasks(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(chat_id): Path<String>,
) -> Response {
    let _ = user;
    let ids = app
        .chat_tasks
        .lock()
        .unwrap()
        .get(&chat_id)
        .cloned()
        .unwrap_or_default();
    ok(json!({"task_ids": ids}))
}

async fn webhook(user: CurrentUser) -> Response {
    let _ = user;
    ok(json!({"url": ""}))
}
async fn webhook_set(user: CurrentUser, Json(body): Json<Value>) -> Response {
    let _ = user;
    ok(body)
}

fn cookie(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        format!("ferrochat_token={token}; Path=/; HttpOnly; SameSite=Lax")
            .parse()
            .unwrap(),
    );
    headers
}

async fn session(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    match auth::session_json(&app.keys, &user) {
        Ok(v) => (cookie(v["token"].as_str().unwrap_or("")), Json(v)).into_response(),
        Err(e) => fail(e),
    }
}

async fn signin(State(app): State<Arc<App>>, Json(body): Json<Value>) -> Response {
    let email = body.get("email").and_then(|v| v.as_str()).unwrap_or("");
    let password = body.get("password").and_then(|v| v.as_str()).unwrap_or("");
    let Some(row) = app.db.user_by_email(email).await.unwrap_or(None) else {
        return fail(AppError::BadRequest("invalid credentials".into()));
    };
    if !auth::verify_password(password, &row.password_hash) {
        return fail(AppError::BadRequest("invalid credentials".into()));
    }
    let user = CurrentUser {
        id: row.id,
        email: row.email,
        name: row.name,
        role: row.role,
        profile_image_url: row.profile_image_url,
    };
    match auth::session_json(&app.keys, &user) {
        Ok(v) => (cookie(v["token"].as_str().unwrap_or("")), Json(v)).into_response(),
        Err(e) => fail(e),
    }
}

async fn signup(State(app): State<Arc<App>>, Json(body): Json<Value>) -> Response {
    if app.db.user_count().await.unwrap_or(1) > 0 {
        return fail(AppError::BadRequest("signup is closed".into()));
    }
    let email = body.get("email").and_then(|v| v.as_str()).unwrap_or("");
    let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("Admin");
    let password = body.get("password").and_then(|v| v.as_str()).unwrap_or("");
    if email.is_empty() || password.len() < 6 {
        return fail(AppError::BadRequest(
            "email and a password of at least 6 characters are required".into(),
        ));
    }
    let hash = match auth::hash_password(password) {
        Ok(h) => h,
        Err(e) => return fail(e),
    };
    let created = match app.db.insert_user(email, name, &hash, "admin").await {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    let user = CurrentUser {
        id: created["id"].as_str().unwrap_or_default().to_string(),
        email: created["email"].as_str().unwrap_or_default().to_string(),
        name: created["name"].as_str().unwrap_or_default().to_string(),
        role: "admin".into(),
        profile_image_url: created["profile_image_url"]
            .as_str()
            .unwrap_or("/user.png")
            .to_string(),
    };
    match auth::session_json(&app.keys, &user) {
        Ok(v) => (cookie(v["token"].as_str().unwrap_or("")), Json(v)).into_response(),
        Err(e) => fail(e),
    }
}

async fn signout() -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        "ferrochat_token=; Path=/; Max-Age=0".parse().unwrap(),
    );
    (headers, Json(json!({"status": true}))).into_response()
}

async fn admin_config(user: CurrentUser) -> Response {
    let _ = user;
    ok(json!({"SHOW_ADMIN_DETAILS": true, "ENABLE_SIGNUP": false, "DEFAULT_USER_ROLE": "admin"}))
}
async fn admin_config_set(user: CurrentUser, Json(body): Json<Value>) -> Response {
    let _ = user;
    ok(body)
}
async fn empty_list(user: CurrentUser) -> Response {
    let _ = user;
    ok(json!([]))
}

async fn ollama_version() -> Response {
    ok(json!({"version": "0.0.0"}))
}

async fn changelog() -> Response {
    ok(json!({}))
}

async fn empty_api_key(user: CurrentUser) -> Response {
    let _ = user;
    ok(json!({"api_key": null}))
}

async fn empty_object(user: CurrentUser) -> Response {
    let _ = user;
    ok(json!({}))
}

async fn admin_details(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    let _ = user;
    let count = app.db.user_count().await.unwrap_or(0);
    ok(json!({"name": "Ferrochat", "version": VERSION, "user_count": count}))
}

async fn update_profile(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&user.name);
    let image = body
        .get("profile_image_url")
        .and_then(|v| v.as_str())
        .unwrap_or(&user.profile_image_url);
    match app.db.update_profile(&user.id, name, image).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
async fn update_password(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    let password = body.get("password").and_then(|v| v.as_str()).unwrap_or("");
    let current = body
        .get("current_password")
        .or_else(|| body.get("password"))
        .and_then(|v| v.as_str())
        .unwrap_or(password);
    let hash = match app.db.password_hash(&user.id).await {
        Ok(h) => h,
        Err(e) => return fail(e),
    };
    if body.get("current_password").is_some() && !auth::verify_password(current, &hash) {
        return fail(AppError::BadRequest("current password is wrong".into()));
    }
    let new_hash = match auth::hash_password(password) {
        Ok(h) => h,
        Err(e) => return fail(e),
    };
    match app.db.update_password(&user.id, &new_hash).await {
        Ok(()) => ok(json!(true)),
        Err(e) => fail(e),
    }
}

fn unwrap_settings(mut value: Value) -> Value {
    while value.get("ui").and_then(|ui| ui.get("ui")).is_some() {
        value = value.get("ui").cloned().unwrap_or(json!({}));
    }
    if value.get("ui").is_some() {
        value
    } else {
        json!({"ui": value})
    }
}

async fn get_settings(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    match app.db.get_settings(&user.id).await {
        Ok(v) => ok(unwrap_settings(v)),
        Err(e) => fail(e),
    }
}
async fn set_settings(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    let stored = if body.get("ui").is_some() {
        body
    } else {
        json!({"ui": body})
    };
    match app.db.set_settings(&user.id, &stored).await {
        Ok(_) => ok(stored),
        Err(e) => fail(e),
    }
}
async fn banners(user: CurrentUser) -> Response {
    let _ = user;
    ok(json!([]))
}
async fn export_config(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    let _ = user;
    match app.db.list_providers().await {
        Ok(p) => ok(json!({"providers": p})),
        Err(e) => fail(e),
    }
}

async fn list_models(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    let _ = user;
    match app.db.list_workspace_models().await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
async fn base_models(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    public_models(State(app), user).await
}
async fn create_model(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    match app.db.upsert_workspace_model(&user.id, &body).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
#[derive(Deserialize)]
struct IdQuery {
    id: String,
}
async fn get_model(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Query(q): Query<IdQuery>,
) -> Response {
    let _ = user;
    match app.db.workspace_model(&q.id).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
async fn update_model(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Query(q): Query<IdQuery>,
    Json(mut body): Json<Value>,
) -> Response {
    body["id"] = json!(q.id);
    create_model(State(app), user, Json(body)).await
}
async fn toggle_model(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Query(q): Query<IdQuery>,
) -> Response {
    let _ = user;
    match app.db.toggle_workspace_model(&q.id).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
async fn delete_model(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Query(q): Query<IdQuery>,
) -> Response {
    let _ = user;
    match app.db.delete_workspace_model(&q.id).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}

async fn list_tools(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    let _ = user;
    let servers = app.db.list_tool_servers().await.unwrap_or_default();
    let mut tools = Vec::new();
    for server in servers {
        if !server["enabled"].as_bool().unwrap_or(false) {
            continue;
        }
        let kind = server["type"].as_str().unwrap_or("mcp_http");
        let specs = ferrochat_mcp::list_tools(kind, &server["config"])
            .await
            .unwrap_or_default();
        for spec in specs {
            tools.push(json!({
                "id": format!("{}:{}", server["id"].as_str().unwrap_or(""), spec.name),
                "user_id": server["user_id"],
                "name": spec.name,
                "content": "",
                "specs": [{"name": spec.name, "description": spec.description, "parameters": spec.input_schema}],
                "meta": {"description": spec.description, "manifest": {}},
                "updated_at": server["updated_at"],
                "created_at": server["created_at"]
            }));
        }
    }
    ok(json!(tools))
}
async fn list_tool_servers(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    let _ = user;
    match app.db.list_tool_servers().await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
async fn upsert_tool_server(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    match app.db.upsert_tool_server(&user.id, &body).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
async fn delete_tool_server(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let _ = user;
    match app.db.delete_tool_server(&id).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}

async fn task_config(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    let _ = user;
    match app.db.config_get("tasks").await {
        Ok(v) => ok(v.unwrap_or(json!({"TASK_MODEL": "", "TITLE_GENERATION_PROMPT_TEMPLATE": ""}))),
        Err(e) => fail(e),
    }
}
async fn task_config_set(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    let _ = user;
    if let Err(e) = app.db.config_set("tasks", &body).await {
        return fail(e);
    }
    ok(body)
}
