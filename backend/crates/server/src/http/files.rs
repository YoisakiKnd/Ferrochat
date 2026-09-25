use super::{fail, ok};
use crate::auth::CurrentUser;
use crate::App;
use axum::{
    extract::{Multipart, Path, State},
    http::header,
    response::{IntoResponse, Response},
};
use calamine::Reader;
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
        let pages = extract_pages(&name, &bytes);
        let text = pages
            .iter()
            .map(|(_, body)| body.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let meta = json!({
            "size": bytes.len(),
            "content_type": mime_guess::from_path(&name).first_or_octet_stream().to_string(),
            "content": text.chars().take(80_000).collect::<String>()
        });
        saved = Some(
            app.db
                .insert_file(&user.id, &name, &path.display().to_string(), &meta)
                .await,
        );
        if let Some(Ok(file)) = &saved {
            if let Some(id) = file.get("id").and_then(|v| v.as_str()) {
                let passages = chunk_pages(&pages);
                let _ = app.db.replace_passages(id, &name, &passages).await;
            }
        }
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
        Ok((name, path)) => match tokio::fs::read(path).await {
            Ok(bytes) => {
                let mime = mime_guess::from_path(&name)
                    .first_or_octet_stream()
                    .to_string();
                (
                    [(header::CONTENT_TYPE, mime), (header::CONTENT_DISPOSITION, "inline".into())],
                    bytes,
                )
                    .into_response()
            }
            Err(e) => fail(AppError::Internal(e.to_string())),
        },
        Err(e) => fail(e),
    }
}

fn extract_pages(name: &str, bytes: &[u8]) -> Vec<(i64, String)> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".pdf") {
        return pdf_extract::extract_text_from_mem_by_pages(bytes)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, text)| (i as i64 + 1, text))
            .filter(|(_, text)| !text.trim().is_empty())
            .collect();
    }
    let text = if lower.ends_with(".docx") {
        docx_text(bytes)
    } else if lower.ends_with(".xlsx") || lower.ends_with(".xls") {
        sheet_text(bytes)
    } else if lower.ends_with(".csv")
        || lower.ends_with(".txt")
        || lower.ends_with(".md")
        || lower.ends_with(".json")
        || lower.ends_with(".rs")
        || lower.ends_with(".py")
        || lower.ends_with(".ts")
        || lower.ends_with(".js")
    {
        String::from_utf8_lossy(bytes).to_string()
    } else {
        String::from_utf8(bytes.to_vec()).unwrap_or_default()
    };
    if text.trim().is_empty() {
        Vec::new()
    } else {
        vec![(0, text)]
    }
}

fn chunk_pages(pages: &[(i64, String)]) -> Vec<(i64, String)> {
    let mut out = Vec::new();
    for (page, text) in pages {
        let mut buf = String::new();
        for word in text.split_whitespace() {
            if buf.chars().count() + word.chars().count() > 700 && !buf.is_empty() {
                out.push((*page, std::mem::take(&mut buf)));
                if out.len() >= 200 {
                    return out;
                }
            }
            if !buf.is_empty() {
                buf.push(' ');
            }
            buf.push_str(word);
        }
        if !buf.trim().is_empty() {
            out.push((*page, buf));
            if out.len() >= 200 {
                break;
            }
        }
    }
    out
}

fn docx_text(bytes: &[u8]) -> String {
    let cursor = std::io::Cursor::new(bytes);
    let Ok(mut archive) = zip::ZipArchive::new(cursor) else {
        return String::new();
    };
    let Ok(mut file) = archive.by_name("word/document.xml") else {
        return String::new();
    };
    let mut xml = String::new();
    if std::io::Read::read_to_string(&mut file, &mut xml).is_err() {
        return String::new();
    }
    let mut out = String::new();
    let mut in_tag = false;
    for ch in xml.chars() {
        if ch == '<' {
            in_tag = true;
            continue;
        }
        if ch == '>' {
            in_tag = false;
            out.push(' ');
            continue;
        }
        if !in_tag {
            out.push(ch);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn sheet_text(bytes: &[u8]) -> String {
    let cursor = std::io::Cursor::new(bytes);
    let Ok(mut book) = calamine::open_workbook_auto_from_rs(cursor) else {
        return String::new();
    };
    let mut out = String::new();
    let names = book.sheet_names().to_vec();
    for name in names {
        let Ok(range) = book.worksheet_range(&name) else {
            continue;
        };
        out.push_str(&name);
        out.push('\n');
        for row in range.rows() {
            let line = row
                .iter()
                .map(|cell| cell.to_string())
                .collect::<Vec<_>>()
                .join("\t");
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}
