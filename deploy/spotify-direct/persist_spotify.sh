#!/bin/bash
set -euo pipefail
export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus
mkdir -p /data/home/.config/systemd/user /data/home/.config/pulse
cp /etc/pulse/default.pa /data/home/.config/pulse/default.pa
printf '\nload-module module-null-sink sink_name=elvnx_spotify sink_properties=device.description=ELVNXSpotify\n' >> /data/home/.config/pulse/default.pa
systemctl --user stop elvnx-spotify-bridge elvnx-spotify-desktop
systemctl --user reset-failed
cat > /data/home/.config/systemd/user/elvnx-spotify-desktop.service <<'EOF'
[Unit]
Description=ELVNX official Spotify audio client
After=pulseaudio.service
Wants=pulseaudio.service
[Service]
Environment=DISPLAY=:1
Environment=PULSE_SINK=elvnx_spotify
Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus
ExecStart=/usr/bin/spotify --disable-gpu --disable-dev-shm-usage
Restart=on-failure
RestartSec=10
[Install]
WantedBy=default.target
EOF
cat > /data/home/.config/systemd/user/elvnx-spotify-bridge.service <<'EOF'
[Unit]
Description=ELVNX private official Spotify PCM bridge
After=elvnx-spotify-desktop.service
Wants=elvnx-spotify-desktop.service
[Service]
Environment=DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus
ExecStart=/usr/bin/python3 /data/workspace/official_spotify_bridge.py
Restart=on-failure
RestartSec=5
[Install]
WantedBy=default.target
EOF
sudo loginctl enable-linger user
systemctl --user daemon-reload
systemctl --user enable --now elvnx-spotify-desktop elvnx-spotify-bridge
