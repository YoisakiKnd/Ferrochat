# Ferrochat size and memory

Numbers come from `scripts/measure.sh`. Later stages should be compared with this baseline.
Browser heap is measured separately with Playwright (`performance.memory` or CDP `Performance.getMetrics`) and filled in when a run is available.

## Baseline (before slimming, 2026-09-25)

| Item | Value |
| --- | --- |
| `frontend/build` | 89.6 MB |
| sourcemaps | 34.8 MB |
| `wasm/` | 22.0 MB |
| `assets/` | 15.6 MB (emojis 8.7, images 5.3, fonts 1.7) |
| frontend JS (uncompressed) | 15.2 MB |
| release binary | not built |
| slim image | not built |
| idle RSS | not measured |
| 10-stream peak RSS | not measured |
| chat page first-load JS | not measured |
| JS heap after a 200-message chat | not measured |

## After phase 1 frontend (2026-09-25)

| Item | Value |
| --- | --- |
| `frontend/build` | 20.4 MB (was 89.6 MB) |
| sourcemaps | removed |
| `wasm/` | removed |
| `assets/emojis` | removed |
| frontend JS (uncompressed, all chunks) | 11.5 MB (was 15.2 MB) |
| largest JS chunk | 2.0 MB (chat editor: tiptap / prosemirror) |
| release binary | not built yet |
| slim image | Dockerfile updated, not built locally |
| idle RSS | not measured |

Heavy libraries (mermaid, katex, jspdf, html2canvas, xyflow, codemirror, highlight.js) are now separate chunks and load only when that feature is used. The chat page still loads the rich-text editor up front.

## After the wrap-up (2026-09-25)

Measured with `scripts/measure.sh`, `ps` on macOS, and Chrome performance metrics in the running app. The slim image was not built: Docker is not installed on this machine.

| Item | Value | Note |
| --- | --- | --- |
| `frontend/build` | 20.5 MB | was 20.4 MB after phase 1 |
| frontend JS, all chunks | 11.6 MB | |
| largest JS chunk | 2.0 MB | chat editor (tiptap / prosemirror) |
| HTML modulepreload | 63 KB | shell only, before the chat route loads |
| chat page decoded JS | 3.6 MB | 55 scripts after opening a chat; transfer was cache hits |
| release binary | 26.4 MB | `strip = true`, `opt-level = s`, frontend embedded |
| slim image | not built | Docker is not available here |
| idle RSS (release) | 6.5 MB | fresh process, before any request |
| 10-stream peak RSS (release) | 28.3 MB | ten mock completions at once; RSS stayed at the peak after they returned |
| JS heap, 200-message chat | 22.5 MB used, 37.3 MB reserved | one browser tab after opening that chat |

The old targets of a first-load JS bundle under 1 MB and a slim image under 25 MB are not met. The chat page pulls in the editor chunk immediately, which alone is 2.0 MB decoded. The image was not measured. The release binary is 26.4 MB because it embeds the frontend.

## After the frontend page pass (2026-09-25)

Chat-page JS is the static module graph of the shell plus the chat route, ignoring dynamic `import()` edges. Rich text, `marked`, panzoom, yaml, and dayjs locales other than English and Chinese load later.

| Item | Value | Note |
| --- | --- | --- |
| `frontend/build` | 19.3 MB | was 20.5 MB |
| frontend JS, all chunks | 10.5 MB | was 11.6 MB |
| largest JS chunk | 0.41 MB | was 2.0 MB (editor is no longer in the first load) |
| chat page first-load JS | 0.85 MB | 42 files; was 3.6 MB / 55 scripts |
| svelte-check | 293 errors | was 406 |
| JS heap, 200-message chat | 22.5 MB used, 37.3 MB reserved | previous run; streaming no longer deep-copies every token |
| release binary | 26.4 MB | unchanged this pass |
| slim image | not built | Docker is not available here |
