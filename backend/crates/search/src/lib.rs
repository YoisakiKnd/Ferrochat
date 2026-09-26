use ferrochat_core::AppError;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct Query {
    pub engine: String,
    pub url: String,
    pub api_key: String,
    pub cx: String,
    pub count: usize,
    pub text: String,
}

pub fn join_base(raw: &str, suffix: &str) -> String {
    let mut base = raw.trim().trim_end_matches('/').to_string();
    if let Some(index) = base.find('?') {
        base.truncate(index);
        base = base.trim_end_matches('/').to_string();
    }
    for tail in [
        suffix,
        "/search",
        "/v7.0/search",
        "/res/v1/web/search",
        "/customsearch/v1",
    ] {
        if !tail.is_empty() && base.ends_with(tail) {
            base.truncate(base.len() - tail.len());
            base = base.trim_end_matches('/').to_string();
        }
    }
    if suffix.is_empty() {
        base
    } else {
        format!("{base}{suffix}")
    }
}

pub async fn search(query: &Query) -> Result<Vec<Hit>, AppError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("Mozilla/5.0 (compatible; Ferrochat)")
        .build()
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let count = query.count.clamp(1, 8);
    match query.engine.as_str() {
        "tavily" => tavily(&client, query, count).await,
        "brave" => brave(&client, query, count).await,
        "bing" => bing(&client, query, count).await,
        "google" => google(&client, query, count).await,
        "duckduckgo" => duckduckgo(&client, query, count).await,
        _ => searxng(&client, query, count).await,
    }
}

pub async fn fetch_text(url: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("Ferrochat/0.2")
        .build()
        .ok()?;
    let res = client.get(url).send().await.ok()?;
    if !res.status().is_success() {
        return None;
    }
    let bytes = res.bytes().await.ok()?;
    let slice = &bytes[..bytes.len().min(200_000)];
    let plain = html_to_text(&String::from_utf8_lossy(slice));
    let plain: String = plain.chars().take(4000).collect();
    if plain.trim().is_empty() {
        None
    } else {
        Some(plain)
    }
}

pub fn html_to_text(html: &str) -> String {
    let mut s = html.to_string();
    for tag in ["script", "style"] {
        loop {
            let lower = s.to_ascii_lowercase();
            let Some(start) = lower.find(&format!("<{tag}")) else {
                break;
            };
            let close = format!("</{tag}");
            let Some(end_rel) = lower[start..].find(&close) else {
                s.truncate(start);
                break;
            };
            let after = lower[start + end_rel..]
                .find('>')
                .map(|i| start + end_rel + i + 1)
                .unwrap_or(s.len());
            s.replace_range(start..after, " ");
        }
    }
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' {
            in_tag = true;
            continue;
        }
        if c == '>' {
            in_tag = false;
            out.push(' ');
            continue;
        }
        if !in_tag {
            out.push(c);
        }
    }
    decode_entities(&collapse_ws(&out))
}

fn collapse_ws(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

async fn searxng(
    client: &reqwest::Client,
    query: &Query,
    count: usize,
) -> Result<Vec<Hit>, AppError> {
    let base = join_base(&query.url, "");
    if base.is_empty() {
        return Err(AppError::BadRequest("SearXNG URL is required".into()));
    }
    let res = client
        .get(join_base(&base, "/search"))
        .header("Accept", "application/json")
        .query(&[("q", query.text.as_str()), ("format", "json")])
        .send()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    let status = res.status();
    let body: Value = res
        .json()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    if !status.is_success() {
        return Err(AppError::BadRequest(body.to_string()));
    }
    let mut hits = hits_from(&body, &["results"], "title", "url", "content", count);
    if hits.is_empty() {
        hits = infobox_hits(&body, count);
    }
    if hits.is_empty() {
        if let Some(reason) = searxng_failure(&body) {
            return Err(AppError::BadRequest(reason));
        }
    }
    Ok(hits)
}

fn infobox_hits(body: &Value, count: usize) -> Vec<Hit> {
    body.get("infoboxes")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let title = item
                        .get("infobox")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .or_else(|| item.get("title").and_then(|v| v.as_str()))
                        .unwrap_or("")
                        .to_string();
                    let url = item
                        .get("urls")
                        .and_then(|v| v.as_array())
                        .and_then(|urls| urls.first())
                        .and_then(|u| u.get("url"))
                        .and_then(|v| v.as_str())
                        .or_else(|| item.get("id").and_then(|v| v.as_str()))
                        .unwrap_or("")
                        .to_string();
                    let snippet = item
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if title.is_empty() || url.is_empty() {
                        None
                    } else {
                        Some(Hit {
                            title,
                            url,
                            snippet,
                        })
                    }
                })
                .take(count)
                .collect()
        })
        .unwrap_or_default()
}

