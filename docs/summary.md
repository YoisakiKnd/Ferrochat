# Ferrochat 项目解析

写于 2026-09-25，2026-09-26 补了归档/标签接口、console 剥离、CI 触发和 0.2.0 版本号这一轮。工作区里的功能还没有提交，也没有打标签。本文按当前代码说明这个仓库是什么、进程怎么起来、一次对话在后端和前端各走哪几步、数据落在哪张表、页面和接口哪些是真的、哪些只是为了不让旧前端报错。

## 1. 定位

Ferrochat 是单机自托管的聊天程序。网页来自 Open WebUI v0.6.5，后端换成一个 Rust 二进制。许可 BSD-3-Clause，见仓库根目录的 `LICENSE` 和 `NOTICE`。前端底子停在 v0.6.5，不要把 Open WebUI v0.6.6 及以后的提交合并进来。

它不是多租户服务。库里还没有用户时注册页开着，第一个人成为管理员，之后 `/api/config` 把 `enable_signup` 设成 false。没有 OAuth、LDAP、API Key 登录、多用户管理面板。

一个进程同时做三件事：用 axum 提供 HTTP、用 socket.io 推流、用 rust-embed 或磁盘目录提供静态网页。默认监听 `0.0.0.0:8080`。数据目录里有 `ferrochat.db`、`uploads/` 和 `secret.key`。

明确不做、并在配置里写死关闭的：知识库与 RAG 管道、绘图、频道、笔记、Python 函数、代码解释器、社区分享、消息评分、用户 webhook、Google Drive、OneDrive。聊天里如果还看得到这些入口，多半是旧组件没删干净，后端对应列表是空的。

## 2. 仓库

```text
llm-web/
  frontend/                 SvelteKit 2、Svelte 4、Vite 5、TypeScript
  backend/                  Cargo workspace
    crates/core             配置、错误、模型能力
    crates/db               SQLite
    crates/providers        各家模型的 HTTP 流
    crates/mcp              MCP 客户端
    crates/search           网页搜索
    crates/server           二进制 ferrochat
  docker/Dockerfile         web → rust 构建 → slim / full
  docker-compose.searxng.yml
  docs/metrics.md           体积和内存
  docs/summary.md           本文
  scripts/measure.sh
  .github/workflows         ci：main 推送和 PR 触发；release 和镜像仅 v* 标签触发
```

Workspace 版本 0.1.0，edition 2021。主要依赖：axum 0.8、sqlx 0.8（只要 sqlite）、reqwest（rustls）、socketioxide 0.16.2、jsonwebtoken、argon2、tokio。Release 为了体积：`opt-level = s`、`lto = fat`、`codegen-units = 1`、`panic = abort`、`strip = true`。

前端没有自己的产品名重写。`frontend/src/lib` 仍是 Open WebUI 的组件树：`components/chat`、`layout`、`admin`、`workspace`、`common`，外加 `lib/apis` 按旧路径调接口。新功能是在这棵树上改的，例如工具页、联网设置、用量、记忆、输入框延迟加载。

## 3. 进程怎么起来

入口是 `backend/crates/server/src/main.rs`。

1. 若命令行参数是 `healthcheck`，请求 `http://127.0.0.1:$PORT/health`，成功退出码 0，否则 1。Docker 健康检查走这条。
2. `Config::from_env()` 读环境变量，建数据目录。
3. 密钥：有 `FERROCHAT_SECRET_KEY` 就用它；否则读 `数据目录/secret.key`；还没有就生成 UUID 写进去。这把密钥签 JWT。多进程必须用同一个环境变量，否则令牌互相不认。
4. `Db::connect` 打开 SQLite，跑迁移，写入内置提供商。
5. Socket.IO 挂在 `/ws/socket.io`。新连接放进 `App.sockets`，并推一条空的 `user-list` 和 `usage`，因为没有多用户在线列表。
6. 路由：`/health`、`http::router()` 里的全部 `/api`，其余路径落到 SPA。静态文件优先读 `FERROCHAT_FRONTEND_DIR`，没有就用编译时嵌进二进制的 `frontend/build`。
7. CORS 全开，外面套 tracing。`axum::serve` 监听到进程退出。

