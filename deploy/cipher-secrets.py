"""Generate private cipher credentials on the VPS without printing them."""
from pathlib import Path
import os
import secrets

path = Path('/etc/lavalink/cipher.env')
if not path.exists():
    path.write_text('API_TOKEN=' + secrets.token_hex(32) + '\nHOST=127.0.0.1\nPORT=8001\nMAX_THREADS=1\nPREPROCESSED_CACHE_SIZE=5\nOVERRIDE_PLAYER_VARIANT=IAS\n', encoding='utf-8')
os.chmod(path, 0o600)
settings = dict(line.split('=', 1) for line in path.read_text().splitlines() if '=' in line)
youtube = Path('/etc/lavalink/youtube.env')
lines = [line for line in youtube.read_text().splitlines() if not line.startswith('YOUTUBE_CIPHER_PASSWORD=')]
lines.append('YOUTUBE_CIPHER_PASSWORD=' + settings['API_TOKEN'])
youtube.write_text('\n'.join(lines) + '\n')
os.chmod(youtube, 0o600)
print('Private cipher credential installed; OAuth credential preserved.')
