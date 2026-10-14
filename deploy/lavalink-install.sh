#!/usr/bin/env bash
set -euo pipefail
sudo apt-get update -qq
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends openjdk-21-jre-headless
if ! id lavalink >/dev/null 2>&1; then sudo useradd --system --home-dir /var/lib/lavalink --shell /usr/sbin/nologin lavalink; fi
sudo install -d -o root -g root -m 755 /opt/lavalink /etc/lavalink
sudo install -d -o lavalink -g lavalink -m 750 /var/lib/lavalink /var/lib/lavalink/plugins
curl -fL --retry 3 https://github.com/lavalink-devs/Lavalink/releases/download/4.2.2/Lavalink.jar -o /home/ubuntu/Lavalink.jar
echo '8cb801e591072c3689fafd71ccf571a95a4ead3cc35dfc045e157d763d89119a  /home/ubuntu/Lavalink.jar' | sha256sum -c -
curl -fL --retry 3 https://maven.lavalink.dev/snapshots/dev/lavalink/youtube/youtube-plugin/2be8e542d3f6f178e048dca565892684c2e40177/youtube-plugin-2be8e542d3f6f178e048dca565892684c2e40177.jar -o /home/ubuntu/youtube-plugin.jar
echo 'b062effdda6abeaa46913dfd1b5d5600120b65f239ce88f30948c7213a3ca1c0  /home/ubuntu/youtube-plugin.jar' | sha256sum -c -
sudo install -o root -g root -m 644 /home/ubuntu/Lavalink.jar /opt/lavalink/Lavalink.jar
sudo install -o root -g root -m 644 /home/ubuntu/youtube-plugin.jar /var/lib/lavalink/plugins/youtube-plugin-current.jar
sudo install -o root -g lavalink -m 640 /home/ubuntu/elvnx-build/deploy/lavalink.yml /etc/lavalink/application.yml
sudo python3 /home/ubuntu/elvnx-build/deploy/lavalink-secrets.py
sudo install -o root -g root -m 644 /home/ubuntu/elvnx-build/deploy/lavalink-secrets.py /opt/lavalink/lavalink-secrets.py
sudo install -o root -g root -m 644 /home/ubuntu/elvnx-build/deploy/lavalink.service /etc/systemd/system/lavalink.service
sudo systemctl daemon-reload
sudo systemctl enable --now lavalink