`App` 里除了数据库和密钥，还有：按任务 id 存的取消令牌、按对话 id 存的任务列表、密钥轮换计数器、数据目录、可选的前端目录。

未知的 `/api` 路径应返回 JSON 404。早期漏掉的路径会掉进 SPA，浏览器拿到 HTML，前端按 JSON 解析就失败。这是改路由时要守的约束。

## 4. 登录

实现在 `backend/crates/server/src/auth.rs`。

密码用 Argon2，盐来自 UUID。登录成功签发 JWT，声明里只有用户 id，过期时间 14 天。请求带 `Authorization: Bearer <token>`。前端把令牌放在 `localStorage.token`。

`/api/config` 不需要登录也能看。它告诉前端：产品名 Ferrochat、版本、是否还在引导（用户数为 0）、注册是否开放、WebSocket 开着。登录之后再补一批开关。联网是否可用，取决于 `config` 表里的 `web_search` 已经配了引擎，或者打开了「优先用模型自己的搜索」。文件限制写死：单文件约 10 MB，一次最多 5 个。

用户设置存在 `users.settings_json`，接口是 `GET/POST /api/v1/users/user/settings`。最近消息条数、自动摘要、记忆开关、富文本开关都在这份 JSON 里，不是单独的列。

## 5. 数据库

连接串 `sqlite:{数据目录}/ferrochat.db?mode=rwc`，外键打开。池子最少 1、最多 5，空闲 60 秒断开。启动先整段执行 `001_init.sql`（语句都是 `IF NOT EXISTS`），再执行 `apply_later_migrations`。已经上过线的迁移文件不要改，下一个变更用新编号。

### 5.1 初始表

`users`：id、邮箱唯一、显示名、密码哈希、角色、头像、`settings_json`、`info_json`、创建和更新时间。

`chats`：id、用户、标题、整段对话的 `chat_json`、分享 id、是否归档、是否置顶、`meta_json`、文件夹 id。消息树不拆成行，前端发来的 history 整包存进去。更新时要和库里的 JSON 合并，不能整段替换，否则生成好的标题会被盖掉。

`tags` 与 `chat_tags`：标签名在单个用户内唯一。`folders`：可以有父级，展开状态存在列上。

`config`：主键是 key，值是 JSON。默认模型、联网、音频、任务开关都在这里。

`providers`：id、类型、显示名、base URL、密钥字符串、额外请求头、是否启用、排序、是否内置。

`provider_models`：某个提供商下选用的远端模型。id 一般是 `提供商:模型`。有分组名、能力 JSON、启用、排序。

`models`：工作区预设。可以指向一个基础模型，自带参数和 meta。这和「提供商模型」不是同一张表。提供商模型是你从远端勾进来的；工作区模型是套在上面的名字、系统提示和默认参数。

`prompts`：斜杠命令，主键是 command。

`tool_servers`：MCP。类型、名字、`config_json`、是否启用。

`files`：上传记录。二进制在 `数据目录/uploads/{id}`，不进 SQLite。

### 5.2 后来的迁移

`002_models_share.sql`：`chats(user_id, updated_at)` 索引，方便侧栏按时间列。`provider_models.params_json` 存默认参数和单价。`provider_key_health` 记录某把密钥的最近错误和冷却截止时间。

`003_file_passages.sql`：FTS5 虚表 `file_passages`，trigram 分词。按文件、文件名、页码存正文，用来在回答里引用页。

`004_memories.sql`：`memories` 以及 `memories_fts`。记忆按用户隔离。

`005_chat_search.sql`：`chats_fts` 存标题和正文，侧栏搜索走它。`usage_events` 按用户、模型、日期记下 prompt token、completion token 和费用。

## 6. 内置提供商

第一次连接数据库时种下这些行。类型决定用哪个适配器：`anthropic`、`gemini` 各自一套，其余走 OpenAI 兼容实现。`ollama` 的类型名单独写，请求格式仍按 OpenAI。密钥列初始为空，提供商默认不启用。若环境变量 `FERROCHAT_PROVIDER_{ID}_API_KEY` 有值，启动时写入并启用。

