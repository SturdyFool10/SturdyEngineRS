#!/usr/bin/env sh
set -eu

cd "$(dirname "$0")/.."

CBINDGEN="${CBINDGEN:-cbindgen}"
if ! command -v "$CBINDGEN" >/dev/null 2>&1; then
  if [ -x "$HOME/.cargo/bin/cbindgen" ]; then
    CBINDGEN="$HOME/.cargo/bin/cbindgen"
  else
    echo "cbindgen not found. Install it with: cargo install cbindgen --locked" >&2
    exit 127
  fi
fi

"$CBINDGEN" --config cbindgen.toml --crate sturdy-engine-ffi --output include/sturdy_engine.h
