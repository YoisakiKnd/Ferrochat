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
        let _ = self.index_chat(&id, user_id, title, chat).await;
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
        let _ = self.index_chat(id, user_id, &title, &merged).await;
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
        Ok(rows.into_iter().map(chat_brief).collect())
    }

    pub async fn all_chats(&self, user_id: &str, archived: bool) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT id, user_id, title, chat_json, created_at, updated_at, share_id, archived, pinned, meta_json, folder_id FROM chats WHERE user_id = ? AND archived = ? ORDER BY updated_at DESC",
        )
        .bind(user_id)
        .bind(archived as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(chat_json).collect())
    }

    pub async fn archived_chats(&self, user_id: &str) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT id, title, created_at, updated_at FROM chats WHERE user_id = ? AND archived = 1 ORDER BY updated_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(chat_brief).collect())
    }

    pub async fn chats_by_tag(&self, user_id: &str, name: &str) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT c.id, c.title, c.created_at, c.updated_at FROM chats c JOIN chat_tags ct ON ct.chat_id = c.id JOIN tags t ON t.id = ct.tag_id WHERE c.user_id = ? AND t.name = ? AND c.archived = 0 ORDER BY c.updated_at DESC",
        )
        .bind(user_id)
        .bind(name)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(chat_brief).collect())
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
        let q = fts_query(text);
        if q.chars().count() >= 3 {
            let rows = sqlx::query(
                "SELECT c.id, c.title, c.created_at, c.updated_at, snippet(chats_fts, 3, '', '', ' … ', 12) AS snippet FROM chats_fts f JOIN chats c ON c.id = f.chat_id WHERE f.user_id = ? AND chats_fts MATCH ? ORDER BY c.updated_at DESC LIMIT 60",
            )
            .bind(user_id)
            .bind(&q)
            .fetch_all(&self.pool)
            .await;
            if let Ok(rows) = rows {
                if !rows.is_empty() {
                    return Ok(rows.into_iter().map(chat_hit).collect());
                }
            }
        }
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
        Ok(rows.into_iter().map(chat_hit).collect())
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

    pub async fn set_all_flag(
        &self,
        user_id: &str,
        column: &str,
        value: i64,
    ) -> Result<u64, AppError> {
        let sql = format!("UPDATE chats SET {column} = ?, updated_at = ? WHERE user_id = ?");
        let r = sqlx::query(&sql)
            .bind(value)
            .bind(now())
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(r.rows_affected())
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

    pub async fn remove_tag(
        &self,
        user_id: &str,
        chat_id: &str,
        name: &str,
    ) -> Result<Vec<Value>, AppError> {
        sqlx::query(
            "DELETE FROM chat_tags WHERE chat_id = ? AND tag_id = (SELECT id FROM tags WHERE user_id = ? AND name = ?)",
        )
        .bind(chat_id)
        .bind(user_id)
        .bind(name)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        sqlx::query(
            "DELETE FROM tags WHERE user_id = ? AND id NOT IN (SELECT tag_id FROM chat_tags)",
        )
        .bind(user_id)
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

    pub async fn replace_passages(
        &self,
        file_id: &str,
        filename: &str,
        passages: &[(i64, String)],
    ) -> Result<(), AppError> {
        sqlx::query("DELETE FROM file_passages WHERE file_id = ?")
            .bind(file_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        for (page, body) in passages {
            let body = body.trim();
            if body.is_empty() {
                continue;
            }
            sqlx::query(
                "INSERT INTO file_passages (file_id, filename, page, body) VALUES (?, ?, ?, ?)",
            )
            .bind(file_id)
            .bind(filename)
            .bind(page.to_string())
            .bind(body)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        }
        Ok(())
    }

    pub async fn search_passages(
        &self,
        file_ids: &[String],
        query: &str,
        limit: i64,
    ) -> Result<Vec<(String, String, i64, String)>, AppError> {
        if file_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<&str> = file_ids
            .iter()
            .map(|id| id.as_str())
            .filter(|id| id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
            .collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let list = ids
            .iter()
            .map(|id| format!("'{id}'"))
            .collect::<Vec<_>>()
            .join(",");
        let q = fts_query(query);
        let sql = if q.chars().count() >= 3 {
            format!(
                "SELECT file_id, filename, page, body FROM file_passages WHERE file_id IN ({list}) AND file_passages MATCH ? ORDER BY rank LIMIT ?"
            )
        } else {
            format!(
                "SELECT file_id, filename, page, body FROM file_passages WHERE file_id IN ({list}) ORDER BY rowid LIMIT ?"
            )
        };
        let mut query = sqlx::query(&sql);
        if q.chars().count() >= 3 {
            query = query.bind(q);
        }
        let rows = query
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let page = row.get::<String, _>("page").parse::<i64>().unwrap_or(0);
                (
                    row.get("file_id"),
                    row.get("filename"),
                    page,
                    row.get("body"),
                )
            })
            .collect())
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

impl Db {
    pub async fn list_memories(&self, user_id: &str) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT id, user_id, content, created_at, updated_at FROM memories WHERE user_id = ? ORDER BY updated_at DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(memory_json).collect())
    }

    pub async fn add_memory(&self, user_id: &str, content: &str) -> Result<Value, AppError> {
        let content = content.trim();
        if content.is_empty() {
            return Err(AppError::BadRequest("content is required".into()));
        }
        let id = Uuid::new_v4().to_string();
        let ts = now();
        sqlx::query(
            "INSERT INTO memories (id, user_id, content, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(user_id)
        .bind(content)
        .bind(ts)
        .bind(ts)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        sqlx::query("INSERT INTO memories_fts (memory_id, user_id, content) VALUES (?, ?, ?)")
            .bind(&id)
            .bind(user_id)
            .bind(content)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(
            json!({"id": id, "user_id": user_id, "content": content, "created_at": ts, "updated_at": ts}),
        )
    }

    pub async fn update_memory(
        &self,
        user_id: &str,
        id: &str,
        content: &str,
    ) -> Result<Value, AppError> {
        let content = content.trim();
        if content.is_empty() {
            return Err(AppError::BadRequest("content is required".into()));
        }
        let ts = now();
        let changed = sqlx::query(
            "UPDATE memories SET content = ?, updated_at = ? WHERE id = ? AND user_id = ?",
        )
        .bind(content)
        .bind(ts)
        .bind(id)
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        if changed.rows_affected() == 0 {
            return Err(AppError::NotFound("memory not found".into()));
        }
        sqlx::query("DELETE FROM memories_fts WHERE memory_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        sqlx::query("INSERT INTO memories_fts (memory_id, user_id, content) VALUES (?, ?, ?)")
            .bind(id)
            .bind(user_id)
            .bind(content)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        let created_at: i64 = sqlx::query_scalar("SELECT created_at FROM memories WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(internal)?;
        Ok(
            json!({"id": id, "user_id": user_id, "content": content, "created_at": created_at, "updated_at": ts}),
        )
    }

    pub async fn delete_memory(&self, user_id: &str, id: &str) -> Result<bool, AppError> {
        let changed = sqlx::query("DELETE FROM memories WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        sqlx::query("DELETE FROM memories_fts WHERE memory_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(changed.rows_affected() > 0)
    }

    pub async fn delete_memories(&self, user_id: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM memories WHERE user_id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        sqlx::query("DELETE FROM memories_fts WHERE user_id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(())
    }

    pub async fn query_memories(
        &self,
        user_id: &str,
        content: &str,
        limit: i64,
    ) -> Result<Vec<(String, i64)>, AppError> {
        let q = fts_query(content);
        let rows = if q.chars().count() >= 3 {
            sqlx::query(
                "SELECT m.content, m.created_at FROM memories_fts f JOIN memories m ON m.id = f.memory_id WHERE f.user_id = ? AND memories_fts MATCH ? ORDER BY rank LIMIT ?",
            )
            .bind(user_id)
            .bind(q)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?
        } else {
            sqlx::query(
                "SELECT content, created_at FROM memories WHERE user_id = ? ORDER BY updated_at DESC LIMIT ?",
            )
            .bind(user_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?
        };
        Ok(rows
            .into_iter()
            .map(|row| (row.get("content"), row.get("created_at")))
            .collect())
    }
}

fn memory_json(row: sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "id": row.get::<String, _>("id"),
        "user_id": row.get::<String, _>("user_id"),
        "content": row.get::<String, _>("content"),
        "created_at": row.get::<i64, _>("created_at"),
        "updated_at": row.get::<i64, _>("updated_at"),
    })
}

fn chat_brief(row: sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "id": row.get::<String, _>("id"),
        "title": row.get::<String, _>("title"),
        "updated_at": row.get::<i64, _>("updated_at"),
        "created_at": row.get::<i64, _>("created_at"),
    })
}

fn chat_hit(row: sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "id": row.get::<String, _>("id"),
        "title": row.get::<String, _>("title"),
        "updated_at": row.get::<i64, _>("updated_at"),
        "created_at": row.get::<i64, _>("created_at"),
        "snippet": row.try_get::<String, _>("snippet").unwrap_or_default(),
    })
}

