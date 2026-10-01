use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

struct Server {
    child: Child,
    base: String,
    _dir: std::path::PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = std::fs::remove_dir_all(&self._dir);
    }
}

fn spawn() -> Server {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dir = std::env::temp_dir().join(format!("ferrochat-test-{port}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_ferrochat"))
        .env("FERROCHAT_DATA_DIR", &dir)
        .env("FERROCHAT_PORT", port.to_string())
        .env("FERROCHAT_HOST", "127.0.0.1")
        .env("FERROCHAT_SECRET_KEY", "test-secret")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::new();
    for _ in 0..50 {
        if client.get(format!("{base}/health")).send().is_ok() {
            return Server {
                child,
                base,
                _dir: dir,
            };
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("server did not start");
}

fn signup(base: &str) -> String {
    let client = reqwest::blocking::Client::new();
    let res = client
        .post(format!("{base}/api/v1/auths/signup"))
        .json(&serde_json::json!({"email":"ada@example.com","password":"secret1","name":"Ada"}))
        .send()
        .unwrap();
    assert!(res.status().is_success(), "{}", res.status());
    res.json::<serde_json::Value>().unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn concurrent_signup_creates_only_one_admin() {
    let server = spawn();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(5));
    let mut workers = Vec::new();
    for index in 0..4 {
        let base = server.base.clone();
        let barrier = barrier.clone();
        workers.push(thread::spawn(move || {
            let client = reqwest::blocking::Client::new();
            barrier.wait();
            client
                .post(format!("{base}/api/v1/auths/signup"))
                .json(&serde_json::json!({"email":format!("user{index}@example.com"),"password":"secret1","name":"Admin"}))
                .send()
                .unwrap()
                .status()
                .is_success()
        }));
    }
    barrier.wait();
    let successes = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(successes, 1);
    let config: serde_json::Value = reqwest::blocking::get(format!("{}/api/config", server.base))
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(config["features"]["enable_signup"], false);
}

#[test]
fn password_change_requires_current_password_and_uses_new_password() {
    let server = spawn();
    let token = signup(&server.base);
    let client = reqwest::blocking::Client::new();
    let endpoint = format!("{}/api/v1/auths/update/password", server.base);
    let wrong = client
        .post(&endpoint)
        .bearer_auth(&token)
        .json(&serde_json::json!({"password":"wrong","new_password":"secret2"}))
        .send()
        .unwrap();
    assert_eq!(wrong.status(), 400);
    let changed = client
        .post(&endpoint)
        .bearer_auth(&token)
        .json(&serde_json::json!({"password":"secret1","new_password":"secret2"}))
        .send()
        .unwrap();
    assert!(changed.status().is_success());

    let signin = |password: &str| {
        client
            .post(format!("{}/api/v1/auths/signin", server.base))
            .json(&serde_json::json!({"email":"ada@example.com","password":password}))
            .send()
            .unwrap()
            .status()
    };
    assert_eq!(signin("secret1"), 400);
    assert!(signin("secret2").is_success());
}

#[test]
fn upload_limit_matches_advertised_ten_megabytes() {
    let server = spawn();
    let token = signup(&server.base);
    let client = reqwest::blocking::Client::new();
    let config: serde_json::Value = client
        .get(format!("{}/api/config", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(config["file"]["max_size"], 10);
    let upload = |size: usize| {
        let boundary = "ferrochat-test-boundary";
        let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"note.txt\"\r\nContent-Type: text/plain\r\n\r\n").into_bytes();
        body.extend(std::iter::repeat_n(b'a', size));
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        client
            .post(format!("{}/api/v1/files/", server.base))
            .bearer_auth(&token)
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()
            .unwrap()
            .status()
    };
    assert!(upload(3 * 1024 * 1024).is_success());
    assert_eq!(upload(10 * 1024 * 1024 + 1), 400);
}

#[test]
fn transcription_rejects_oversized_upstream_response() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let upstream = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let _ = socket.read(&mut request);
        let body = vec![b'a'; 1024 * 1024 + 1];
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket.write_all(headers.as_bytes()).unwrap();
        let _ = socket.write_all(&body);
    });

    let server = spawn();
    let token = signup(&server.base);
    let client = reqwest::blocking::Client::new();
    let configured = client
        .post(format!("{}/api/v1/audio/config", server.base))
        .bearer_auth(&token)
        .json(
            &serde_json::json!({"stt_engine":"openai","url":format!("http://127.0.0.1:{port}/v1")}),
        )
        .send()
        .unwrap();
    assert!(configured.status().is_success());

    let boundary = "ferrochat-audio-test";
    let body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.webm\"\r\nContent-Type: audio/webm\r\n\r\nsmall\r\n--{boundary}--\r\n");
    let response = client
        .post(format!("{}/api/v1/audio/transcriptions", server.base))
        .bearer_auth(&token)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body)
        .send()
        .unwrap();
    assert_eq!(response.status(), 500);
    let detail: serde_json::Value = response.json().unwrap();
    assert!(detail["detail"].as_str().unwrap().contains("exceeds limit"));
    upstream.join().unwrap();
}