fn searxng_failure(body: &Value) -> Option<String> {
    let engines = body.get("unresponsive_engines")?.as_array()?;
    if engines.is_empty() {
        return None;
    }
    let notes: Vec<String> = engines
        .iter()
        .filter_map(|item| {
            let pair = item.as_array()?;
            let name = pair.first()?.as_str()?;
            let why = pair.get(1)?.as_str().unwrap_or("");
            Some(format!("{name}: {why}"))
        })
        .collect();
    if notes.is_empty() {
        None
    } else {
        Some(format!(
            "SearXNG returned no results ({})",
            notes.join("; ")
        ))
    }
}

async fn tavily(
    client: &reqwest::Client,
    query: &Query,
    count: usize,
) -> Result<Vec<Hit>, AppError> {
    if query.api_key.is_empty() {
        return Err(AppError::BadRequest("Tavily API key is required".into()));
    }
    let res = client
        .post(join_base(
            if query.url.trim().is_empty() {
                "https://api.tavily.com"
            } else {
                query.url.as_str()
            },
            "/search",
        ))
        .json(&serde_json::json!({
            "api_key": query.api_key,
            "query": query.text,
            "max_results": count
        }))
        .send()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    let body: Value = res
        .json()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    Ok(hits_from(
        &body,
        &["results"],
        "title",
        "url",
        "content",
        count,
    ))
}

async fn brave(
    client: &reqwest::Client,
    query: &Query,
    count: usize,
) -> Result<Vec<Hit>, AppError> {
    if query.api_key.is_empty() {
        return Err(AppError::BadRequest("Brave API key is required".into()));
    }
    let res = client
        .get(join_base(
            if query.url.trim().is_empty() {
                "https://api.search.brave.com"
            } else {
                query.url.as_str()
            },
            "/res/v1/web/search",
        ))
        .header("X-Subscription-Token", &query.api_key)
        .query(&[("q", query.text.as_str()), ("count", &count.to_string())])
        .send()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    let body: Value = res
        .json()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    Ok(hits_from(
        &body,
        &["web", "results"],
        "title",
        "url",
        "description",
        count,
    ))
}

async fn bing(client: &reqwest::Client, query: &Query, count: usize) -> Result<Vec<Hit>, AppError> {
    if query.api_key.is_empty() {
        return Err(AppError::BadRequest("Bing API key is required".into()));
    }
    let res = client
        .get(join_base(
            if query.url.trim().is_empty() {
                "https://api.bing.microsoft.com"
            } else {
                query.url.as_str()
            },
            "/v7.0/search",
        ))
        .header("Ocp-Apim-Subscription-Key", &query.api_key)
        .query(&[("q", query.text.as_str()), ("count", &count.to_string())])
        .send()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    let body: Value = res
        .json()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    Ok(hits_from(
        &body,
        &["webPages", "value"],
        "name",
        "url",
        "snippet",
        count,
    ))
}

async fn google(
    client: &reqwest::Client,
    query: &Query,
    count: usize,
) -> Result<Vec<Hit>, AppError> {
    if query.api_key.is_empty() || query.cx.is_empty() {
        return Err(AppError::BadRequest(
            "Google API key and search engine id are required".into(),
        ));
    }
    let res = client
        .get(join_base(
            if query.url.trim().is_empty() {
                "https://www.googleapis.com"
            } else {
                query.url.as_str()
            },
            "/customsearch/v1",
        ))
        .query(&[
            ("key", query.api_key.as_str()),
            ("cx", query.cx.as_str()),
            ("q", query.text.as_str()),
            ("num", &count.to_string()),
        ])
        .send()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    let body: Value = res
        .json()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    Ok(hits_from(
        &body,
        &["items"],
        "title",
        "link",
        "snippet",
        count,
    ))
}

