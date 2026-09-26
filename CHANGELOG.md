# Changelog

## 0.2.0 - 2026-09-26

- Archiving a chat now toggles: a second request restores it, matching the frontend unarchive action. Added the list endpoints the frontend already called: `GET /chats/archived`, `GET /chats/all`, `GET /chats/all/archived`, `POST /chats/tags` (filter by tag), and `POST /chats/archive/all`.
- Removing a tag from a chat deletes it. `DELETE /chats/{id}/tags` was a stub that returned an empty list and left the tag in place; it now removes the named tag and drops tags that no longer have any chat.
- Production builds strip `console.log`, `console.info`, and `console.debug` from the bundle; `warn` and `error` stay. Template-expression logging moved behind a DEV guard.
- CI runs on pushes to `main` and on pull requests instead of release tags only. The Playwright suite tagged `@chats` still runs only on `v*` tags, so pull requests do not need Node browser installs.
- The migration guard checks the whole migrations directory: the file set must be exactly 001–005, and every `*.sql.sha256` digest present is verified. Digests for 002–005 are committed.
- Fixed the "plain textarea" e2e flake at its source: the on-mount autofocus dispatched a trusted focusin, so the focus handler loaded the rich editor before `requestIdleCallback` could. Programmatic autofocus no longer triggers the load.
- Release builds cross-compile `aarch64-unknown-linux-musl` in a `messense/rust-musl-cross` container and `x86_64-unknown-linux-musl` with `musl-tools`; the checksum and upload steps use exact binary paths, and one platform failing no longer cancels the matrix.
- Web search (SearXNG by default, plus Tavily, Brave, Bing, Google, DuckDuckGo, and model-native search), document excerpts with page citations, and long-term memory.
- Browser speech input and optional OpenAI-compatible transcription and speech.
- Follow-up questions, direct text streaming for selection ask/explain, Markdown/JSON/PDF export, FTS chat search with a hit snippet, temporary chats, and quote-to-input from the selection toolbar.
- One Tools page (translate, polish, summarize) streams a model you already configured. Summarize can search the web. These are not models.
- Long chats can keep the last N messages and store a model-written summary for the rest. Provider usage is used when the API sends it; otherwise the count is labeled as an estimate. Per-model prices are USD per million tokens.
- After a reply, Ferrochat can suggest one memory. It is saved only after you confirm.
- Separate admin Models tab: enable/disable, rename, capabilities, default params, drag ordering, and a global default model.
- Providers tab fetches the full remote model list and adds the selected models in one batch.
- Built-in presets for Moonshot, Zhipu, DashScope, Volcengine Ark, MiniMax, Baichuan, and StepFun.
- Model default params are now merged into every request; values set in the chat still win.
- Fixed chat updates replacing the stored chat instead of merging, which reset generated titles.
- Removed the Connections settings tab, call mode, Valves, and knowledge/filter/action selectors.
- Restored translations for template-string i18n keys and redrew the PNG icons.
- Measured size: release binary 26.4 MB, idle RSS 6.5 MB, ten-stream peak 28.3 MB. Chat-page first-load JavaScript is 0.85 MB across 42 files; the largest chunk is 0.41 MB because the editor is lazy-loaded. The slim image was not built in this environment.

## 0.1.0

- Rust backend (axum, sqlx, SQLite) with the Open WebUI v0.6.5 chat frontend.
- Provider and model management, MCP tools, prompts, and workspace presets.
- Browser-language i18n until a language is chosen in settings.
- Chat sharing, tag generation, and image parts forwarded to vision models.
- Missing `/api` routes return JSON 404 instead of the HTML shell.
