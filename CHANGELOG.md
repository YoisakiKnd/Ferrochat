# Changelog

## Unreleased

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
- Measured size: release binary 26.4 MB, idle RSS 6.5 MB, ten-stream peak 28.3 MB. Chat-page JavaScript is about 3.6 MB. The slim image was not built in this environment.

## 0.1.0

- Rust backend (axum, sqlx, SQLite) with the Open WebUI v0.6.5 chat frontend.
- Provider and model management, MCP tools, prompts, and workspace presets.
- Browser-language i18n until a language is chosen in settings.
- Chat sharing, tag generation, and image parts forwarded to vision models.
- Missing `/api` routes return JSON 404 instead of the HTML shell.
