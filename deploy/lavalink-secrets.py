import os
import secrets
from pathlib import Path

target = Path('/etc/lavalink/lavalink.env')
if target.exists():
    password = target.read_text().strip().split('=', 1)[1]
else:
    password = secrets.token_hex(32)
    target.write_text('LAVALINK_SERVER_PASSWORD=' + password + '\n')
os.chmod(target, 0o600)
bot = Path('/etc/elvnx-bot/bot.env')
lines = [line for line in bot.read_text().splitlines() if not line.startswith(('LAVALINK_URL=', 'LAVALINK_PASSWORD='))]
lines.extend(['LAVALINK_URL=http://127.0.0.1:2333', 'LAVALINK_PASSWORD=' + password])
bot.write_text('\n'.join(lines) + '\n')
os.chmod(bot, 0o600)
