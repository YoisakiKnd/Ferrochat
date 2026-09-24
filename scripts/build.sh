#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
(cd frontend && npm ci && npm run build)
FERROCHAT_FRONTEND_DIR="$PWD/frontend/build" cargo build --manifest-path backend/Cargo.toml --release --bin ferrochat
echo "binary: backend/target/release/ferrochat"
