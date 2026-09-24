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

The compose file pulls `ghcr.io/yoisakiknd/ferrochat`. Set `FERROCHAT_TAG=0.1` to pin a release; the default is `latest`.

Images and CI run only when a `v*` tag is pushed. `v1.2.3` publishes `1.2.3`, `1.2`, and `latest` for `linux/amd64` and `linux/arm64`. Pushes to `main` do not build or test.

To build locally:

```bash
docker build -f docker/Dockerfile --target slim -t ferrochat .
docker run -p 3000:8080 -v ferrochat-data:/data ferrochat
```

`slim` is the published image. `full` adds Node.js and `uv` so stdio MCP servers that launch through `npx` or `uvx` work inside the container.

Set `FERROCHAT_PROVIDER_OPENAI_API_KEY` (or `DEEPSEEK`, `ANTHROPIC`, `GEMINI`, `GROQ`, `OPENROUTER`, `SILICONFLOW`, `OLLAMA`, `AZURE`) to enable a built-in provider on first start.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `FERROCHAT_HOST` | `0.0.0.0` | Bind address |
| `FERROCHAT_PORT` | `8080` | HTTP port |
| `FERROCHAT_DATA_DIR` | `./data` | SQLite database, uploads, and `secret.key` |
| `FERROCHAT_SECRET_KEY` | generated | JWT signing key. Set this when running more than one process |
| `FERROCHAT_FRONTEND_DIR` | embedded build | Override the static frontend |

## Upgrade and backup

Copy the data directory before upgrading. The SQLite file is `ferrochat.db` inside that directory. Schema changes ship as new files under `backend/crates/db/migrations`. Do not edit a migration that has already been applied; add the next numbered file instead.

Releases are semver tags (`v0.1.0`). See `CHANGELOG.md`. GitHub Release assets include a `.sha256` file next to each binary.

## What it keeps

Chat, folders, prompts, workspace model presets, and MCP tools. Provider and model setup follows the Cherry Studio layout: a provider list, keys, a connectivity check, and models grouped by series.

## What it drops

Knowledge bases, RAG, voice, image generation, channels, notes, pipelines, Python functions, OAuth, and the multi-user admin panel.