| id | 适配器 | 默认地址 |
| --- | --- | --- |
| openai | openai | `https://api.openai.com/v1` |
| anthropic | anthropic | `https://api.anthropic.com` |
| gemini | gemini | `https://generativelanguage.googleapis.com/v1beta` |
| deepseek | openai | `https://api.deepseek.com/v1` |
| openrouter | openai | `https://openrouter.ai/api/v1` |
| siliconflow | openai | `https://api.siliconflow.cn/v1` |
| ollama | ollama | `http://127.0.0.1:11434/v1` |
| azure | azure | 占位的 Azure 部署地址，要改成自己的资源 |
| groq | openai | `https://api.groq.com/openai/v1` |
| moonshot | openai | `https://api.moonshot.cn/v1` |
| zhipu | openai | `https://open.bigmodel.cn/api/paas/v4` |
| dashscope | openai | `https://dashscope.aliyuncs.com/compatible-mode/v1` |
| volcengine | openai | `https://ark.cn-beijing.volces.com/api/v3` |
| minimax | openai | `https://api.minimax.chat/v1` |
| baichuan | openai | `https://api.baichuan-ai.com/v1` |
| stepfun | openai | `https://api.stepfun.com/v1` |

管理页还可以手添提供商，不必是这张表里的 id。导入导出是 `/api/v1/providers/export` 和 `import`。连通性检查是 `POST /api/v1/providers/{id}/check`。拉远端模型列表是 `GET .../remote-models`，勾选后 `POST .../models/batch` 一次写入。

模型 id 在聊天里写成 `提供商id:远端模型id`，例如 `deepseek:deepseek-chat`。`split_model` 只按第一个冒号切开。

拉列表时，`ferrochat_core::infer_capabilities` 用模型名猜能力，管理员之后可以改：

- 名字里有 vision、gpt-4o、claude-3、gemini、qwen-vl 等，标成视觉
- 有 reason、o1、o3、deepseek-r1、qwq、gemini-2.5 等，标成推理
- 有 embed，标成向量，并且不标工具
- 有 search 或 online，标成可联网
- 有 whisper、tts、audio，标成音频

`group_name` 按前缀把模型收成系列，供管理页分组。这些规则是字符串包含，不是厂商官方清单，新模型名对不上时要手改能力。

一个提供商的 `api_keys` 可以是多把，轮换用内存里的计数器。某把密钥报错后写入 `provider_key_health`，冷却 60 秒，这段时间跳过它。

## 7. 一次对话

前端把当前历史交给 `POST /api/chat/completions`。`chat::run` 生成 `task_id`，放进取消表，立刻返回 `{status, task_id}`。真正的生成在 `tokio::spawn` 里跑 `drive`。失败时向这条对话推一条带 error 的完成事件，再推 done。任务结束从 map 里删掉。停止按钮打 `POST /api/tasks/stop/{id}`，取消令牌让循环退出。`GET /api/tasks/chat/{chat_id}` 列出这条对话上的任务。

`drive` 的顺序：

