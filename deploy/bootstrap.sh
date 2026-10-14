#!/usr/bin/env bash
set -euo pipefail
sudo apt-get update -qq
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends build-essential pkg-config curl ca-certificates fonts-noto-core
if ! command -v swapon >/dev/null; then exit 1; fi
if [ "$(swapon --noheadings | wc -l)" -eq 0 ]; then
  sudo fallocate -l 2G /var/swap-elvnx
  sudo chmod 600 /var/swap-elvnx
  sudo mkswap /var/swap-elvnx
  sudo swapon /var/swap-elvnx
  if ! grep -q '^/var/swap-elvnx ' /etc/fstab; then
    printf '%s\n' '/var/swap-elvnx none swap sw 0 0' | sudo tee -a /etc/fstab >/dev/null
  fi
fi
if ! id elvnx >/dev/null 2>&1; then
  sudo useradd --system --home-dir /var/lib/elvnx-bot --shell /usr/sbin/nologin elvnx
fi
sudo install -d -o elvnx -g elvnx -m 750 /var/lib/elvnx-bot /var/lib/elvnx-bot/data
sudo install -d -o root -g root -m 755 /opt/elvnx-bot /etc/elvnx-bot
if [ ! -x "$HOME/.cargo/bin/rustc" ]; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/elvnx-rustup.sh
  sh /tmp/elvnx-rustup.sh -y --profile minimal --no-modify-path
fi
"$HOME/.cargo/bin/rustup" component add clippy rustfmt
"$HOME/.cargo/bin/rustc" --version
free -h
