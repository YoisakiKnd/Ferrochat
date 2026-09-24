use super::{fail, ok};
use crate::auth::CurrentUser;
use crate::App;
use axum::{
    extract::{Path, Query, State},
    response::Response,
    Json,
};
use ferrochat_core::infer_capabilities;
use ferrochat_providers::{build, next_key, Conn};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub(crate) async fn list_providers(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    let _ = user;
    match app.db.list_providers().await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn upsert_provider(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    let _ = user;
    match app.db.upsert_provider(&body).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn get_provider(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let _ = user;
    match app.db.provider(&id).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn delete_provider(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let _ = user;
    match app.db.delete_provider(&id).await {
        Ok(()) => ok(json!(true)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn provider_models(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let _ = user;
    match app.db.list_provider_models(&id).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn add_provider_model(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let _ = user;
    let model_id = body.get("model_id").and_then(|v| v.as_str()).unwrap_or("");
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(model_id);
    let caps = body
        .get("capabilities")
        .cloned()
        .or_else(|| serde_json::to_value(infer_capabilities(model_id)).ok());
    match app.db.add_provider_model(&id, model_id, name, caps).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn add_provider_models_batch(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let _ = user;
    let list = body
        .get("models")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut added = Vec::new();
    for item in list {
        let model_id = item.get("model_id").and_then(|v| v.as_str()).unwrap_or("");
        if model_id.is_empty() {
            continue;
        }
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(model_id);
        let caps = item
            .get("capabilities")
            .cloned()
            .or_else(|| serde_json::to_value(infer_capabilities(model_id)).ok());
        match app.db.add_provider_model(&id, model_id, name, caps).await {
            Ok(v) => added.push(v),
            Err(e) => return fail(e),
        }
    }
    ok(json!(added))
}
pub(crate) async fn managed_models(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    let _ = user;
    let default_model = app
        .db
        .config_get("default_models")
        .await
        .ok()
        .flatten()
        .unwrap_or(json!(""));
    match app.db.managed_models().await {
        Ok(v) => ok(json!({ "models": v, "default_model": default_model })),
        Err(e) => fail(e),
    }
}
pub(crate) async fn reorder_models(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    let _ = user;
    let ids: Vec<String> = body
        .get("ids")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    match app.db.reorder_models(&ids).await {
        Ok(()) => ok(json!(true)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn set_default_model(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    let _ = user;
    let value = body.get("default_model").cloned().unwrap_or(json!(""));
    match app.db.config_set("default_models", &value).await {
        Ok(()) => ok(json!({ "default_model": value })),
        Err(e) => fail(e),
    }
}
pub(crate) async fn patch_provider_model(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path((id, model_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let _ = user;
    let key = if model_id.contains(':') {
        model_id
    } else {
        format!("{id}:{model_id}")
    };
    match app.db.update_provider_model(&key, &body).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn provider_keys(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let _ = user;
    match app.db.key_health(&id).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn export_providers(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Query(q): Query<ExportQuery>,
) -> Response {
    let _ = user;
    match app.db.list_providers().await {
        Ok(mut providers) => {
            if !q.include_keys.unwrap_or(false) {
                for provider in &mut providers {
                    if let Some(obj) = provider.as_object_mut() {
                        obj.insert("api_keys".into(), json!(""));
                    }
                }
            }
            ok(json!({ "providers": providers }))
        }
        Err(e) => fail(e),
    }
}
#[derive(Deserialize)]
pub(crate) struct ExportQuery {
    include_keys: Option<bool>,
}
pub(crate) async fn import_providers(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    let _ = user;
    let list = body
        .get("providers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut saved = Vec::new();
    for provider in list {
        match app.db.upsert_provider(&provider).await {
            Ok(v) => saved.push(v),
            Err(e) => return fail(e),
        }
    }
    ok(json!(saved))
}
pub(crate) async fn delete_provider_model(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path((id, model_id)): Path<(String, String)>,
) -> Response {
    let _ = user;
    let key = if model_id.contains(':') {
        model_id
    } else {
        format!("{id}:{model_id}")
    };
    match app.db.delete_provider_model(&key).await {
        Ok(()) => ok(json!(true)),
        Err(e) => fail(e),
    }
}

pub(crate) fn adapter_for(provider: &Value) -> Box<dyn ferrochat_providers::ChatProvider> {
    let conn = Conn {
        kind: provider["type"].as_str().unwrap_or("openai").to_string(),
        base_url: provider["base_url"].as_str().unwrap_or("").to_string(),
        api_key: next_key(provider["api_keys"].as_str().unwrap_or(""), 0),
        headers: provider["headers"].clone(),
        client: reqwest::Client::new(),
    };
    build(conn)
}

pub(crate) async fn remote_models(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let _ = user;
    let provider = match app.db.provider(&id).await {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    match adapter_for(&provider).list_models().await {
        Ok(models) => ok(json!(models)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn check_provider(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let _ = user;
    let provider = match app.db.provider(&id).await {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    let model = body.get("model").and_then(|v| v.as_str());
    match adapter_for(&provider).check(model).await {
        Ok(()) => ok(json!({"status": true})),
        Err(e) => fail(e),
    }
}
