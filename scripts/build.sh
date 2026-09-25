#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
(cd frontend && npm ci && npm run build)
node <<'JS'
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const root = path.join('frontend', 'build');
const skip = new Set(['.png', '.jpg', '.jpeg', '.webp', '.gif', '.wasm', '.woff2', '.br', '.gz']);
function walk(dir) {
  for (const name of fs.readdirSync(dir)) {
    const file = path.join(dir, name);
    const stat = fs.statSync(file);
    if (stat.isDirectory()) walk(file);
    else if (!skip.has(path.extname(name)) && stat.size > 1024) {
      const data = fs.readFileSync(file);
      fs.writeFileSync(file + '.gz', zlib.gzipSync(data, { level: 9 }));
      fs.writeFileSync(file + '.br', zlib.brotliCompressSync(data));
    }
  }
}
if (fs.existsSync(root)) walk(root);
JS
FERROCHAT_FRONTEND_DIR="$PWD/frontend/build" cargo build --manifest-path backend/Cargo.toml --release --bin ferrochat
echo "binary: backend/target/release/ferrochat"
