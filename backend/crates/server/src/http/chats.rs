use super::{fail, ok};
use crate::auth::CurrentUser;
use crate::App;
use axum::{
    extract::{Path, Query, State},
    response::Response,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Deserialize)]
pub(crate) struct PageQuery {
    page: Option<i64>,
}

pub(crate) async fn list_chats(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Query(q): Query<PageQuery>,
) -> Response {
    match app
        .db
        .list_chats(&user.id, q.page.unwrap_or(1), false)
        .await
    {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn new_chat(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    let chat = body.get("chat").cloned().unwrap_or(body);
    match app
        .db
        .insert_chat(&user.id, &chat, &json!({}), false, None)
        .await
    {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
#[derive(Deserialize)]
pub(crate) struct SearchQuery {
    text: Option<String>,
}
pub(crate) async fn search_chats(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Query(q): Query<SearchQuery>,
) -> Response {
    match app
        .db
        .search_chats(&user.id, q.text.as_deref().unwrap_or(""))
        .await
    {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn pinned(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    match app.db.pinned_chats(&user.id).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn all_tags(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    match app.db.all_tags(&user.id).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn import_chat(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    let chat = body.get("chat").cloned().unwrap_or(json!({}));
    let meta = body.get("meta").cloned().unwrap_or(json!({}));
    let pinned = body
        .get("pinned")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let folder = body.get("folder_id").and_then(|v| v.as_str());
    match app
        .db
        .insert_chat(&user.id, &chat, &meta, pinned, folder)
        .await
    {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn get_chat(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    match app.db.chat(&id, &user.id).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn update_chat(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let chat = body.get("chat").cloned().unwrap_or(body);
    match app.db.update_chat(&id, &user.id, &chat).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn delete_chat(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    match app.db.delete_chat(&id, &user.id).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn pin_chat(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let current = app.db.chat(&id, &user.id).await.ok();
    let next = if current
        .as_ref()
        .and_then(|c| c.get("pinned"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        0
    } else {
        1
    };
    match app.db.set_flag(&id, &user.id, "pinned", next).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn archive_chat(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let current = app.db.chat(&id, &user.id).await.ok();
    let next = if current
        .as_ref()
        .and_then(|c| c.get("archived"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        0
    } else {
        1
    };
    match app.db.set_flag(&id, &user.id, "archived", next).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn archived_chats(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    match app.db.archived_chats(&user.id).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn all_chats(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    match app.db.all_chats(&user.id, false).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn all_archived_chats(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    match app.db.all_chats(&user.id, true).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn archive_all_chats(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    match app.db.set_all_flag(&user.id, "archived", 1).await {
        Ok(_) => ok(json!({"status": true})),
        Err(e) => fail(e),
    }
}
#[derive(Deserialize)]
pub(crate) struct TagNameQuery {
    name: Option<String>,
}
pub(crate) async fn chats_by_tag(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(q): Json<TagNameQuery>,
) -> Response {
    let name = q.name.unwrap_or_default();
    if name.is_empty() {
        return ok(json!([]));
    }
    match app.db.chats_by_tag(&user.id, &name).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn clone_chat(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let chat = match app.db.chat(&id, &user.id).await {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    let inner = chat.get("chat").cloned().unwrap_or(json!({}));
    match app
        .db
        .insert_chat(&user.id, &inner, &json!({}), false, None)
        .await
    {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn chat_folder(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let folder = body.get("folder_id").and_then(|v| v.as_str());
    match app.db.set_folder(&id, &user.id, folder).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn chat_tags(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let _ = user;
    match app.db.chat_tags(&id).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn add_chat_tag(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let name = body
        .get("name")
        .or_else(|| body.get("tag"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    match app.db.add_tag(&user.id, &id, name).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn delete_chat_tag(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
    body: Option<Json<Value>>,
) -> Response {
    let empty = json!({});
    let value = match &body {
        Some(Json(v)) => v,
        None => &empty,
    };
    let key = value.get("name").or_else(|| value.get("tag"));
    let name = key.and_then(|x| x.as_str()).unwrap_or("");
    if name.is_empty() {
        return ok(json!([]));
    }
    match app.db.remove_tag(&user.id, &id, name).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}

pub(crate) async fn list_folders(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    match app.db.list_folders(&user.id).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn create_folder(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Folder");
    match app.db.create_folder(&user.id, name, None).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn get_folder(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let _ = user;
    match app.db.folder(&id).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn delete_folder(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    match app.db.delete_folder(&id, &user.id).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn rename_folder(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let _ = user;
    let name = body
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Folder");
    match app.db.update_folder_name(&id, name).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}

pub(crate) async fn list_prompts(State(app): State<Arc<App>>, user: CurrentUser) -> Response {
    match app.db.list_prompts(&user.id).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn create_prompt(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Json(body): Json<Value>,
) -> Response {
    save_prompt(app, user, body).await
}
pub(crate) async fn update_prompt(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(command): Path<String>,
    Json(mut body): Json<Value>,
) -> Response {
    body["command"] = json!(command);
    save_prompt(app, user, body).await
}
pub(crate) async fn save_prompt(app: Arc<App>, user: CurrentUser, body: Value) -> Response {
    let command = body
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("/prompt");
    let title = body
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or(command);
    let content = body.get("content").and_then(|v| v.as_str()).unwrap_or("");
    match app
        .db
        .upsert_prompt(&user.id, command, title, content)
        .await
    {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn delete_prompt(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(command): Path<String>,
) -> Response {
    let _ = user;
    match app.db.delete_prompt(&command).await {
        Ok(v) => ok(json!(v)),
        Err(e) => fail(e),
    }
}
pub(crate) async fn get_prompt(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(command): Path<String>,
) -> Response {
    match app.db.list_prompts(&user.id).await {
        Ok(list) => ok(list
            .into_iter()
            .find(|p| p["command"].as_str() == Some(command.as_str()))
            .unwrap_or(json!(null))),
        Err(e) => fail(e),
    }
}
pub(crate) async fn shared_chat(
    State(app): State<Arc<App>>,
    Path(share_id): Path<String>,
) -> Response {
    match app.db.chat_by_share(&share_id).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn share_chat(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    match app.db.set_share(&id, &user.id, true).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
pub(crate) async fn unshare_chat(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    match app.db.set_share(&id, &user.id, false).await {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}