async fn duckduckgo(
    client: &reqwest::Client,
    query: &Query,
    count: usize,
) -> Result<Vec<Hit>, AppError> {
    let res = client
        .get(join_base(
            if query.url.trim().is_empty() {
                "https://html.duckduckgo.com"
            } else {
                query.url.as_str()
            },
            "/html/",
        ))
        .query(&[("q", query.text.as_str())])
        .send()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    let html = res
        .text()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    Ok(parse_duckduckgo(&html, count))
}

fn hits_from(
    body: &Value,
    path: &[&str],
    title: &str,
    url: &str,
    snippet: &str,
    count: usize,
) -> Vec<Hit> {
    let mut cursor = body;
    for key in path {
        cursor = match cursor.get(*key) {
            Some(next) => next,
            None => return Vec::new(),
        };
    }
    cursor
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let url = item.get(url)?.as_str()?.to_string();
                    if !url.starts_with("http") {
                        return None;
                    }
                    Some(Hit {
                        title: item
                            .get(title)
                            .and_then(|v| v.as_str())
                            .unwrap_or(&url)
                            .to_string(),
                        url,
                        snippet: item
                            .get(snippet)
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .take(count)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_duckduckgo(html: &str, count: usize) -> Vec<Hit> {
    let mut hits = Vec::new();
    for part in html.split("class=\"result__a\"").skip(1) {
        if hits.len() >= count {
            break;
        }
        let Some(href) = attr(part, "href") else {
            continue;
        };
        let url = unescape_ddg(&href);
        if !url.starts_with("http") {
            continue;
        }
        let title = html_to_text(part.split("</a>").next().unwrap_or(""));
        hits.push(Hit {
            title: if title.is_empty() { url.clone() } else { title },
            url,
            snippet: String::new(),
        });
    }
    hits
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let rest = tag.split(&needle).nth(1)?;
    Some(rest.split('"').next()?.to_string())
}

fn unescape_ddg(url: &str) -> String {
    if let Some(query) = url.split("uddg=").nth(1) {
        let encoded = query.split('&').next().unwrap_or(query);
        return urlencoding_decode(encoded);
    }
    url.to_string()
}

fn urlencoding_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn configured(value: &Value) -> bool {
    let engine = value.get("engine").and_then(|v| v.as_str()).unwrap_or("");
    match engine {
        "searxng" => value
            .get("url")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty()),
        "duckduckgo" => true,
        "google" => {
            !value
                .get("api_key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .is_empty()
                && !value
                    .get("cx")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .is_empty()
        }
        "tavily" | "brave" | "bing" => !value
            .get("api_key")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .is_empty(),
        "native" => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_drops_a_pasted_path() {
        assert_eq!(
            join_base("https://searx.example.com/search?q=1", "/search"),
            "https://searx.example.com/search"
        );
        assert_eq!(
            join_base("https://searx.example.com", "/search"),
            "https://searx.example.com/search"
        );
    }

    #[test]
    fn strips_html_and_scripts() {
        let text = html_to_text("<style>x{}</style><p>Hello &amp; <b>world</b></p>");
        assert_eq!(text, "Hello & world");
    }

    #[tokio::test]
    async fn searxng_parses_local_json() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let body =
                r#"{"results":[{"title":"Ferro","url":"https://example.com","content":"chat"}]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            use tokio::io::AsyncWriteExt;
            let _ = stream.write_all(resp.as_bytes()).await;
        });
        let hits = search(&Query {
            engine: "searxng".into(),
            url: format!("http://{addr}"),
            api_key: String::new(),
            cx: String::new(),
            count: 3,
            text: "ferrochat".into(),
        })
        .await
        .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://example.com");
    }
}
