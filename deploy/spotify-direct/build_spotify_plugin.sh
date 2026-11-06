#!/bin/bash
set -euo pipefail
cd /data/workspace/spotify-plugin
mkdir -p classes/lavalink-plugins
javac -cp 'deps/BOOT-INF/lib/*' -d classes /data/workspace/SpotifyDirectPlugin.java
printf '%s\n' 'name=elvnx-spotify-direct' 'path=online.elvnx.spotify' 'version=0.1.0' > classes/lavalink-plugins/elvnx-spotify-direct.properties
jar cf elvnx-spotify-direct.jar -C classes .
sudo install -m 640 -o lavalink -g lavalink elvnx-spotify-direct.jar /var/lib/lavalink/plugins/elvnx-spotify-direct.jar