1. 读 `model`，`resolve` 成提供商 id、远端模型 id、预设系统提示、额外工具。提供商未启用就直接错误返回。
2. 取下一把没在冷却中的密钥，按提供商类型 `build` 出适配器。
3. `fold_context` 整理消息。非视觉模型会丢掉图片部分。附带的文件若是小文本，可以直接折进上下文。
4. `attach_document_excerpts` 从 `file_passages` 取摘录，带页码。
5. 读该模型的 `params_json`。请求里没有系统消息时，用预设里的 system。`substitute` 替换用户名一类占位。
6. 请求体 `features.web_search` 为真时跑 `prepare_web_search`。模型有 web 能力就让适配器自己搜（`native_search`）。否则用已配置引擎。每条来源推 `source` 事件，摘录写进消息。
7. `load_tools` 拉取这次勾选的 MCP 工具，转成 OpenAI tools 数组。
8. `compress_messages`：设置里的最近条数大于 0 时，丢掉更早的消息，必要时让模型写摘要。摘要和覆盖条数用 `chat:summary` 推给前端，下次同一段历史会复用，不重复概括。
9. 最多 5 轮。每轮把消息、合并后的参数、工具模式发给 `chat_stream`。流里的推理文本包进 `<details type="reasoning">`，正文增量用 `chat:completion` 的 delta 推出去。引用也是 `source` 事件。
10. 这一轮如果收齐了工具调用，把助手消息和 `tool` 结果追加进 messages，清空工具列表，再进入下一轮。工具执行是 `ferrochat_mcp::call_tool`。未知工具名会写成一段错误文本，不让整个请求崩掉。取消令牌在流的每个 chunk 上检查。
11. 流里若带了供应商返回的 token 数就用它。缺了就估算：提问侧用压缩后的消息长度，回答侧大约每 4 个字符一个 token。`estimated` 标志告诉前端要不要显示「估算」。费用是 `(输入 token × 输入单价 + 输出 token × 输出单价) / 1_000_000`。单价优先用模型上存的 `input_price` / `output_price`。然后 `record_usage`。
12. 推最终的 done，带全文、标题和 usage。
13. 若 `background_tasks.title_generation` 开着，`maybe_title` 再请求一次短标题，写入聊天行，并推 `chat:title`。标签同理，推 `chat:tags`。
14. `follow_up` 默认开，生成几条追问，事件 `chat:follow_ups`。
15. 用户开了记忆、且允许提议时，`suggest_memory` 抽出一句事实，事件 `chat:memory_suggestion`。前端不自动保存，要点确认才 `POST /api/v1/memories/add`。

密钥在流中途出错会 `mark_key_error` 然后把错误返回给前端，这一轮不会自动换下一把重试。换钥匙发生在下一次 `drive` 开头。

选中文字的「问」和「解释」走 `stream_direct`，不经过上面这套完整后处理，文本直接进调用方给的 channel。

前端收到 socket 事件后，在 `Chat.svelte` 里改当前那条消息的字段。token 增量不每字触发一次整页赋值，而是 `requestAnimationFrame` 合并。消息组件引用 `history.messages[id]` 的浅拷贝，只在正文或完成状态变化时换对象，不再 `JSON.parse(JSON.stringify(...))`。Markdown 对已经稳定的段落缓存 lexer 结果，生成过程中只重解析最后一个段落。

侧栏列表一次切片 40 条，多出来的要点「显示更多」，分页接口仍然存在。离开对话时清掉 usage 心跳的 `setInterval`、未完成的动画帧，以及控制面板上的 `ResizeObserver`。

## 8. 文件、搜索、工具、语音

上传 `POST /api/v1/files/`，multipart。文件按 UUID 写到 `uploads/`。抽正文：

- PDF：按页 `pdf_extract`
- docx：读文档 XML
- xls、xlsx：calamine
- csv 和普通文本：当文本

每页写入 `file_passages`。下载是 `GET /api/v1/files/{id}/content`。

搜索 crate 统一成 `Hit { title, url, snippet }`。引擎包括 SearXNG、Tavily、Brave、Bing、Google、DuckDuckGo。`join_base` 会去掉用户多贴的路径和查询串，避免 base URL 和接口路径拼两次。网页摘录会去掉脚本和标签。SearXNG 推荐用仓库里的 `docker-compose.searxng.yml`，本机端口 8088，再在设置里填 `http://127.0.0.1:8088`。不保存引擎时，`enable_web_search` 是 false。

MCP 客户端说 JSON-RPC：`initialize`、`tools/list`、`tools/call`。传输是子进程 stdio，或 Streamable HTTP。工具描述进模型的 tools 数组。容器里要跑 `npx` / `uvx` 时用 Docker `full` 阶段；`slim` 没有 Node 和 uv。

音频配置在 `config` 键 `audio`。`stt_engine` 默认 `web`，用浏览器语音识别。改成远程后，`/api/v1/audio/transcriptions` 和 `/api/v1/audio/speech` 转发到兼容 OpenAI 的地址。

