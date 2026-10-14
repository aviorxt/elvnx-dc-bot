#!/usr/bin/env bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$HOME/elvnx-build"
rm -f "$HOME/elvnx-build/BUILD_OK"
cargo fmt --all -- --check
cargo test --profile vps --locked -j 1
cargo clippy --profile vps --locked --all-targets -j 1 -- -D warnings
cargo build --profile vps --locked -j 1
./target/vps/elvnx-bot --check
./target/vps/elvnx-bot --preview
touch "$HOME/elvnx-build/BUILD_OK"