#[test]
fn socket_namespace_rejects_missing_token() {
    let server = spawn();
    let token = signup(&server.base);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let connect = |auth: &str| {
        let url = format!("{}/ws/socket.io/?EIO=4&transport=polling", server.base);
        let opening = client.get(&url).send().unwrap().text().unwrap();
        let payload: serde_json::Value = serde_json::from_str(&opening[1..]).unwrap();
        let sid = payload["sid"].as_str().unwrap();
        let poll = format!("{url}&sid={sid}");
        let posted = client.post(&poll).body(format!("40{auth}")).send().unwrap();
        assert!(posted.status().is_success(), "{}", posted.status());
        for _ in 0..3 {
            let packet = client.get(&poll).send().unwrap().text().unwrap();
            let mut result = None;
            for frame in packet.split('\u{1e}') {
                if frame == "2" {
                    client.post(&poll).body("3").send().unwrap();
                } else if !frame.is_empty() {
                    result.get_or_insert_with(|| frame.to_string());
                }
            }
            if let Some(result) = result {
                return result;
            }
        }
        panic!("socket did not finish namespace handshake")
    };
    assert!(!connect("{}").starts_with("40{"));
    let valid = connect(&serde_json::json!({"token": token}).to_string());
    assert!(valid.starts_with("40{"), "{valid}");
}