工具页在前端自己调 `/api/chat/completions`，系统提示按翻译、润色、总结写死，不新建 `chats` 行。总结可以把 `features.web_search` 设为真。历史记在浏览器 `localStorage` 键 `ferrochat-tool-history`，最多 20 条，可以单条删。模型选择记在 `ferrochat-tool-model`。

## 9. 前端

### 9.1 路由

| 路径 | 作用 |
| --- | --- |
| `/auth` | 用户数为 0 时注册管理员，否则登录 |
| `/` | 新对话 |
| `/c/[id]` | 已有对话 |
| `/s/[id]` | 分享页，只读，不要求这是你的会话 |
| `/tools`、`/tools/[kind]` | 翻译、润色、总结 |
| `/admin` | 转到 `/admin/settings` |
| `/admin/settings?tab=` | 管理设置 |
| `/workspace/models` | 预设的列表、创建、编辑 |
| `/workspace/prompts` | 斜杠提示词 |
| `/workspace/tools` | MCP 服务器 |

管理标签：`providers`、`models`、`general`、`web_search`、`audio`、`usage`、`interface`。

聊天设置（用户自己的，不是管理页）里有通用项、界面、个性化。最近消息条数和自动摘要在通用。记忆开关和「提议记忆」在个性化。没有可用模型时，首页和工具页链到 `/admin/settings`，文案是「去添加提供商」。

### 9.2 聊天页组成

`Chat.svelte` 是编排者：历史对象、选中的模型、socket 监听、发送和停止。消息列表在 `Messages.svelte`，空对话时才动态加载，避免把 Markdown 渲染整棵树打进首屏。单条助手消息是 `ResponseMessage.svelte`，用户消息是 `UserMessage.svelte`，多模型并排是 `MultiResponseMessages.svelte`。

输入是 `MessageInput.svelte`。富文本默认开着时，先渲染 textarea（Enter 发送，Shift+Enter 换行，能贴文件）。`requestIdleCallback` 之后，或用户真实聚焦（`isTrusted`）时，才 `import` `RichTextInput.svelte`。程序自己的自动聚焦不算，否则一进页面就会把编辑器拉进来。设置里关掉富文本的人一直用 textarea。

右侧控制面板 `ChatControls.svelte` 也是打开时才加载。里面有工件和总览。总览依赖图布局，经 `OverviewLazy.svelte` 再加载。

代码块在需要时加载 CodeMirror。公式、mermaid、导出 PDF 同样是动态 import。`lowlight` 只注册 `common` 语言集。

### 9.3 首屏为什么能到 1 MB 以下

测量方式：从 `index.html` 的 modulepreload 出发，加上包含聊天页字符串的那个大块，只沿静态 `import` 走，跳过 `__vite__mapDeps` 里的动态依赖。优化前这条链大约 3.6 MB、55 个文件，最大一块 2.0 MB，里面是 MSAL（OneDrive）、tiptap、全部 highlight 语言、turndown、fuse。

现在这条链大约 0.85 MB、42 个文件，最大一块约 0.41 MB。OneDrive 和 Google Drive 的源文件已删，`@azure/msal-browser` 已从依赖去掉。dayjs 只静态带 en、zh-cn、zh-tw，其他语言在 `loadDayjsLocale` 里按需 import。

没有使用 Vite `manualChunks` 把编辑器、markdown、socket 收成具名文件。试过之后，Rollup 把预加载辅助函数放进编辑器块，结果任何带动态 import 的模块都会把编辑器重新拉回首屏。编辑器现在仍是单独的动态块，只是文件名是哈希，不是 `editor.js`。

## 10. 接口一览

全部登记在 `backend/crates/server/src/http/mod.rs`。下面按用途归类，不是逐行抄路由表。

会话与配置：`/api/config`、`/api/version`、`/api/changelog`、`/api/v1/auths/signin|signup|signout`、个人资料和密码、用户设置、横幅、配置导入导出、webhook 读写（功能开关是关的）。

