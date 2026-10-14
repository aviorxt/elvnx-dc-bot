#!/usr/bin/env python3
"""Read root-only bot settings and return non-secret deployment status."""
import json
import sys
import urllib.error
import urllib.request
import sqlite3


def main():
    settings = {}
    with open('/etc/elvnx-bot/bot.env', encoding='utf-8') as source:
        for line in source:
            if '=' in line and not line.lstrip().startswith('#'):
                key, value = line.strip().split('=', 1)
                settings[key] = value.strip().strip('"').strip("'")

    def request(path):
        req = urllib.request.Request(
            'https://discord.com/api/v10/' + path,
            headers={
                'Authorization': 'Bot ' + settings['DISCORD_TOKEN'],
                'User-Agent': 'DiscordBot (https://www.avrxt.dev, 0.1.0)',
            },
        )
        with urllib.request.urlopen(req, timeout=20) as response:
            return json.load(response)

    if len(sys.argv) > 1 and sys.argv[1] == 'configuration':
        with sqlite3.connect('/var/lib/elvnx-bot/data/elvnx.db') as db:
            row = db.execute('SELECT value FROM settings WHERE guild=?', (settings['DISCORD_GUILD_ID'],)).fetchone()
        print(json.dumps({'log_channel': json.loads(row[0]).get('log_channel') if row else None}))
        return
    user = request('users/@me')
    if not user.get('bot'):
        raise ValueError('A dedicated bot token is required')
    guild_id = settings['DISCORD_GUILD_ID']
    if len(sys.argv) > 1 and sys.argv[1] == 'channels':
        channels = request('guilds/' + guild_id + '/channels')
        print(json.dumps([{'id': ch['id'], 'name': ch['name'], 'type': ch['type'], 'parent_id': ch.get('parent_id')} for ch in channels if ch['type']==4 or 'log' in ch['name'].lower()]))
    elif len(sys.argv) > 1 and sys.argv[1] == 'capabilities':
        app = request('applications/@me')
        flags = app.get('flags', 0)
        print(json.dumps({
            'member_intent_available': bool(flags & ((1 << 14) | (1 << 15))),
            'content_intent_available': bool(flags & ((1 << 18) | (1 << 19))),
            'external_interactions_endpoint': bool(app.get('interactions_endpoint_url')),
        }))
    elif len(sys.argv) > 1 and sys.argv[1] == 'music-permissions':
        member = request(f'guilds/{guild_id}/members/{user["id"]}')
        roles = request(f'guilds/{guild_id}/roles')
        permissions = 0
        for role in roles:
            if role['id'] == guild_id or role['id'] in member['roles']:
                permissions |= int(role['permissions'])
        print(json.dumps({'administrator':bool(permissions & 8), 'connect':bool(permissions & (1 << 20)), 'speak':bool(permissions & (1 << 21)), 'set_voice_channel_status':bool(permissions & (1 << 48))}))
    elif len(sys.argv) > 1 and sys.argv[1] == 'commands':
        commands = request(f'applications/{user["id"]}/guilds/{guild_id}/commands')
        expected = {'settings-panel', 'role-panel', 'poll', 'poll-close', 'poll-result', 'remind', 'reminders', 'reminder-cancel', 'report', 'report-setup', 'config-backup', 'config-restore', 'config-import', 'birthday', 'birthday-remove', 'birthday-setup', 'dj-role', 'vote-skip', 'shuffle', 'repeat', 'playlist'}
        names = {command['name'] for command in commands}
        playlist = next((command for command in commands if command['name'] == 'playlist'), {})
        print(json.dumps({'command_count': len(commands), 'missing_features': sorted(expected-names), 'playlist_subcommands': sorted(option['name'] for option in playlist.get('options', []) if option['type'] == 1)}))
    else:
        guild = request('guilds/' + guild_id)
        print(json.dumps({'bot': user['username'], 'bot_id': user['id'], 'guild': guild['name'], 'guild_id': guild['id']}))


if __name__ == '__main__':
    try:
        main()
    except urllib.error.HTTPError as error:
        print(json.dumps({'error': 'Discord API check failed', 'http_status': error.code}))
        sys.exit(2)
    except Exception as error:
        print(json.dumps({'error': 'Discord deployment check failed', 'type': type(error).__name__}))
        sys.exit(2)
