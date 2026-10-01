mod workspace;

use ferrochat_core::{group_name, infer_capabilities, now, AppError};
use serde_json::{json, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Clone)]
pub struct Db {
    pub pool: SqlitePool,
}

#[cfg(test)]
mod access_tests {
    use super::*;

    #[tokio::test]
    async fn legacy_share_links_are_snapshotted_during_upgrade() {
        let dir = std::env::temp_dir().join(format!("ferrochat-share-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let url = format!("sqlite:{}?mode=rwc", dir.join("test.db").display());
        let db = Db::connect(&url).await.unwrap();
        let user = db
            .insert_first_user("share@example.com", "Share", "hash", "admin")
            .await
            .unwrap();
        let user_id = user["id"].as_str().unwrap();
        let chat = db
            .insert_chat(
                user_id,
                &json!({"title":"Before upgrade","messages":[]}),
                &json!({}),
                false,
                None,
            )
            .await
            .unwrap();
        let chat_id = chat["id"].as_str().unwrap();
        sqlx::query("UPDATE chats SET share_id = 'legacy-link' WHERE id = ?")
            .bind(chat_id)
            .execute(&db.pool)
            .await
            .unwrap();
        sqlx::query("ALTER TABLE chats DROP COLUMN shared_chat_json")
            .execute(&db.pool)
            .await
            .unwrap();
        db.pool.close().await;
        let upgraded = Db::connect(&url).await.unwrap();
        upgraded
            .update_chat(chat_id, user_id, &json!({"title":"After upgrade"}))
            .await
            .unwrap();
        assert_eq!(
            upgraded.chat_by_share("legacy-link").await.unwrap()["title"],
            "Before upgrade"
        );
        upgraded.pool.close().await;
        let reopened = Db::connect(&url).await.unwrap();
        assert_eq!(
            reopened.chat_by_share("legacy-link").await.unwrap()["title"],
            "Before upgrade"
        );
        reopened.pool.close().await;
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn first_user_and_file_passages_are_user_scoped() {
        let dir = std::env::temp_dir().join(format!("ferrochat-db-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let url = format!("sqlite:{}?mode=rwc", dir.join("test.db").display());
        let db = Db::connect(&url).await.unwrap();
        let first = db
            .insert_first_user("first@example.com", "First", "hash", "admin")
            .await
            .unwrap();
        assert!(db
            .insert_first_user("second@example.com", "Second", "hash", "admin")
            .await
            .is_err());
        let first_id = first["id"].as_str().unwrap();
        let second_id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users (id, email, name, password_hash, role, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(&second_id).bind("second@example.com").bind("Second").bind("hash").bind("user")
            .bind(now()).bind(now()).execute(&db.pool).await.unwrap();
        assert!(db.is_primary_user(first_id).await.unwrap());
        assert!(!db.is_primary_user(&second_id).await.unwrap());
        let chat = db
            .insert_chat(
                first_id,
                &json!({"title":"Original"}),
                &json!({}),
                false,
                None,
            )
            .await
            .unwrap();
        let chat_id = chat["id"].as_str().unwrap();
        db.set_chat_title(chat_id, &second_id, "Foreign")
            .await
            .unwrap();
        assert_eq!(
            db.chat(chat_id, first_id).await.unwrap()["title"],
            "Original"
        );
        assert!(db.add_tag(&second_id, chat_id, "foreign").await.is_err());
        let file = db
            .insert_file(first_id, "note.txt", "/tmp/note.txt", &json!({}))
            .await
            .unwrap();
        let file_id = file["id"].as_str().unwrap().to_string();
        assert!(db.file_path(&file_id, &second_id).await.is_err());
        db.replace_passages(&file_id, "note.txt", &[(0, "secret body".into())])
            .await
            .unwrap();
        assert!(db
            .search_passages(&second_id, &[file_id.clone()], "", 6)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            db.search_passages(first_id, &[file_id.clone()], "", 6)
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(db
            .search_passages(&second_id, &[file_id.clone()], "secret", 6)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            db.search_passages(first_id, &[file_id], "secret", 6)
                .await
                .unwrap()
                .len(),
            1
        );
        let folder = db.create_folder(first_id, "Private", None).await.unwrap();
        let folder_id = folder["id"].as_str().unwrap();
        assert!(db.folder(folder_id, &second_id).await.is_err());
        assert!(db
            .update_folder_name(folder_id, &second_id, "Changed")
            .await
            .is_err());
        assert_eq!(
            db.folder(folder_id, first_id).await.unwrap()["name"],
            "Private"
        );
        let second_chat = db
            .insert_chat(
                &second_id,
                &json!({"title":"Second"}),
                &json!({}),
                false,
                None,
            )
            .await
            .unwrap();
        assert!(db
            .set_folder(
                second_chat["id"].as_str().unwrap(),
                &second_id,
                Some(folder_id)
            )
            .await
            .is_err());
        assert!(db
            .insert_chat(&second_id, &json!({}), &json!({}), false, Some(folder_id))
            .await
            .is_err());
        assert!(db
            .set_folder_expanded(folder_id, &second_id, true)
            .await
            .is_err());
        assert!(db
            .create_folder(&second_id, "Foreign child", Some(folder_id))
            .await
            .is_err());
        let child = db
            .create_folder(first_id, "Child", Some(folder_id))
            .await
            .unwrap();
        let child_id = child["id"].as_str().unwrap();
        assert!(db
            .set_folder_parent(folder_id, first_id, Some(child_id))
            .await
            .is_err());
        db.set_folder_expanded(folder_id, first_id, true)
            .await
            .unwrap();
        db.set_folder(chat_id, first_id, Some(folder_id))
            .await
            .unwrap();
        let folders = db.list_folders(first_id).await.unwrap();
        let populated = folders
            .iter()
            .find(|folder| folder["id"] == folder_id)
            .unwrap();
        assert_eq!(populated["items"]["chats"][0]["id"], chat_id);
        assert_eq!(populated["is_expanded"], true);
        assert!(db.list_chats(first_id, 1, false).await.unwrap().is_empty());
        db.delete_folder(folder_id, first_id).await.unwrap();
        assert!(db.folder(child_id, first_id).await.unwrap()["parent_id"].is_null());
        assert!(db.chat(chat_id, first_id).await.unwrap()["folder_id"].is_null());
        assert_eq!(db.list_chats(first_id, 1, false).await.unwrap().len(), 1);

        db.upsert_prompt(first_id, "/private", "Original", "secret")
            .await
            .unwrap();
        assert!(db
            .upsert_prompt(&second_id, "/private", "Changed", "leak")
            .await
            .is_err());
        assert!(!db.delete_prompt("/private", &second_id).await.unwrap());
        assert_eq!(
            db.list_prompts(first_id).await.unwrap()[0]["content"],
            "secret"
        );
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

impl Db {
    pub async fn connect(url: &str) -> Result<Self, AppError> {
        let opts = SqliteConnectOptions::from_str(url)
            .map_err(|e| AppError::Internal(e.to_string()))?
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .min_connections(1)
            .idle_timeout(std::time::Duration::from_secs(60))
            .connect_with(opts)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let sql = include_str!("../migrations/001_init.sql");
        sqlx::raw_sql(sql)
            .execute(&pool)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let db = Self { pool };
        db.apply_later_migrations().await?;
        db.normalize_settings().await?;
        db.seed_providers().await?;
        Ok(db)
    }

    pub async fn user_count(&self) -> Result<i64, AppError> {
        let n: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await
            .map_err(internal)?;
        Ok(n.0)
    }

    pub async fn is_primary_user(&self, id: &str) -> Result<bool, AppError> {
        let primary: Option<String> =
            sqlx::query_scalar("SELECT id FROM users ORDER BY created_at ASC, rowid ASC LIMIT 1")
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?;
        Ok(primary.as_deref() == Some(id))
    }

    pub async fn insert_first_user(
        &self,
        email: &str,
        name: &str,
        password_hash: &str,
        role: &str,
    ) -> Result<Value, AppError> {
        let id = Uuid::new_v4().to_string();
        let ts = now();
        let inserted = sqlx::query(
            "INSERT INTO users (id, email, name, password_hash, role, created_at, updated_at) SELECT ?, ?, ?, ?, ?, ?, ? WHERE NOT EXISTS (SELECT 1 FROM users)",
        )
        .bind(&id)
        .bind(email)
        .bind(name)
        .bind(password_hash)
        .bind(role)
        .bind(ts)
        .bind(ts)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        if inserted.rows_affected() == 0 {
            return Err(AppError::BadRequest("signup is closed".into()));
        }
        self.user_by_id(&id).await
    }

    pub async fn user_by_email(&self, email: &str) -> Result<Option<UserRow>, AppError> {
        let row = sqlx::query(
            "SELECT id, email, name, password_hash, role, profile_image_url FROM users WHERE email = ?",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?;
        Ok(row.map(map_user))
    }

    pub async fn user_by_id(&self, id: &str) -> Result<Value, AppError> {
        let row =
            sqlx::query("SELECT id, email, name, role, profile_image_url FROM users WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?
                .ok_or_else(|| AppError::NotFound("user not found".into()))?;
        Ok(json!({
            "id": row.get::<String, _>("id"),
            "email": row.get::<String, _>("email"),
            "name": row.get::<String, _>("name"),
            "role": row.get::<String, _>("role"),
            "profile_image_url": row.get::<String, _>("profile_image_url"),
        }))
    }

    pub async fn password_hash(&self, id: &str) -> Result<String, AppError> {
        let row = sqlx::query("SELECT password_hash FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(internal)?
            .ok_or_else(|| AppError::NotFound("user not found".into()))?;
        Ok(row.get("password_hash"))
    }

    pub async fn update_profile(
        &self,
        id: &str,
        name: &str,
        image: &str,
    ) -> Result<Value, AppError> {
        sqlx::query(
            "UPDATE users SET name = ?, profile_image_url = ?, updated_at = ? WHERE id = ?",
        )
        .bind(name)
        .bind(image)
        .bind(now())
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        self.user_by_id(id).await
    }

    pub async fn update_password(&self, id: &str, hash: &str) -> Result<(), AppError> {
        sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?")
            .bind(hash)
            .bind(now())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(())
    }

    pub async fn get_settings(&self, id: &str) -> Result<Value, AppError> {
        let row = sqlx::query("SELECT settings_json FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(internal)?
            .ok_or_else(|| AppError::NotFound("user not found".into()))?;
        let raw: String = row.get("settings_json");
        Ok(serde_json::from_str(&raw).unwrap_or_else(|_| json!({})))
    }

    pub async fn set_settings(&self, id: &str, settings: &Value) -> Result<Value, AppError> {
        sqlx::query("UPDATE users SET settings_json = ?, updated_at = ? WHERE id = ?")
            .bind(settings.to_string())
            .bind(now())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(settings.clone())
    }

    pub async fn config_get(&self, key: &str) -> Result<Option<Value>, AppError> {
        let row = sqlx::query("SELECT value_json FROM config WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(internal)?;
        Ok(row.and_then(|r| serde_json::from_str(r.get("value_json")).ok()))
    }

    pub async fn config_set(&self, key: &str, value: &Value) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO config (key, value_json) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        )
        .bind(key)
        .bind(value.to_string())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(())
    }

    async fn seed_providers(&self) -> Result<(), AppError> {
        let builtins = [
            ("openai", "openai", "OpenAI", "https://api.openai.com/v1"),
            (
                "anthropic",
                "anthropic",
                "Anthropic",
                "https://api.anthropic.com",
            ),
            (
                "gemini",
                "gemini",
                "Gemini",
                "https://generativelanguage.googleapis.com/v1beta",
            ),
            (
                "deepseek",
                "openai",
                "DeepSeek",
                "https://api.deepseek.com/v1",
            ),
            (
                "openrouter",
                "openai",
                "OpenRouter",
                "https://openrouter.ai/api/v1",
            ),
            (
                "siliconflow",
                "openai",
                "SiliconFlow",
                "https://api.siliconflow.cn/v1",
            ),
            ("ollama", "ollama", "Ollama", "http://127.0.0.1:11434/v1"),
            (
                "azure",
                "azure",
                "Azure OpenAI",
                "https://YOUR_RESOURCE.openai.azure.com/openai/deployments",
            ),
            ("groq", "openai", "Groq", "https://api.groq.com/openai/v1"),
            (
                "moonshot",
                "openai",
                "Moonshot (Kimi)",
                "https://api.moonshot.cn/v1",
            ),
            (
                "zhipu",
                "openai",
                "Zhipu (GLM)",
                "https://open.bigmodel.cn/api/paas/v4",
            ),
            (
                "dashscope",
                "openai",
                "DashScope (Qwen)",
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
            ),
            (
                "volcengine",
                "openai",
                "Volcengine Ark (Doubao)",
                "https://ark.cn-beijing.volces.com/api/v3",
            ),
            (
                "minimax",
                "openai",
                "MiniMax",
                "https://api.minimax.chat/v1",
            ),
            (
                "baichuan",
                "openai",
                "Baichuan",
                "https://api.baichuan-ai.com/v1",
            ),
            ("stepfun", "openai", "StepFun", "https://api.stepfun.com/v1"),
        ];
        for (i, (id, kind, name, url)) in builtins.iter().enumerate() {
            let env_key = format!("FERROCHAT_PROVIDER_{}_API_KEY", id.to_ascii_uppercase());
            let key = std::env::var(&env_key).unwrap_or_default();
            let enabled = if key.is_empty() { 0 } else { 1 };
            sqlx::query(
                "INSERT INTO providers (id, type, name, base_url, api_keys, enabled, sort_order, is_builtin)
                 VALUES (?, ?, ?, ?, ?, ?, ?, 1)
                 ON CONFLICT(id) DO UPDATE SET
                    api_keys = CASE WHEN providers.api_keys = '' AND excluded.api_keys != '' THEN excluded.api_keys ELSE providers.api_keys END,
                    enabled = CASE WHEN providers.api_keys = '' AND excluded.api_keys != '' THEN 1 ELSE providers.enabled END",
            )
            .bind(id)
            .bind(kind)
            .bind(name)
            .bind(url)
            .bind(&key)
            .bind(enabled)
            .bind(i as i64)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        }
        Ok(())
    }

    pub async fn list_providers(&self) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT id, type, name, base_url, api_keys, headers_json, enabled, sort_order, is_builtin FROM providers ORDER BY sort_order",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(provider_json).collect())
    }

    pub async fn provider(&self, id: &str) -> Result<Value, AppError> {
        let row = sqlx::query(
            "SELECT id, type, name, base_url, api_keys, headers_json, enabled, sort_order, is_builtin FROM providers WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound("provider not found".into()))?;
        Ok(provider_json(row))
    }

    pub async fn upsert_provider(&self, body: &Value) -> Result<Value, AppError> {
        let id = body
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let kind = body
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("openai");
        let name = body
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Custom");
        let base_url = body.get("base_url").and_then(|v| v.as_str()).unwrap_or("");
        let api_keys = body.get("api_keys").and_then(|v| v.as_str()).unwrap_or("");
        let headers = body.get("headers").cloned().unwrap_or_else(|| json!({}));
        let enabled = body
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let sort_order = body
            .get("sort_order")
            .and_then(|v| v.as_i64())
            .unwrap_or(100);
        sqlx::query(
            "INSERT INTO providers (id, type, name, base_url, api_keys, headers_json, enabled, sort_order, is_builtin)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0)
             ON CONFLICT(id) DO UPDATE SET type=excluded.type, name=excluded.name, base_url=excluded.base_url,
               api_keys=excluded.api_keys, headers_json=excluded.headers_json, enabled=excluded.enabled, sort_order=excluded.sort_order",
        )
        .bind(&id)
        .bind(kind)
        .bind(name)
        .bind(base_url)
        .bind(api_keys)
        .bind(headers.to_string())
        .bind(enabled as i64)
        .bind(sort_order)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        self.provider(&id).await
    }

    pub async fn delete_provider(&self, id: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM provider_models WHERE provider_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        sqlx::query("DELETE FROM providers WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(())
    }

    pub async fn list_provider_models(&self, provider_id: &str) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT id, provider_id, model_id, name, group_name, capabilities_json, params_json, enabled, sort_order FROM provider_models WHERE provider_id = ? ORDER BY sort_order, name",
        )
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows.into_iter().map(provider_model_json).collect())
    }

    pub async fn add_provider_model(
        &self,
        provider_id: &str,
        model_id: &str,
        name: &str,
        capabilities: Option<Value>,
    ) -> Result<Value, AppError> {
        let id = format!("{provider_id}:{model_id}");
        let caps = capabilities
            .unwrap_or_else(|| serde_json::to_value(infer_capabilities(model_id)).unwrap());
        let group = group_name(model_id);
        sqlx::query(
            "INSERT INTO provider_models (id, provider_id, model_id, name, group_name, capabilities_json, enabled, sort_order)
             VALUES (?, ?, ?, ?, ?, ?, 1, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM provider_models))
             ON CONFLICT(provider_id, model_id) DO UPDATE SET enabled=1",
        )
        .bind(&id)
        .bind(provider_id)
        .bind(model_id)
        .bind(if name.is_empty() { model_id } else { name })
        .bind(&group)
        .bind(caps.to_string())
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        self.update_provider_model(&id, &json!({})).await
    }

    pub async fn managed_models(&self) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT m.id, m.provider_id, m.model_id, m.name, m.group_name, m.capabilities_json, m.params_json, m.enabled, m.sort_order,
                    p.name AS provider_name, p.type AS provider_type, p.enabled AS provider_enabled
             FROM provider_models m JOIN providers p ON p.id = m.provider_id
             ORDER BY m.sort_order, p.sort_order, m.name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let provider_name: String = row.get("provider_name");
                let provider_type: String = row.get("provider_type");
                let provider_enabled: i64 = row.get("provider_enabled");
                let mut value = provider_model_json(row);
                value["provider_name"] = json!(provider_name);
                value["provider_type"] = json!(provider_type);
                value["provider_enabled"] = json!(provider_enabled != 0);
                value
            })
            .collect())
    }

    pub async fn reorder_models(&self, ids: &[String]) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(internal)?;
        for (i, id) in ids.iter().enumerate() {
            sqlx::query("UPDATE provider_models SET sort_order = ? WHERE id = ?")
                .bind(i as i64 + 1)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(internal)?;
        }
        tx.commit().await.map_err(internal)?;
        Ok(())
    }

    pub async fn update_provider_model(&self, id: &str, patch: &Value) -> Result<Value, AppError> {
        if let Some(name) = patch.get("name").and_then(|v| v.as_str()) {
            sqlx::query("UPDATE provider_models SET name = ? WHERE id = ?")
                .bind(name)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(internal)?;
        }
        if let Some(enabled) = patch.get("enabled").and_then(|v| v.as_bool()) {
            sqlx::query("UPDATE provider_models SET enabled = ? WHERE id = ?")
                .bind(enabled as i64)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(internal)?;
        }
        if let Some(caps) = patch.get("capabilities") {
            sqlx::query("UPDATE provider_models SET capabilities_json = ? WHERE id = ?")
                .bind(caps.to_string())
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(internal)?;
        }
        if let Some(params) = patch.get("params") {
            sqlx::query("UPDATE provider_models SET params_json = ? WHERE id = ?")
                .bind(params.to_string())
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(internal)?;
        }
        let row = sqlx::query(
            "SELECT id, provider_id, model_id, name, group_name, capabilities_json, params_json, enabled, sort_order FROM provider_models WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(internal)?
        .ok_or_else(|| AppError::NotFound("model not found".into()))?;
        Ok(provider_model_json(row))
    }

    pub async fn model_params(&self, provider_id: &str, model_id: &str) -> Result<Value, AppError> {
        let id = format!("{provider_id}:{model_id}");
        let raw: Option<String> =
            sqlx::query_scalar("SELECT params_json FROM provider_models WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?;
        Ok(raw
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(json!({})))
    }

    pub async fn provider_model_vision(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<bool, AppError> {
        let id = format!("{provider_id}:{model_id}");
        let raw: Option<String> =
            sqlx::query_scalar("SELECT capabilities_json FROM provider_models WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?;
        let Some(raw) = raw else {
            return Ok(true);
        };
        let caps: Value = serde_json::from_str(&raw).unwrap_or(json!({}));
        Ok(caps
            .get("vision")
            .and_then(|v| v.as_bool())
            .unwrap_or(false))
    }

    pub async fn provider_model_web(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<bool, AppError> {
        let id = format!("{provider_id}:{model_id}");
        let raw: Option<String> =
            sqlx::query_scalar("SELECT capabilities_json FROM provider_models WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(internal)?;
        let Some(raw) = raw else {
            return Ok(false);
        };
        let caps: Value = serde_json::from_str(&raw).unwrap_or(json!({}));
        Ok(caps.get("web").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn delete_provider_model(&self, id: &str) -> Result<(), AppError> {
        sqlx::query("DELETE FROM provider_models WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        Ok(())
    }

    pub async fn enabled_models(&self) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT m.id, m.provider_id, m.model_id, m.name, m.group_name, m.capabilities_json, p.name AS provider_name, p.type AS provider_type
             FROM provider_models m JOIN providers p ON p.id = m.provider_id
             WHERE m.enabled = 1 AND p.enabled = 1
             ORDER BY m.sort_order, p.sort_order, m.name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let caps: Value = serde_json::from_str(row.get("capabilities_json")).unwrap_or(json!({}));
                json!({
                    "id": row.get::<String, _>("id"),
                    "name": row.get::<String, _>("name"),
                    "object": "model",
                    "created": 0,
                    "owned_by": row.get::<String, _>("provider_type"),
                    "provider": {
                        "id": row.get::<String, _>("provider_id"),
                        "name": row.get::<String, _>("provider_name")
                    },
                    "tags": [],
                    "info": { "meta": { "capabilities": caps, "description": row.get::<String, _>("provider_name") } }
                })
            })
            .collect())
    }
}

pub struct UserRow {
    pub id: String,
    pub email: String,
    pub name: String,
    pub password_hash: String,
    pub role: String,
    pub profile_image_url: String,
}

fn map_user(row: sqlx::sqlite::SqliteRow) -> UserRow {
    UserRow {
        id: row.get("id"),
        email: row.get("email"),
        name: row.get("name"),
        password_hash: row.get("password_hash"),
        role: row.get("role"),
        profile_image_url: row.get("profile_image_url"),
    }
}

fn provider_json(row: sqlx::sqlite::SqliteRow) -> Value {
    let headers: Value = serde_json::from_str(row.get("headers_json")).unwrap_or(json!({}));
    json!({
        "id": row.get::<String, _>("id"),
        "type": row.get::<String, _>("type"),
        "name": row.get::<String, _>("name"),
        "base_url": row.get::<String, _>("base_url"),
        "api_keys": row.get::<String, _>("api_keys"),
        "headers": headers,
        "enabled": row.get::<i64, _>("enabled") != 0,
        "sort_order": row.get::<i64, _>("sort_order"),
        "is_builtin": row.get::<i64, _>("is_builtin") != 0,
    })
}

fn provider_model_json(row: sqlx::sqlite::SqliteRow) -> Value {
    let caps: Value = serde_json::from_str(row.get("capabilities_json")).unwrap_or(json!({}));
    json!({
        "id": row.get::<String, _>("id"),
        "provider_id": row.get::<String, _>("provider_id"),
        "model_id": row.get::<String, _>("model_id"),
        "name": row.get::<String, _>("name"),
        "group_name": row.get::<String, _>("group_name"),
        "capabilities": caps,
        "params": serde_json::from_str::<Value>(row.get("params_json")).unwrap_or(json!({})),
        "enabled": row.get::<i64, _>("enabled") != 0,
        "sort_order": row.get::<i64, _>("sort_order"),
    })
}

impl Db {
    async fn apply_later_migrations(&self) -> Result<(), AppError> {
        let has_params: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('provider_models') WHERE name = 'params_json'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        if has_params == 0 {
            let sql = include_str!("../migrations/002_models_share.sql");
            sqlx::raw_sql(sql)
                .execute(&self.pool)
                .await
                .map_err(internal)?;
        } else {
            sqlx::query(
                "CREATE INDEX IF NOT EXISTS chats_user_updated ON chats(user_id, updated_at)",
            )
            .execute(&self.pool)
            .await
            .map_err(internal)?;
        }
        let has_passages: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'file_passages'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        if has_passages == 0 {
            let sql = include_str!("../migrations/003_file_passages.sql");
            sqlx::raw_sql(sql)
                .execute(&self.pool)
                .await
                .map_err(internal)?;
        }
        let has_memories: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'memories'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        if has_memories == 0 {
            let sql = include_str!("../migrations/004_memories.sql");
            sqlx::raw_sql(sql)
                .execute(&self.pool)
                .await
                .map_err(internal)?;
        }
        let has_chat_fts: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'chats_fts'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        if has_chat_fts == 0 {
            let sql = include_str!("../migrations/005_chat_search.sql");
            sqlx::raw_sql(sql)
                .execute(&self.pool)
                .await
                .map_err(internal)?;
            self.reindex_chats().await?;
        }
        let has_shared_snapshot: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('chats') WHERE name = 'shared_chat_json'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(internal)?;
        if has_shared_snapshot == 0 {
            let mut tx = self.pool.begin().await.map_err(internal)?;
            sqlx::raw_sql(include_str!("../migrations/006_share_snapshot.sql"))
                .execute(&mut *tx)
                .await
                .map_err(internal)?;
            tx.commit().await.map_err(internal)?;
        }
        self.remove_builtin_presets().await?;
        Ok(())
    }

    async fn normalize_settings(&self) -> Result<(), AppError> {
        let rows = sqlx::query("SELECT id, settings_json FROM users")
            .fetch_all(&self.pool)
            .await
            .map_err(internal)?;
        for row in rows {
            let id: String = row.get("id");
            let raw: String = row.get("settings_json");
            let value: Value = serde_json::from_str(&raw).unwrap_or(json!({}));
            let flat = unwrap_settings(value);
            let encoded = flat.to_string();
            if encoded != raw {
                sqlx::query("UPDATE users SET settings_json = ? WHERE id = ?")
                    .bind(encoded)
                    .bind(id)
                    .execute(&self.pool)
                    .await
                    .map_err(internal)?;
            }
        }
        Ok(())
    }

    pub async fn cooled_fingerprints(&self, provider_id: &str) -> Result<Vec<String>, AppError> {
        let rows = sqlx::query(
            "SELECT key_fingerprint FROM provider_key_health WHERE provider_id = ? AND cooldown_until > ?",
        )
        .bind(provider_id)
        .bind(ferrochat_core::now())
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows
            .into_iter()
            .map(|r| r.get::<String, _>("key_fingerprint"))
            .collect())
    }

    pub async fn mark_key_error(
        &self,
        provider_id: &str,
        fingerprint: &str,
        error: &str,
        cooldown_secs: i64,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO provider_key_health (provider_id, key_fingerprint, last_error, cooldown_until)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(provider_id, key_fingerprint) DO UPDATE SET last_error = excluded.last_error, cooldown_until = excluded.cooldown_until",
        )
        .bind(provider_id)
        .bind(fingerprint)
        .bind(error)
        .bind(ferrochat_core::now() + cooldown_secs)
        .execute(&self.pool)
        .await
        .map_err(internal)?;
        Ok(())
    }

    pub async fn key_health(&self, provider_id: &str) -> Result<Vec<Value>, AppError> {
        let rows = sqlx::query(
            "SELECT key_fingerprint, last_error, cooldown_until FROM provider_key_health WHERE provider_id = ?",
        )
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await
        .map_err(internal)?;
        Ok(rows
            .into_iter()
            .map(|r| {
                json!({
                    "fingerprint": r.get::<String, _>("key_fingerprint"),
                    "last_error": r.get::<String, _>("last_error"),
                    "cooldown_until": r.get::<i64, _>("cooldown_until"),
                })
            })
            .collect())
    }
}

fn unwrap_settings(mut value: Value) -> Value {
    while value.get("ui").and_then(|ui| ui.get("ui")).is_some() {
        value = value.get("ui").cloned().unwrap_or(json!({}));
    }
    if value.get("ui").is_some() {
        value
    } else {
        json!({ "ui": value })
    }
}

fn internal(err: sqlx::Error) -> AppError {
    AppError::Internal(err.to_string())
}