对话：`/api/chat/completions`、`/api/chat/completed`、任务停止和按对话查询。`/api/v1/chats/` 分页列表（只含未归档）、`/chats/archived` 归档列表、`/chats/all` 和 `/chats/all/archived` 全量 JSON（导出用）、新建、搜索、置顶列表、按标签名筛选（`POST /chats/tags`）、一键全部归档、全部标签、导入。单条对话有读、更新、删除、置顶、归档（再点一次取消归档）、克隆、分享、移动文件夹、打标签、按名删标签。

文件夹和提示词：`/api/v1/folders/` 的列表、创建、重命名。`/api/v1/prompts/` 的列表、创建、更新、删除。

模型：公开列表 `/api/models`。工作区预设 `/api/v1/models/` 的增删改和 toggle。管理用的 `/api/v1/models/managed`、`reorder`、`default`。

提供商：列表和单条、删除、远端模型、连通性、已选模型、批量添加、单模型修改和删除、密钥、导入导出。

文件、记忆、用量、音频、联网测试、任务配置：见第 8 节和 `/api/v1/usage`、`/api/v1/memories/`。

实时：浏览器连 `/ws/socket.io`。服务端按对话把事件发给对应 socket。前端事件名是 `chat-events`。常见 `type`：`chat:completion`、`chat:message:delta`、`source`、`chat:title`、`chat:tags`、`chat:summary`、`chat:follow_ups`、`chat:memory_suggestion`。

空列表，调用了也不会有数据：`/api/v1/channels/`、`/api/v1/functions/`、`/api/v1/knowledge/`、`/api/v1/knowledge/list`、`/api/v1/groups/`。`/api/config` 里对应的 enable 开关为 false 的还有：代码执行、绘图、自动补全生成、社区分享、评分、用户 webhook、管理员查看他人聊天、Google Drive、OneDrive、直连、频道。

`#` 知识库命令的 Svelte 组件还在，因为它同时处理把 URL 贴进输入框。知识库列表是空的，选不出库。

## 11. 体积、测试、发布

`scripts/measure.sh` 只打印数字，缺文件就写 n/a，不让构建失败。聊天页 JS 用静态模块图，不把动态 import 算进首屏。浏览器堆用 Chrome 的 `Performance.getMetrics`，200 条消息那次是优化深拷贝之前测的。

| 项目 | 一开始 | 现在 |
| --- | --- | --- |
| `frontend/build` | 89.6 MB | 19.3 MB |
| 全部前端 JS | 15.2 MB | 10.5 MB |
| 聊天页首屏 JS | 3.6 MB，55 个文件 | 0.85 MB，42 个文件 |
| 最大 JS 块 | 2.0 MB | 0.41 MB |
| release 二进制 | 未测 | 26.4 MB |
| 空闲 RSS | 未测 | 6.5 MB |
| 10 路 mock 同时回复的 RSS 峰值 | 未测 | 28.3 MB |
| 200 条消息打开后的 JS 堆 | 未测 | 22.5 MB 已用，37.3 MB 预留 |
| svelte-check | 406 | 293 |

瘦身时删过 source map、wasm、整包 emoji。本机没有 Docker，slim 镜像没有打出来，所以没有镜像体积。README、CHANGELOG 和 `docs/metrics.md` 现在一致：聊天页首屏 0.85 MB / 42 个文件，最大块 0.41 MB。

后端测试：`cargo test`。覆盖能力推断、mock 流、搜索 HTML 清洗和 SearXNG JSON、摘要复用、以及 `backend/crates/server/tests/api.rs` 里的接口（登录、聊天 JSON、视觉消息、批量加模型、工具流、用量、记忆、没有内置预设污染、归档往返与取消归档、标签增删与筛选）。

前端：`npm run build`、`npm run check`、Playwright。`frontend/e2e/smoke.spec.ts` 三条常规跑；`frontend/e2e/chats.spec.ts` 两条标题带 `@chats`，只在 v* 标签构建里跑（`--grep "@chats"`），覆盖归档列表/取消归档/一键归档和标签筛选/删除这些前端早就调用、后端过去缺路由或空实现的接口。Playwright 自己拉起后端，端口默认 8091，前端用 `frontend/build`。若环境里有 `PLAYWRIGHT_BROWSERS_PATH` 指到不存在的目录，要先去掉这个变量。