fn message_text(chat: &Value) -> String {
    let mut out = String::new();
    let push = |out: &mut String, value: &Value| {
        if let Some(text) = value.get("content").and_then(|v| v.as_str()) {
            out.push_str(text);
            out.push('\n');
        }
    };
    if let Some(list) = chat.get("messages").and_then(|v| v.as_array()) {
        for item in list {
            push(&mut out, item);
        }
    }
    if let Some(map) = chat
        .pointer("/history/messages")
        .and_then(|v| v.as_object())
    {
        for item in map.values() {
            push(&mut out, item);
        }
    }
    out
}

impl Db {
    pub async fn index_chat(
        &self,
        id: &str,
        user_id: &str,
        title: &str,
        chat: &Value,
    ) -> Result<(), AppError> {
        sqlx::query("DELETE FROM chats_fts WHERE chat_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        sqlx::query("INSERT INTO chats_fts (chat_id, user_id, title, body) VALUES (?, ?, ?, ?)")
            .bind(id)
            .bind(user_id)
            .bind(title)
            .bind(message_text(chat))
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(())
    }

    pub async fn reindex_chats(&self) -> Result<(), AppError> {
        let rows = sqlx::query("SELECT id, user_id, title, chat_json FROM chats")
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
        for row in rows {
            let chat: Value = serde_json::from_str(row.get("chat_json")).unwrap_or(json!({}));
            self.index_chat(row.get("id"), row.get("user_id"), row.get("title"), &chat)
                .await?;
        }
        Ok(())
    }

    pub async fn remove_builtin_presets(&self) -> Result<(), AppError> {
        sqlx::query(
            "DELETE FROM models WHERE user_id = 'system' AND id IN ('preset-translate', 'preset-polish', 'preset-summary')",
        )
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(())
    }

    pub async fn record_usage(
        &self,
        user_id: &str,
        model: &str,
        prompt_tokens: i64,
        completion_tokens: i64,
        cost: f64,
    ) -> Result<(), AppError> {
        let day = chrono_day();
        sqlx::query(
            "INSERT INTO usage_events (user_id, model, day, prompt_tokens, completion_tokens, cost) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(model)
        .bind(day)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(cost)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(())
    }

    pub async fn usage_summary(&self) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT model, day, SUM(prompt_tokens) AS prompt_tokens, SUM(completion_tokens) AS completion_tokens, SUM(cost) AS cost FROM usage_events GROUP BY model, day ORDER BY day DESC, model LIMIT 60",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                json!({
                    "model": row.get::<String, _>("model"),
                    "day": row.get::<String, _>("day"),
                    "prompt_tokens": row.get::<i64, _>("prompt_tokens"),
                    "completion_tokens": row.get::<i64, _>("completion_tokens"),
                    "cost": row.get::<f64, _>("cost"),
                })
            })
            .collect())
    }
}

fn chrono_day() -> String {
    let secs = now().max(0);
    let days = secs / 86400;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    if m <= 2 {
        y += 1;
    }
    format!("{y:04}-{m:02}-{d:02}")
}

fn fts_query(raw: &str) -> String {
    let mut terms = Vec::new();
    for word in raw.split_whitespace() {
        let clean: String = word
            .chars()
            .filter(|c| c.is_alphanumeric())
            .take(24)
            .collect();
        if clean.chars().count() < 3 {
            continue;
        }
        let chars: Vec<char> = clean.chars().collect();
        let cjk = chars.iter().any(|c| ('\u{4e00}'..='\u{9fff}').contains(c));
        if cjk && chars.len() > 6 {
            let windows: Vec<String> = chars.windows(3).map(|w| w.iter().collect()).collect();
            let step = (windows.len() / 8).max(1);
            for window in windows.into_iter().step_by(step).take(8) {
                terms.push(window);
            }
        } else {
            terms.push(clean);
        }
        if terms.len() >= 8 {
            break;
        }
    }
    terms
        .into_iter()
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>()
        .join(" OR ")
}
