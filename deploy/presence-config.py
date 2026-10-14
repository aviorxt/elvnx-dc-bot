#!/usr/bin/env python3
"""Enable the approved presence intent without printing or replacing other secrets."""
import os
from pathlib import Path

path = Path('/etc/elvnx-bot/bot.env')
lines = path.read_text(encoding='utf-8').splitlines()
lines = [line for line in lines if not line.startswith('ENABLE_PRESENCE_EVENTS=')]
lines.append('ENABLE_PRESENCE_EVENTS=true')
os.umask(0o077)
path.write_text('\n'.join(lines) + '\n', encoding='utf-8')
path.chmod(0o600)
print('Presence intent enabled in the private server environment.')
