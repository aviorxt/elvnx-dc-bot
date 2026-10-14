#!/usr/bin/env bash
set -euo pipefail
source_dir="$HOME/elvnx-build"
test -f "$source_dir/BUILD_OK"
sudo install -o root -g root -m 755 "$source_dir/target/vps/elvnx-bot" /opt/elvnx-bot/elvnx-bot.next
sudo cp -p /opt/elvnx-bot/elvnx-bot /opt/elvnx-bot/elvnx-bot.previous
sudo systemctl stop elvnx-bot
sudo mv /opt/elvnx-bot/elvnx-bot.next /opt/elvnx-bot/elvnx-bot
if ! sudo systemctl start elvnx-bot; then
    sudo cp -p /opt/elvnx-bot/elvnx-bot.previous /opt/elvnx-bot/elvnx-bot
    sudo systemctl reset-failed elvnx-bot
    sudo systemctl start elvnx-bot
    exit 1
fi
sleep 5
if ! sudo systemctl is-active --quiet elvnx-bot; then
    sudo systemctl stop elvnx-bot
    sudo cp -p /opt/elvnx-bot/elvnx-bot.previous /opt/elvnx-bot/elvnx-bot
    sudo systemctl reset-failed elvnx-bot
    sudo systemctl start elvnx-bot
    exit 1
fi
sudo systemctl is-active elvnx-bot
sha256sum /opt/elvnx-bot/elvnx-bot
sudo journalctl -u elvnx-bot -n 5 --no-pager