本地手工看过的数据目录是 `/tmp/ferrochat-browser`，账号 `ada@ferrochat.local` / `secret1`。这不是仓库里的默认账号，只是那台机器上的浏览器数据。

发布：推 `main` 或开 PR 跑 `ci.yml`（fmt、`cargo test`、前端构建、体积、类型基线、常规 Playwright）；推 `v*` 标签才会跑 `release.yml` 和 `docker.yml`，产出 `1.2.3`、`1.2`、`latest`，架构 amd64 和 arm64。镜像名 `ghcr.io/yoisakiknd/ferrochat`。`docker compose up -d` 使用它。本地构建：

```bash
docker build -f docker/Dockerfile --target slim -t ferrochat .
docker run -p 3000:8080 -v ferrochat-data:/data ferrochat
```

升级前复制整个数据目录。GitHub Release 的二进制旁边有 `.sha256`。工作区版本号已升到 0.2.0（Cargo.toml、Cargo.lock、frontend/package.json），联网、记忆、工具页、摘要、用量、首屏瘦身、归档/标签接口补全都在 CHANGELOG 的 0.2.0，尚未推 `v0.2.0` 标签，所以镜像和 Release 还停在 0.1.0。

本地开发：

```bash
cd frontend && npm ci && npm run build
cd ../backend && FERROCHAT_FRONTEND_DIR=../frontend/build cargo run --bin ferrochat
```

| 变量 | 默认 | 作用 |
| --- | --- | --- |
| `FERROCHAT_HOST` | `0.0.0.0` | 绑定地址 |
| `FERROCHAT_PORT` | `8080` | 端口 |
| `FERROCHAT_DATA_DIR` | `./data` | 数据库、上传、密钥 |
| `FERROCHAT_SECRET_KEY` | 自动生成到 `secret.key` | 多进程必须固定 |
| `FERROCHAT_FRONTEND_DIR` | 使用嵌进二进制的构建 | 开发时指到 `frontend/build` |
| `FERROCHAT_PROVIDER_{ID}_API_KEY` | 空 | 首次启动写入并启用对应内置提供商 |

改前端后要重新 `npm run build`。若后端是用 `FERROCHAT_FRONTEND_DIR` 起的，它读磁盘上的 `build`，一般不用重编 Rust。嵌进二进制的那种必须重编才会换网页。

## 12. 还没收口的地方

- svelte-check 还有 293 条，集中在模型编辑器、输入框、单条回复、总览节点、访问控制和 `lib/utils`。基线从 413 收紧到 293，2026-09-26 本机 `npx svelte-check --output machine` 实测正好 293，与基线一致。Playwright 用例没在本机跑（浏览器未安装）。
- 200 条消息的堆内存没有在去掉深拷贝之后重测。
- slim 镜像没有在这台机器上构建。
- 知识库命令组件还在源码里，后端没有知识库数据。
- 能力推断靠模型名字，冷门模型会标错，需要在管理页手改。
- 工具调用最多 5 轮，密钥出错不会在同一次生成里自动换钥匙。
- 归档/标签接口补全、console 剥离、CI 触发扩展、迁移守卫、0.2.0 版本号已在 2026-09-26 本机实测收口：`cargo fmt --check` 干净（此前全仓从未格式化过，本次连带 `gemini.rs`/`openai.rs`/`chat.rs`/`files.rs` 等既有文件一并格式化）；`cargo test` 15 个测试 0 失败（含归档切换、标签增删两条新集成测试）；`vite build` 成功，产物中 `console.log(` 调用从源码的 479 处降到 1 处（第三方库的条件引用，非调用点）；Playwright 用例仍未在本机跑。
- 迁移守卫核对 `migrations/` 的文件名白名单（001–005，缺文件或多文件都红）并校验每个已存在的 `.sql.sha256`。001–005 的摘要均已生成并校验通过。新增迁移时同步生成摘要并加白名单条目：`cd backend/crates/db/migrations && sha256sum <新文件>.sql | cut -d' ' -f1 > <新文件>.sql.sha256`。