#[test]
fn auth_chats_and_unknown_api_are_json() {
    let server = spawn();
    let token = signup(&server.base);
    let client = reqwest::blocking::Client::new();
    let session = client
        .get(format!("{}/api/v1/auths/", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap();
    assert_eq!(session.status(), 200);
    assert!(session
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("json"));

    let chat = client
        .post(format!("{}/api/v1/chats/new", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"chat":{"title":"Hi","history":{"messages":{}}}}))
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    let id = chat["id"].as_str().unwrap();
    let updated = client
        .post(format!("{}/api/v1/chats/{id}", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"chat":{"messages":[]}}))
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert_eq!(updated["title"], "Hi");
    assert!(updated["chat"]["history"].is_object());
    let listed = client
        .get(format!("{}/api/v1/chats/", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert!(listed.as_array().unwrap().iter().any(|c| c["id"] == id));

    let shared = client
        .post(format!("{}/api/v1/chats/{id}/share", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    let share_id = shared["share_id"].as_str().unwrap();
    let public = client
        .get(format!("{}/api/v1/chats/share/{share_id}", server.base))
        .send()
        .unwrap();
    assert_eq!(public.status(), 200);
    let public = public.json::<serde_json::Value>().unwrap();
    assert_eq!(public["id"], share_id);
    assert!(public.get("user_id").is_none());
    assert!(public.get("folder_id").is_none());
    client
        .post(format!("{}/api/v1/chats/{id}", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"chat":{"title":"Private later update"}}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    let snapshot = || {
        client
            .get(format!("{}/api/v1/chats/share/{share_id}", server.base))
            .send()
            .unwrap()
    };
    assert_eq!(
        snapshot().json::<serde_json::Value>().unwrap()["title"],
        "Hi"
    );
    let reshared = client
        .post(format!("{}/api/v1/chats/{id}/share", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert_eq!(reshared["share_id"], share_id);
    assert_eq!(
        snapshot().json::<serde_json::Value>().unwrap()["title"],
        "Private later update"
    );
    client
        .delete(format!("{}/api/v1/chats/{id}/share", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(snapshot().status(), 404);
    for chat in [
        serde_json::json!(null),
        serde_json::json!("invalid"),
        serde_json::json!({"history":{"messages":[]}}),
    ] {
        let result = client
            .post(format!("{}/api/v1/chats/import", server.base))
            .bearer_auth(&token)
            .json(&serde_json::json!({"chat":chat}))
            .send()
            .unwrap();
        assert_eq!(result.status(), 400);
    }

    client
        .post(format!("{}/api/v1/folders/", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name":"Inbox"}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    client
        .post(format!("{}/api/v1/prompts/create", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"command":"sum","title":"Sum","content":"Summarize"}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();

    let missing = client
        .get(format!("{}/api/v1/does-not-exist", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap();
    assert_eq!(missing.status(), 404);
    let body = missing.text().unwrap();
    assert!(body.contains("not found"));
    assert!(!body.contains("<html"));
}

#[test]
fn page_load_routes_return_json() {
    let server = spawn();
    let token = signup(&server.base);
    let client = reqwest::blocking::Client::new();
    let paths = [
        "/api/config",
        "/api/models",
        "/api/v1/chats/",
        "/api/v1/chats/pinned",
        "/api/v1/chats/all/tags",
        "/api/v1/folders/",
        "/api/v1/prompts/list",
        "/api/v1/models/",
        "/api/v1/groups/",
        "/api/v1/tools/",
        "/api/v1/functions/",
        "/api/v1/knowledge/",
        "/api/v1/memories/",
        "/api/changelog",
        "/api/v1/users/user/settings",
        "/api/v1/auths/admin/config",
    ];
    for path in paths {
        let res = client
            .get(format!("{}{path}", server.base))
            .bearer_auth(&token)
            .send()
            .unwrap();
        assert_ne!(res.status(), 404, "{path}");
        let ctype = res
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ctype.contains("json"), "{path} {ctype}");
    }
}

#[test]
fn batch_add_keeps_custom_names_and_reorders() {
    let server = spawn();
    let token = signup(&server.base);
    let client = reqwest::blocking::Client::new();
    let post = |path: &str, body: serde_json::Value| {
        client
            .post(format!("{}{path}", server.base))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<serde_json::Value>()
            .unwrap()
    };
    post(
        "/api/v1/providers",
        serde_json::json!({"id":"m","type":"mock","name":"Mock","base_url":"","api_keys":"","enabled":true}),
    );
    let added = post(
        "/api/v1/providers/m/models/batch",
        serde_json::json!({"models":[{"model_id":"a"},{"model_id":"b"},{"model_id":"c"}]}),
    );
    assert_eq!(added.as_array().unwrap().len(), 3);
    post(
        "/api/v1/providers/m/models/a",
        serde_json::json!({"name":"Alpha"}),
    );
    post(
        "/api/v1/providers/m/models/batch",
        serde_json::json!({"models":[{"model_id":"a"}]}),
    );
    post(
        "/api/v1/models/managed/reorder",
        serde_json::json!({"ids":["m:c","m:a","m:b"]}),
    );
    post(
        "/api/v1/models/managed/default",
        serde_json::json!({"default_model":"m:c"}),
    );
    let managed = client
        .get(format!("{}/api/v1/models/managed", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    let ids: Vec<&str> = managed["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["m:c", "m:a", "m:b"]);
    assert_eq!(managed["models"][1]["name"], "Alpha");
    assert_eq!(managed["default_model"], "m:c");
    let public = client
        .get(format!("{}/api/models", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert_eq!(public["data"][0]["id"], "m:c");
}

#[test]
fn openai_compatible_stream_receives_image_part() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        let mut buf = [0u8; 8192];
        let n = sock.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        assert!(req.contains("image_url"), "{req}");
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = sock.write_all(resp.as_bytes());
    });

    let server = spawn();
    let token = signup(&server.base);
    let client = reqwest::blocking::Client::new();
    let providers = client
        .get(format!("{}/api/v1/providers", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<Vec<serde_json::Value>>()
        .unwrap();
    let mut openai = providers.into_iter().find(|p| p["id"] == "openai").unwrap();
    openai["enabled"] = serde_json::json!(true);
    openai["api_keys"] = serde_json::json!("sk-test");
    openai["base_url"] = serde_json::json!(format!("http://127.0.0.1:{port}/v1"));
    client
        .post(format!("{}/api/v1/providers", server.base))
        .bearer_auth(&token)
        .json(&openai)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    client
        .post(format!("{}/api/v1/providers/openai/models", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"model_id":"gpt-4o","name":"GPT-4o","capabilities":{"vision":true,"reasoning":false,"tools":false,"web":false,"embedding":false}}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    let res = client
        .post(format!("{}/api/chat/completions", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "model": "openai:gpt-4o",
            "messages": [{"role":"user","content":"see this"}],
            "files": [{"type":"image","url":"data:image/png;base64,aaaa"}]
        }))
        .send()
        .unwrap();
    assert!(
        res.status().is_success(),
        "{}",
        res.text().unwrap_or_default()
    );
}

#[test]
fn mock_usage_search_and_no_builtin_presets() {
    let server = spawn();
    let token = signup(&server.base);
    let client = reqwest::blocking::Client::new();
    client
        .post(format!("{}/api/v1/providers", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"id":"mock","type":"mock","name":"Mock","base_url":"","api_keys":"","enabled":true}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    client
        .post(format!("{}/api/v1/providers/mock/models", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"model_id":"mock-model","name":"Mock"}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    let started = client
        .post(format!("{}/api/chat/completions", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "stream": true,
            "chat_id": "usage-probe",
            "model": "mock:mock-model",
            "messages": [{"role":"user","content":"hello uniquephrasezeta"}]
        }))
        .send()
        .unwrap();
    assert!(started.status().is_success());
    thread::sleep(Duration::from_millis(500));
    let usage = client
        .get(format!("{}/api/v1/usage", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert!(usage
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["model"] == "mock:mock-model"
            && row["completion_tokens"].as_i64().unwrap_or(0) > 0));
    let mut cleared = false;
    for _ in 0..20 {
        let tasks: serde_json::Value = client
            .get(format!("{}/api/tasks/chat/usage-probe", server.base))
            .bearer_auth(&token)
            .send()
            .unwrap()
            .json()
            .unwrap();
        if tasks["task_ids"]
            .as_array()
            .is_some_and(|items| items.is_empty())
        {
            cleared = true;
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    assert!(
        cleared,
        "finished tasks should be removed from the chat index"
    );

    let chat = client
        .post(format!("{}/api/v1/chats/new", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"chat":{"title":"notes","history":{"messages":{"a":{"role":"user","content":"uniquephrasezeta"}}}}}))
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    let found = client
        .get(format!(
            "{}/api/v1/chats/search?text=uniquephrasezeta",
            server.base
        ))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert!(found
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["id"] == chat["id"]));

    let models = client
        .get(format!("{}/api/models", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    let names = models["data"].to_string();
    assert!(!names.contains("preset-translate"));
    assert!(found.as_array().unwrap().iter().any(|row| row["snippet"]
        .as_str()
        .unwrap_or("")
        .contains("uniquephrasezeta")
        || row["title"].as_str().unwrap_or("").contains("notes")));
}

#[test]
fn tool_stream_prices_import_and_memory() {
    let server = spawn();
    let token = signup(&server.base);
    let client = reqwest::blocking::Client::new();
    client
        .post(format!("{}/api/v1/providers", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"id":"mock","type":"mock","name":"Mock","base_url":"","api_keys":"","enabled":true}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    client
        .post(format!("{}/api/v1/providers/mock/models", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"model_id":"mock-model","name":"Mock"}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    client
        .post(format!(
            "{}/api/v1/providers/mock/models/mock-model",
            server.base
        ))
        .bearer_auth(&token)
        .json(&serde_json::json!({"params":{"input_price":2.0,"output_price":4.0}}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();

    let tool = client
        .post(format!("{}/api/chat/completions", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "stream": true,
            "model": "mock:mock-model",
            "messages": [{"role":"user","content":"translate this"}],
            "features": {"web_search": true}
        }))
        .send()
        .unwrap();
    assert!(tool.status().is_success(), "{}", tool.status());
    let body = tool.text().unwrap();
    assert!(body.contains("Hello"), "{body}");
    assert!(body.contains("[DONE]"), "{body}");

    let started = client
        .post(format!("{}/api/chat/completions", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "stream": true,
            "chat_id": "price-probe",
            "model": "mock:mock-model",
            "messages": [{"role":"user","content":"price me"}]
        }))
        .send()
        .unwrap();
    assert!(started.status().is_success());
    thread::sleep(Duration::from_millis(500));
    let usage = client
        .get(format!("{}/api/v1/usage", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert!(usage.as_array().unwrap().iter().any(|row| {
        row["model"] == "mock:mock-model" && row["cost"].as_f64().unwrap_or(0.0) > 0.0
    }));

    let imported = client
        .post(format!("{}/api/v1/chats/import", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"chat":{"title":"imported note","history":{"messages":{"a":{"role":"user","content":"brought in"}}}}}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    let loaded = client
        .get(format!(
            "{}/api/v1/chats/{}",
            server.base,
            imported["id"].as_str().unwrap()
        ))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert_eq!(loaded["chat"]["title"], "imported note");

    client
        .post(format!("{}/api/v1/memories/add", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"content":"Ada likes rust"}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    let memories = client
        .get(format!("{}/api/v1/memories/", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .text()
        .unwrap();
    assert!(memories.contains("Ada likes rust"), "{memories}");
}

#[test]
fn archive_toggles_and_lists_stay_consistent() {
    let server = spawn();
    let token = signup(&server.base);
    let client = reqwest::blocking::Client::new();
    let mut create = |title: &str| -> String {
        client
            .post(format!("{}/api/v1/chats/new", server.base))
            .bearer_auth(&token)
            .json(&serde_json::json!({"chat":{"title":title,"history":{"messages":{}}}}))
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<serde_json::Value>()
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let first = create("archive-me");
    let second = create("keep-visible");
    let ids = |arr: &serde_json::Value| -> Vec<String> {
        arr.as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap_or_default().to_string())
            .collect()
    };
    let list_ids = |path: &str| -> Vec<String> {
        ids(&client
            .get(format!("{}{path}", server.base))
            .bearer_auth(&token)
            .send()
            .unwrap()
            .json::<serde_json::Value>()
            .unwrap())
    };

    let archived = client
        .post(format!("{}/api/v1/chats/{first}/archive", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert_eq!(archived["archived"], true);

    assert_eq!(list_ids("/api/v1/chats/"), vec![second.clone()]);
    assert_eq!(list_ids("/api/v1/chats/archived"), vec![first.clone()]);
    let all_archived = client
        .get(format!("{}/api/v1/chats/all/archived", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert_eq!(ids(&all_archived), vec![first.clone()]);
    assert_eq!(all_archived[0]["chat"]["title"], "archive-me");
    assert_eq!(list_ids("/api/v1/chats/all"), vec![second.clone()]);

    let unarchived = client
        .post(format!("{}/api/v1/chats/{first}/archive", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert_eq!(unarchived["archived"], false);
    assert!(list_ids("/api/v1/chats/archived").is_empty());
    assert_eq!(list_ids("/api/v1/chats/").len(), 2);

    let status = client
        .post(format!("{}/api/v1/chats/archive/all", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert_eq!(status["status"], true);
    assert_eq!(list_ids("/api/v1/chats/archived").len(), 2);
    client
        .post(format!("{}/api/v1/chats/{first}/archive", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap();
    assert_eq!(list_ids("/api/v1/chats/archived"), vec![second.clone()]);
}

#[test]
fn tags_add_filter_and_remove_roundtrip() {
    let server = spawn();
    let token = signup(&server.base);
    let client = reqwest::blocking::Client::new();
    let chat = client
        .post(format!("{}/api/v1/chats/new", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"chat":{"title":"tagged","history":{"messages":{}}}}))
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    let id = chat["id"].as_str().unwrap().to_string();

    let after_add = client
        .post(format!("{}/api/v1/chats/{id}/tags", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name":"work"}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert_eq!(after_add.as_array().unwrap().len(), 1);
    assert_eq!(after_add[0]["name"], "work");

    let filtered = client
        .post(format!("{}/api/v1/chats/tags", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name":"work"}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert_eq!(filtered.as_array().unwrap()[0]["id"], id.as_str());

    let all_tags = client
        .get(format!("{}/api/v1/chats/all/tags", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert_eq!(all_tags.as_array().unwrap().len(), 1);

    let after_delete = client
        .delete(format!("{}/api/v1/chats/{id}/tags", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name":"work"}))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert!(after_delete.as_array().unwrap().is_empty());

    let filtered = client
        .post(format!("{}/api/v1/chats/tags", server.base))
        .bearer_auth(&token)
        .json(&serde_json::json!({"name":"work"}))
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert!(filtered.as_array().unwrap().is_empty());
    let all_tags = client
        .get(format!("{}/api/v1/chats/all/tags", server.base))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .json::<serde_json::Value>()
        .unwrap();
    assert!(all_tags.as_array().unwrap().is_empty());
}
