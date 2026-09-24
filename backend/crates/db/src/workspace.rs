use super::{internal, Db};
use ferrochat_core::{now, AppError};
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

impl Db {
    pub async fn insert_chat(
        &self,
        user_id: &str,
        chat: &Value,
        meta: &Value,
        pinned: bool,
        folder_id: Option<&str>,
    ) -> Result<Value, AppError> {
        let id = Uuid::new_v4().to_string();
        let title = chat
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("New Chat");
        let ts = now();
        sqlx::query(
            "INSERT INTO chats (id, user_id, title, chat_json, created_at, updated_at, pinned, meta_json, folder_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(user_id)
        .bind(title)
        .bind(chat.to_string())
        .bind(ts)
        .bind(ts)
        .bind(pinned as i64)
        .bind(meta.to_string())
        .bind(folder_id)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        self.chat(&id, user_id).await
    }

    pub async fn update_chat(
        &self,
        id: &str,
        user_id: &str,
        chat: &Value,
    ) -> Result<Value, AppError> {
        let existing: Option<String> =
            sqlx::query_scalar("SELECT chat_json FROM chats WHERE id = ? AND user_id = ?")
                .bind(id)
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?;
        let mut merged: Value = existing
            .and_then(|s| serde_json::from_str(&s).ok())
            .filter(Value::is_object)
            .unwrap_or(json!({}));
        if let (Some(target), Some(patch)) = (merged.as_object_mut(), chat.as_object()) {
            for (k, v) in patch {
                target.insert(k.clone(), v.clone());
            }
        }
        let title = merged
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("New Chat")
            .to_string();
        sqlx::query("UPDATE chats SET title = ?, chat_json = ?, updated_at = ? WHERE id = ? AND user_id = ?")
            .bind(&title)
            .bind(merged.to_string())
            .bind(now())
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        self.chat(id, user_id).await
    }

    pub async fn set_chat_title(&self, id: &str, title: &str) -> Result<(), AppError> {
        let row = sqlx::query("SELECT chat_json FROM chats WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(internal)?;
        if let Some(row) = row {
            let mut chat: Value = serde_json::from_str(row.get("chat_json")).unwrap_or(json!({}));
            chat["title"] = json!(title);
            sqlx::query("UPDATE chats SET title = ?, chat_json = ?, updated_at = ? WHERE id = ?")
                .bind(title)
                .bind(chat.to_string())
                .bind(now())
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(internal)?;
        }
        Ok(())
    }

    pub async fn chat(&self, id: &str, user_id: &str) -> Result<Value, AppError> {
        let row = sqlx::query(
            "SELECT id, user_id, title, chat_json, created_at, updated_at, share_id, archived, pinned, meta_json, folder_id FROM chats WHERE id = ? AND user_id = ?",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound("chat not found".into()))?;
        Ok(chat_json(row))
    }

    pub async fn list_chats(
        &self,
        user_id: &str,
        page: i64,
        archived: bool,
    ) -> Result<Vec<Value>, AppError> {
        let limit = 60;
        let offset = page.saturating_sub(1) * limit;
        let rows = sqlx::query(
            "SELECT id, title, created_at, updated_at FROM chats WHERE user_id = ? AND archived = ? ORDER BY updated_at DESC LIMIT ? OFFSET ?",
        )
        .bind(user_id)
        .bind(archived as i64)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows
            .into_iter()
            .map(|r| {
                json!({
                    "id": r.get::<String, _>("id"),
                    "title": r.get::<String, _>("title"),
                    "updated_at": r.get::<i64, _>("updated_at"),
                    "created_at": r.get::<i64, _>("created_at"),
                })
            })
            .collect())
    }

    pub async fn pinned_chats(&self, user_id: &str) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT id, user_id, title, chat_json, created_at, updated_at, share_id, archived, pinned, meta_json, folder_id FROM chats WHERE user_id = ? AND pinned = 1 AND archived = 0 ORDER BY updated_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(chat_json).collect())
    }

    pub async fn search_chats(&self, user_id: &str, text: &str) -> Result<Vec<Value>, AppError> {
        let like = format!("%{text}%");
        let rows = sqlx::query(
            "SELECT id, title, created_at, updated_at FROM chats WHERE user_id = ? AND (title LIKE ? OR chat_json LIKE ?) ORDER BY updated_at DESC LIMIT 60",
        )
        .bind(user_id)
        .bind(&like)
        .bind(&like)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows
            .into_iter()
            .map(|r| json!({"id": r.get::<String,_>("id"), "title": r.get::<String,_>("title"), "updated_at": r.get::<i64,_>("updated_at"), "created_at": r.get::<i64,_>("created_at")}))
            .collect())
    }

    pub async fn delete_chat(&self, id: &str, user_id: &str) -> Result<bool, AppError> {
        let r = sqlx::query("DELETE FROM chats WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn set_flag(
        &self,
        id: &str,
        user_id: &str,
        column: &str,
        value: i64,
    ) -> Result<Value, AppError> {
        let sql =
            format!("UPDATE chats SET {column} = ?, updated_at = ? WHERE id = ? AND user_id = ?");
        sqlx::query(&sql)
            .bind(value)
            .bind(now())
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        self.chat(id, user_id).await
    }

    pub async fn set_share(
        &self,
        id: &str,
        user_id: &str,
        shared: bool,
    ) -> Result<Value, AppError> {
        let share_id = if shared {
            Some(Uuid::new_v4().to_string())
        } else {
            None
        };
        sqlx::query("UPDATE chats SET share_id = ?, updated_at = ? WHERE id = ? AND user_id = ?")
            .bind(&share_id)
            .bind(now())
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        self.chat(id, user_id).await
    }

    pub async fn chat_by_share(&self, share_id: &str) -> Result<Value, AppError> {
        let row = sqlx::query(
            "SELECT id, user_id, title, chat_json, created_at, updated_at, share_id, archived, pinned, meta_json, folder_id FROM chats WHERE share_id = ?",
        )
        .bind(share_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound("shared chat not found".into()))?;
        Ok(chat_json(row))
    }

    pub async fn set_folder(
        &self,
        id: &str,
        user_id: &str,
        folder_id: Option<&str>,
    ) -> Result<Value, AppError> {
        sqlx::query("UPDATE chats SET folder_id = ?, updated_at = ? WHERE id = ? AND user_id = ?")
            .bind(folder_id)
            .bind(now())
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        self.chat(id, user_id).await
    }

    pub async fn all_tags(&self, user_id: &str) -> Result<Vec<Value>, AppError> {
        let rows =
            sqlx::query("SELECT id, name, user_id FROM tags WHERE user_id = ? ORDER BY name")
                .bind(user_id)
                .fetch_all(&self.pool)
                .await
                .map_err(internal)?;
        Ok(rows
            .into_iter()
            .map(|r| json!({"id": r.get::<String,_>("id"), "name": r.get::<String,_>("name"), "user_id": r.get::<String,_>("user_id")}))
            .collect())
    }

    pub async fn chat_tags(&self, chat_id: &str) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT t.id, t.name, t.user_id FROM tags t JOIN chat_tags ct ON ct.tag_id = t.id WHERE ct.chat_id = ?",
        )
        .bind(chat_id)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows
            .into_iter()
            .map(|r| json!({"id": r.get::<String,_>("id"), "name": r.get::<String,_>("name"), "user_id": r.get::<String,_>("user_id")}))
            .collect())
    }

    pub async fn add_tag(
        &self,
        user_id: &str,
        chat_id: &str,
        name: &str,
    ) -> Result<Vec<Value>, AppError> {
        let existing = sqlx::query("SELECT id FROM tags WHERE user_id = ? AND name = ?")
            .bind(user_id)
            .bind(name)
            .fetch_optional(&self.pool)
            .await
            .map_err(internal)?;
        let tag_id = if let Some(row) = existing {
            row.get::<String, _>("id")
        } else {
            let id = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO tags (id, name, user_id) VALUES (?, ?, ?)")
                .bind(&id)
                .bind(name)
                .bind(user_id)
                .execute(&self.pool)
                .await
                .map_err(internal)?;
            id
        };
        sqlx::query("INSERT OR IGNORE INTO chat_tags (chat_id, tag_id) VALUES (?, ?)")
            .bind(chat_id)
            .bind(&tag_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        self.chat_tags(chat_id).await
    }

    pub async fn list_folders(&self, user_id: &str) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT id, parent_id, user_id, name, items_json, meta_json, is_expanded, created_at, updated_at FROM folders WHERE user_id = ? ORDER BY name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(folder_json).collect())
    }

    pub async fn create_folder(
        &self,
        user_id: &str,
        name: &str,
        parent_id: Option<&str>,
    ) -> Result<Value, AppError> {
        let id = Uuid::new_v4().to_string();
        let ts = now();
        sqlx::query(
            "INSERT INTO folders (id, parent_id, user_id, name, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(parent_id)
        .bind(user_id)
        .bind(name)
        .bind(ts)
        .bind(ts)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        self.folder(&id).await
    }

    pub async fn folder(&self, id: &str) -> Result<Value, AppError> {
        let row = sqlx::query(
            "SELECT id, parent_id, user_id, name, items_json, meta_json, is_expanded, created_at, updated_at FROM folders WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound("folder not found".into()))?;
        Ok(folder_json(row))
    }

    pub async fn update_folder_name(&self, id: &str, name: &str) -> Result<Value, AppError> {
        sqlx::query("UPDATE folders SET name = ?, updated_at = ? WHERE id = ?")
            .bind(name)
            .bind(now())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        self.folder(id).await
    }

    pub async fn delete_folder(&self, id: &str, user_id: &str) -> Result<bool, AppError> {
        let r = sqlx::query("DELETE FROM folders WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn list_prompts(&self, user_id: &str) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT command, user_id, title, content, timestamp FROM prompts WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(prompt_json).collect())
    }

    pub async fn upsert_prompt(
        &self,
        user_id: &str,
        command: &str,
        title: &str,
        content: &str,
    ) -> Result<Value, AppError> {
        let cmd = if command.starts_with('/') {
            command.to_string()
        } else {
            format!("/{command}")
        };
        sqlx::query(
            "INSERT INTO prompts (command, user_id, title, content, timestamp) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(command) DO UPDATE SET title=excluded.title, content=excluded.content, timestamp=excluded.timestamp",
        )
        .bind(&cmd)
        .bind(user_id)
        .bind(title)
        .bind(content)
        .bind(now())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(
            json!({"command": cmd, "user_id": user_id, "title": title, "content": content, "timestamp": now()}),
        )
    }

    pub async fn delete_prompt(&self, command: &str) -> Result<bool, AppError> {
        let r = sqlx::query("DELETE FROM prompts WHERE command = ?")
            .bind(command)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn list_workspace_models(&self) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT id, user_id, base_model_id, name, params_json, meta_json, is_active, created_at, updated_at FROM models ORDER BY updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(model_json).collect())
    }

    pub async fn upsert_workspace_model(
        &self,
        user_id: &str,
        body: &Value,
    ) -> Result<Value, AppError> {
        let id = body.get("id").and_then(|v| v.as_str()).unwrap_or("model");
        let name = body.get("name").and_then(|v| v.as_str()).unwrap_or(id);
        let base = body.get("base_model_id").and_then(|v| v.as_str());
        let params = body.get("params").cloned().unwrap_or(json!({}));
        let meta = body.get("meta").cloned().unwrap_or(json!({}));
        let active = body
            .get("is_active")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let ts = now();
        sqlx::query(
            "INSERT INTO models (id, user_id, base_model_id, name, params_json, meta_json, is_active, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET base_model_id=excluded.base_model_id, name=excluded.name, params_json=excluded.params_json, meta_json=excluded.meta_json, is_active=excluded.is_active, updated_at=excluded.updated_at",
        )
        .bind(id)
        .bind(user_id)
        .bind(base)
        .bind(name)
        .bind(params.to_string())
        .bind(meta.to_string())
        .bind(active as i64)
        .bind(ts)
        .bind(ts)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        self.workspace_model(id).await
    }

    pub async fn workspace_model(&self, id: &str) -> Result<Value, AppError> {
        let row = sqlx::query(
            "SELECT id, user_id, base_model_id, name, params_json, meta_json, is_active, created_at, updated_at FROM models WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound("model not found".into()))?;
        Ok(model_json(row))
    }

    pub async fn toggle_workspace_model(&self, id: &str) -> Result<Value, AppError> {
        sqlx::query("UPDATE models SET is_active = CASE WHEN is_active = 1 THEN 0 ELSE 1 END, updated_at = ? WHERE id = ?")
            .bind(now())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        self.workspace_model(id).await
    }

    pub async fn delete_workspace_model(&self, id: &str) -> Result<bool, AppError> {
        let r = sqlx::query("DELETE FROM models WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn list_tool_servers(&self) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query("SELECT id, user_id, type, name, config_json, enabled, created_at, updated_at FROM tool_servers ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
        Ok(rows.into_iter().map(tool_server_json).collect())
    }

    pub async fn upsert_tool_server(&self, user_id: &str, body: &Value) -> Result<Value, AppError> {
        let id = body
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let kind = body
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("mcp_http");
        let name = body.get("name").and_then(|v| v.as_str()).unwrap_or("MCP");
        let config = body.get("config").cloned().unwrap_or(json!({}));
        let enabled = body
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let ts = now();
        sqlx::query(
            "INSERT INTO tool_servers (id, user_id, type, name, config_json, enabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET type=excluded.type, name=excluded.name, config_json=excluded.config_json, enabled=excluded.enabled, updated_at=excluded.updated_at",
        )
        .bind(&id)
        .bind(user_id)
        .bind(kind)
        .bind(name)
        .bind(config.to_string())
        .bind(enabled as i64)
        .bind(ts)
        .bind(ts)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(
            json!({"id": id, "user_id": user_id, "type": kind, "name": name, "config": config, "enabled": enabled, "created_at": ts, "updated_at": ts}),
        )
    }

    pub async fn delete_tool_server(&self, id: &str) -> Result<bool, AppError> {
        let r = sqlx::query("DELETE FROM tool_servers WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(r.rows_affected() > 0)
    }

    pub async fn insert_file(
        &self,
        user_id: &str,
        filename: &str,
        path: &str,
        meta: &Value,
    ) -> Result<Value, AppError> {
        let id = Uuid::new_v4().to_string();
        let ts = now();
        sqlx::query("INSERT INTO files (id, user_id, filename, path, meta_json, created_at) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(&id)
            .bind(user_id)
            .bind(filename)
            .bind(path)
            .bind(meta.to_string())
            .bind(ts)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(
            json!({"id": id, "user_id": user_id, "filename": filename, "meta": meta, "created_at": ts}),
        )
    }

    pub async fn file_path(&self, id: &str) -> Result<(String, String), AppError> {
        let row = sqlx::query("SELECT filename, path FROM files WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(internal)?
            .ok_or_else(|| AppError::NotFound("file not found".into()))?;
        Ok((row.get("filename"), row.get("path")))
    }
}

fn chat_json(row: sqlx::sqlite::SqliteRow) -> Value {
    let chat: Value = serde_json::from_str(row.get("chat_json")).unwrap_or(json!({}));
    let meta: Value = serde_json::from_str(row.get("meta_json")).unwrap_or(json!({}));
    json!({
        "id": row.get::<String, _>("id"),
        "user_id": row.get::<String, _>("user_id"),
        "title": row.get::<String, _>("title"),
        "chat": chat,
        "created_at": row.get::<i64, _>("created_at"),
        "updated_at": row.get::<i64, _>("updated_at"),
        "share_id": row.get::<Option<String>, _>("share_id"),
        "archived": row.get::<i64, _>("archived") != 0,
        "pinned": row.get::<i64, _>("pinned") != 0,
        "meta": meta,
        "folder_id": row.get::<Option<String>, _>("folder_id"),
    })
}

fn folder_json(row: sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "id": row.get::<String, _>("id"),
        "parent_id": row.get::<Option<String>, _>("parent_id"),
        "user_id": row.get::<String, _>("user_id"),
        "name": row.get::<String, _>("name"),
        "items": serde_json::from_str::<Value>(row.get("items_json")).unwrap_or(json!({})),
        "meta": serde_json::from_str::<Value>(row.get("meta_json")).unwrap_or(json!({})),
        "is_expanded": row.get::<i64, _>("is_expanded") != 0,
        "created_at": row.get::<i64, _>("created_at"),
        "updated_at": row.get::<i64, _>("updated_at"),
    })
}

fn prompt_json(row: sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "command": row.get::<String, _>("command"),
        "user_id": row.get::<String, _>("user_id"),
        "title": row.get::<String, _>("title"),
        "content": row.get::<String, _>("content"),
        "timestamp": row.get::<i64, _>("timestamp"),
    })
}

fn model_json(row: sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "id": row.get::<String, _>("id"),
        "user_id": row.get::<String, _>("user_id"),
        "base_model_id": row.get::<Option<String>, _>("base_model_id"),
        "name": row.get::<String, _>("name"),
        "params": serde_json::from_str::<Value>(row.get("params_json")).unwrap_or(json!({})),
        "meta": serde_json::from_str::<Value>(row.get("meta_json")).unwrap_or(json!({})),
        "is_active": row.get::<i64, _>("is_active") != 0,
        "created_at": row.get::<i64, _>("created_at"),
        "updated_at": row.get::<i64, _>("updated_at"),
    })
}

fn tool_server_json(row: sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "id": row.get::<String, _>("id"),
        "user_id": row.get::<String, _>("user_id"),
        "type": row.get::<String, _>("type"),
        "name": row.get::<String, _>("name"),
        "config": serde_json::from_str::<Value>(row.get("config_json")).unwrap_or(json!({})),
        "enabled": row.get::<i64, _>("enabled") != 0,
        "created_at": row.get::<i64, _>("created_at"),
        "updated_at": row.get::<i64, _>("updated_at"),
    })
}
