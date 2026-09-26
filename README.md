# Ferrochat

Self-hosted chat UI for OpenAI-compatible, Anthropic, Gemini, and Ollama providers. Ferrochat keeps the chat workspace from Open WebUI v0.6.5 and replaces the Python backend with a single Rust binary.

Ferrochat is a derivative of [Open WebUI v0.6.5](https://github.com/open-webui/open-webui/tree/v0.6.5) (BSD-3-Clause). See `LICENSE` and `NOTICE`. Do not merge Open WebUI v0.6.6 or later into this tree.

## Run

```bash
cd frontend && npm ci && npm run build
cd ../backend && FERROCHAT_FRONTEND_DIR=../frontend/build cargo run --bin ferrochat
```

Open http://127.0.0.1:8080. The first account you create is the only account.

## Docker

```bash
docker compose up -d
```

The compose file pulls `ghcr.io/yoisakiknd/ferrochat`. Set `FERROCHAT_TAG=0.2` to pin a release; the default is `latest`.

CI checks (Rust format and tests, frontend build, size report, type-error baseline, and the smoke Playwright suite) run on pushes to `main` and on pull requests. Published images, GitHub Release assets, the `@chats` Playwright suite, and multi-arch manifests run only when a `v*` tag is pushed. `v1.2.3` publishes `1.2.3`, `1.2`, and `latest` for `linux/amd64` and `linux/arm64`.

To build locally:

```bash
docker build -f docker/Dockerfile --target slim -t ferrochat .
docker run -p 3000:8080 -v ferrochat-data:/data ferrochat
```

`slim` is the published image. `full` adds Node.js and `uv` so stdio MCP servers that launch through `npx` or `uvx` work inside the container.

Set `FERROCHAT_PROVIDER_OPENAI_API_KEY` (or `DEEPSEEK`, `ANTHROPIC`, `GEMINI`, `GROQ`, `OPENROUTER`, `SILICONFLOW`, `OLLAMA`, `AZURE`) to enable a built-in provider on first start.

Web search is off until an admin saves an engine under Settings, Web Search. SearXNG is the recommended engine:

```bash
docker compose -f docker-compose.searxng.yml up -d
```

Point Ferrochat at `http://127.0.0.1:8088`. Models with the `web` capability can use the provider’s own search instead; if that model cannot search, Ferrochat falls back to the configured engine.

Voice input uses the browser speech recognizer. Admins can switch transcription and speech to an OpenAI-compatible endpoint under Settings, Audio. Sidebar search matches message text and shows a short hit under the chat title.

The sidebar Tools page translates, polishes, or summarizes text with a model you already enabled. It does not create a chat. Summarize can search the web when an engine is configured. Long chats can keep only the last N messages (Settings, General) and reuse a stored summary of the older ones. Set input and output price per million tokens on Admin, Models to show a cost. When a provider does not report usage, the token line is marked as an estimate. Memory suggestions appear under a reply and are saved only after you confirm. A temporary chat is not written to the database.

On this machine the release binary is 26.4 MB, idle RSS is 6.5 MB, and ten simultaneous mock replies peaked at 28.3 MB. The chat page loads about 0.85 MB of JavaScript across 42 files; the largest chunk is 0.41 MB because the rich-text editor is lazy-loaded. See `docs/metrics.md`. The slim image was not built here because Docker is not installed.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `FERROCHAT_HOST` | `0.0.0.0` | Bind address |
| `FERROCHAT_PORT` | `8080` | HTTP port |
| `FERROCHAT_DATA_DIR` | `./data` | SQLite database, uploads, and `secret.key` |
| `FERROCHAT_SECRET_KEY` | generated | JWT signing key. Set this when running more than one process |
| `FERROCHAT_FRONTEND_DIR` | embedded build | Override the static frontend |

By default the server binds `0.0.0.0` and the API accepts any origin; sessions are bearer tokens kept in browser storage. That is fine on localhost, but expose it publicly only behind a reverse proxy with TLS and authentication, or set `FERROCHAT_HOST=127.0.0.1`.

## Upgrade and backup

Copy the data directory before upgrading. The SQLite file is `ferrochat.db` inside that directory. Schema changes ship as new files under `backend/crates/db/migrations`. Do not edit a migration that has already been applied; add the next numbered file instead.

Releases are semver tags (`v0.2.1`). See `CHANGELOG.md`. GitHub Release assets include a `.sha256` file next to each binary.

## What it keeps

Chat, folders, prompts, workspace model presets, and MCP tools. Provider and model setup follows the Cherry Studio layout: a provider list, keys, a connectivity check, and models grouped by series.

## What it drops

Knowledge bases, RAG, voice, image generation, channels, notes, pipelines, Python functions, OAuth, and the multi-user admin panel.
