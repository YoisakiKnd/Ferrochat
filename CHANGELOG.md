# Changelog

## Unreleased

- Separate admin Models tab: enable/disable, rename, capabilities, default params, drag ordering, and a global default model.
- Providers tab fetches the full remote model list and adds the selected models in one batch.
- Built-in presets for Moonshot, Zhipu, DashScope, Volcengine Ark, MiniMax, Baichuan, and StepFun.
- Model default params are now merged into every request; values set in the chat still win.
- Fixed chat updates replacing the stored chat instead of merging, which reset generated titles.
- Removed the Connections, Tools, Personalization, and Audio settings tabs, voice input, call mode, Valves, and knowledge/filter/action selectors.
- Restored translations for template-string i18n keys and redrew the PNG icons.

## 0.1.0

- Rust backend (axum, sqlx, SQLite) with the Open WebUI v0.6.5 chat frontend.
- Provider and model management, MCP tools, prompts, and workspace presets.
- Browser-language i18n until a language is chosen in settings.
- Chat sharing, tag generation, and image parts forwarded to vision models.
- Missing `/api` routes return JSON 404 instead of the HTML shell.
