use super::{fail, ok};
use crate::auth::CurrentUser;
use crate::App;
use axum::{
    extract::{Multipart, Path, State},
    http::header,
    response::{IntoResponse, Response},
};
use ferrochat_core::AppError;
use serde_json::json;
use std::sync::Arc;

pub(crate) async fn upload_file(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    mut multipart: Multipart,
) -> Response {
    let mut saved = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.file_name().unwrap_or("upload").to_string();
        let bytes = match field.bytes().await {
            Ok(b) => b,
            Err(e) => return fail(AppError::BadRequest(e.to_string())),
        };
        let dir = app.data_dir.join("uploads");
        if tokio::fs::create_dir_all(&dir).await.is_err() {
            return fail(AppError::Internal("cannot create uploads".into()));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let path = dir.join(&id);
        if tokio::fs::write(&path, &bytes).await.is_err() {
            return fail(AppError::Internal("cannot write upload".into()));
        }
        let meta = json!({"size": bytes.len(), "content_type": "application/octet-stream"});
        saved = Some(
            app.db
                .insert_file(&user.id, &name, &path.display().to_string(), &meta)
                .await,
        );
        break;
    }
    match saved {
        Some(Ok(v)) => ok(v),
        Some(Err(e)) => fail(e),
        None => fail(AppError::BadRequest("no file".into())),
    }
}
pub(crate) async fn file_content(
    State(app): State<Arc<App>>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> Response {
    let _ = user;
    match app.db.file_path(&id).await {
        Ok((_, path)) => match tokio::fs::read(path).await {
            Ok(bytes) => {
                ([(header::CONTENT_TYPE, "application/octet-stream")], bytes).into_response()
            }
            Err(e) => fail(AppError::Internal(e.to_string())),
        },
        Err(e) => fail(e),
    }
}
