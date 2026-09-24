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
