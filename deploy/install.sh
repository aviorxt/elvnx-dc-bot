#!/usr/bin/env bash
set -euo pipefail
source_dir="$HOME/elvnx-build"
test -f "$source_dir/BUILD_OK"
sudo install -o root -g root -m 755 "$source_dir/target/vps/elvnx-bot" /opt/elvnx-bot/elvnx-bot
sudo install -o root -g root -m 644 "$source_dir/deploy/discord-check.py" /opt/elvnx-bot/discord-check.py
sudo install -d -o root -g root -m 755 /opt/elvnx-bot/licenses
sudo install -o root -g root -m 644 "$source_dir"/assets/fonts/*-OFL.txt /opt/elvnx-bot/licenses/
sudo install -o root -g root -m 644 "$source_dir/automod.json" /etc/elvnx-bot/automod.json
sudo ln -sfn /etc/elvnx-bot/automod.json /var/lib/elvnx-bot/automod.json
sudo install -o root -g root -m 644 "$source_dir/deploy/elvnx-bot.service" /etc/systemd/system/elvnx-bot.service
sudo systemd-analyze verify /etc/systemd/system/elvnx-bot.service
sudo systemctl daemon-reload
sudo systemctl enable elvnx-bot
sudo -u elvnx sh -c 'umask 077; cd /var/lib/elvnx-bot && /opt/elvnx-bot/elvnx-bot --check && /opt/elvnx-bot/elvnx-bot --preview'
sudo chmod 600 /var/lib/elvnx-bot/data/elvnx.db
if sudo test -s /etc/elvnx-bot/bot.env; then
  printf '%s\n' 'Service installed. Credentials exist; validate them before starting.'
else
  printf '%s\n' 'Service installed and enabled. Discord token is required before startup.'
fi
