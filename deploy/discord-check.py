#!/usr/bin/env python3
"""Read root-only bot settings and return non-secret deployment status."""
import json
import sys
import urllib.error
import urllib.request
import sqlite3
import os


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
    if len(sys.argv)>1 and sys.argv[1]=='availability-state':
        with sqlite3.connect('/var/lib/elvnx-bot/data/elvnx.db') as db:
            row=db.execute('SELECT value FROM availability WHERE guild=?',(settings['DISCORD_GUILD_ID'],)).fetchone()
        cfg=json.loads(row[0]) if row else {}
        boards=[]
        for board in cfg.get('boards',[]):
            item={'key':board['key'],'channel':board['channel'],'message_count':len(board['messages'])}
            if board['messages']:
                message=request(f'channels/{board["channel"]}/messages/{board["messages"][0]}')
                item['embed_count']=len(message.get('embeds',[]))
                item['field_names']=[field['name'] for embed in message.get('embeds',[]) for field in embed.get('fields',[])]
                item['attachment_count']=len(message.get('attachments',[]))
                item['cover_image_present']=any(embed.get('image',{}).get('url') for embed in message.get('embeds',[]))
            boards.append(item)
        children=[channel['name'] for channel in request('guilds/'+settings['DISCORD_GUILD_ID']+'/channels') if channel.get('parent_id')==str(cfg.get('category'))]
        print(json.dumps({'enabled':cfg.get('enabled',False),'category':cfg.get('category'),'audience':cfg.get('audience'),'boards':boards,'category_channels':children,'retired_count':len(cfg.get('retired',[]))}))
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
            'presence_intent_available': bool(flags & ((1 << 12) | (1 << 13))),
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
    elif len(sys.argv) > 1 and sys.argv[1] == 'backup-global':
        commands = request(f'applications/{user["id"]}/commands')
        os.umask(0o077)
        try:
            with open('/home/ubuntu/elvnx-global-commands-before-cleanup.json', 'x', encoding='utf-8') as destination:
                json.dump(commands, destination)
        except FileExistsError:
            pass
        print(json.dumps({'legacy_global_count': len(commands), 'backup_preserved': True}))
    elif len(sys.argv) > 1 and sys.argv[1] == 'command-audit':
        local = request(f'applications/{user["id"]}/guilds/{guild_id}/commands')
        global_commands = request(f'applications/{user["id"]}/commands')
        channels = request('guilds/' + guild_id + '/channels')
        roles = request('guilds/' + guild_id + '/roles')
        print(json.dumps({'guild_commands': [{'name': c['name'], 'id': c['id']} for c in local], 'global_commands': [{'name': c['name'], 'id': c['id']} for c in global_commands], 'channels': [{'id': c['id'], 'name': c['name'], 'type': c['type'], 'overwrites': c.get('permission_overwrites', [])} for c in channels if c['type'] == 0], 'roles': [{'id': r['id'], 'name': r['name'], 'permissions': r['permissions'], 'bot': r.get('tags', {}).get('bot_id')} for r in roles]}))
    elif len(sys.argv) > 1 and sys.argv[1] == 'commands':
        commands = request(f'applications/{user["id"]}/guilds/{guild_id}/commands')
        global_commands = request(f'applications/{user["id"]}/commands')
        expected = {'settings-panel', 'role-panel', 'poll', 'poll-close', 'poll-result', 'remind', 'reminders', 'reminder-cancel', 'report', 'setup-report', 'config-backup', 'config-restore', 'config-import', 'birthday', 'birthday-remove', 'birthday-setup', 'dj-role', 'vote-skip', 'shuffle', 'repeat', 'playlist'}
        names = {command['name'] for command in commands}
        expected.add('availibilty-panel')
        expected.add('version')
        playlist = next((command for command in commands if command['name'] == 'playlist'), {})
        report = next((command for command in commands if command['name'] == 'setup-report'), {})
        print(json.dumps({'command_count': len(commands), 'global_count': len(global_commands), 'duplicate_names': sorted(names & {c['name'] for c in global_commands}), 'missing_features': sorted(expected-names), 'playlist_subcommands': sorted(option['name'] for option in playlist.get('options', []) if option['type'] == 1), 'report_options': [{'name': o['name'], 'required': o.get('required', False)} for o in report.get('options', [])]}))
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
