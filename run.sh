#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
local_toolchain="$project_dir/../.toolchain"
if [ -x "$local_toolchain/cargo/bin/cargo" ]; then
    export CARGO_HOME="$local_toolchain/cargo"
    export RUSTUP_HOME="$local_toolchain/rustup"
    export PATH="$CARGO_HOME/bin:$PATH"
fi
if ! command -v cargo >/dev/null 2>&1; then
    echo 'Rust is required. Install it from https://rustup.rs, then run this script again.' >&2
    exit 1
fi
exec cargo run --quiet --release --manifest-path "$project_dir/Cargo.toml" -- "$@"
