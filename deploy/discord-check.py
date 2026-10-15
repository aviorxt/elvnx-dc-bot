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

    if len(sys.argv)>1 and sys.argv[1]=='24-7':
        bot=request('users/@me')['id'];guild=settings['DISCORD_GUILD_ID'];global_commands=request('applications/'+bot+'/commands');guild_commands=request('applications/'+bot+'/guilds/'+guild+'/commands');alias=next((c for c in global_commands if c['name']=='24-7'),None)
        print(json.dumps({'guild_count':len(guild_commands),'global_names':[c['name'] for c in global_commands],'duplicate_names':sorted({c['name'] for c in global_commands}&{c['name'] for c in guild_commands}),'alias':{k:alias.get(k) for k in ['name','default_member_permissions','contexts','integration_types','options']} if alias else None}));return
    if len(sys.argv)>1 and sys.argv[1]=='music-features':
        guild=settings['DISCORD_GUILD_ID'];bot=request('users/@me')['id']
        with sqlite3.connect('file:/var/lib/elvnx-bot/data/elvnx.db?mode=ro',uri=True) as db:
            cfg=json.loads(db.execute('SELECT value FROM settings WHERE guild=?',(guild,)).fetchone()[0])
            panels={name:db.execute('SELECT channel,message FROM '+name+' WHERE guild=?',(guild,)).fetchone() for name in ['music_request_panels','music_favorite_panels']}
            favorites=db.execute('SELECT COUNT(*) FROM music_favorites WHERE guild=?',(guild,)).fetchone()[0]
        result={'configured':{k:cfg.get(k) for k in ['music_request_channel','music_favorite_channel','music_stay_channel']},'recaps_enabled':not cfg.get('music_recaps_disabled',False),'favorites_count':favorites,'registration_enabled':settings.get('REGISTER_COMMANDS')=='true'}
        for name,row in panels.items():
            if row:
                m=request('channels/'+row[0]+'/messages/'+row[1]);result[name]={'channel':row[0],'message':row[1],'owned_by_bot':m['author']['id']==bot,'titles':[e.get('title') for e in m.get('embeds',[])],'cover':any(e.get('image') for e in m.get('embeds',[])),'controls':[c.get('custom_id') for r in m.get('components',[]) for c in r.get('components',[])]}
            else:result[name]=None
        try:
            voice=request('guilds/'+guild+'/voice-states/'+bot);result['voice']={'channel':voice.get('channel_id'),'self_deaf':voice.get('self_deaf')}
        except urllib.error.HTTPError as e:result['voice']={'http_status':e.code}
        commands=request('applications/'+bot+'/guilds/'+guild+'/commands');playlist=next(c for c in commands if c['name']=='playlist');result['command_count']=len(commands);result['playlist_subcommands']=[o['name'] for o in playlist.get('options',[])];print(json.dumps(result));return
    if len(sys.argv)>1 and sys.argv[1]=='activity-state':
        guild=settings['DISCORD_GUILD_ID'];bot=request('users/@me')['id']
        with sqlite3.connect('file:/var/lib/elvnx-bot/data/elvnx.db?mode=ro',uri=True) as db:
            cfg=json.loads(db.execute('SELECT value FROM settings WHERE guild=?',(guild,)).fetchone()[0])
            panel=db.execute('SELECT channel,message,version FROM activity_panels WHERE guild=?',(guild,)).fetchone()
            voice=db.execute('SELECT channel,message FROM voice_room_panels WHERE guild=?',(guild,)).fetchone()
            rooms=db.execute('SELECT channel,owner FROM voice_rooms WHERE guild=?',(guild,)).fetchall()
        def panel_info(row):
            if not row:return None
            m=request('channels/'+row[0]+'/messages/'+row[1])
            return {'channel':row[0],'message':row[1],'owned_by_bot':m['author']['id']==bot,'title':[e.get('title') for e in m.get('embeds',[])],'animated_icon':any('.gif' in e.get('author',{}).get('icon_url','') for e in m.get('embeds',[])),'cover':any(e.get('image') for e in m.get('embeds',[])),'button_count':sum(len(r.get('components',[])) for r in m.get('components',[]))}
        channel=request('channels/'+str(cfg['log_channel']))
        roles=request('guilds/'+guild+'/roles');member=request('guilds/'+guild+'/members/'+bot)
        permissions=0
        for r in roles:
            if r['id']==guild or r['id'] in member['roles']:permissions|=int(r['permissions'])
        print(json.dumps({'log_header':panel_info(panel),'voice_panel':panel_info(voice),'rooms':rooms,'everyone_view_denied':any(o['id']==guild and int(o['deny'])&1024 and not int(o['allow'])&1024 for o in channel['permission_overwrites']),'audit_permission':bool(permissions&8 or permissions&128),'log_version':panel[2] if panel else None}));return
    if len(sys.argv)>1 and sys.argv[1]=='voice-state':
        guild=settings['DISCORD_GUILD_ID']
        with sqlite3.connect('file:/var/lib/elvnx-bot/data/elvnx.db?mode=ro',uri=True) as db:
            cfg=json.loads(db.execute('SELECT value FROM settings WHERE guild=?',(guild,)).fetchone()[0])
            rooms=db.execute('SELECT channel,owner FROM voice_rooms WHERE guild=?',(guild,)).fetchall()
        channels=request('guilds/'+guild+'/channels');by_id={c['id']:c for c in channels}
        hub=by_id.get(str(cfg.get('join_to_create')),{})
        def view(c):return {k:c.get(k) for k in ['id','name','type','parent_id','permission_overwrites']}
        print(json.dumps({'hub':view(hub),'category':view(by_id.get(hub.get('parent_id'),{})),'panel':cfg.get('voice_panel'),'rooms':[{'channel':r[0],'owner':r[1],'info':view(by_id.get(r[0],{}))} for r in rooms]}));return
    if len(sys.argv)>1 and sys.argv[1]=='premium-state':
        guild=settings['DISCORD_GUILD_ID'];bot=request('users/@me')['id']
        emojis=request('applications/'+bot+'/emojis')['items']
        with sqlite3.connect('file:/var/lib/elvnx-bot/data/elvnx.db?mode=ro',uri=True) as db:
            cfg=json.loads(db.execute('SELECT value FROM settings WHERE guild=?',(guild,)).fetchone()[0])
            exists=db.execute("SELECT COUNT(*) FROM sqlite_master WHERE name='honeypot_panels'").fetchone()[0]
            panel=db.execute('SELECT channel,message FROM honeypot_panels WHERE guild=?',(guild,)).fetchone() if exists else None
            actions=db.execute('SELECT COUNT(*) FROM honeypot_actions').fetchone()[0] if exists else None
        log=request('channels/'+str(cfg['log_channel']))
        private=any(o['id']==guild and int(o['deny']) & 1024 for o in log.get('permission_overwrites',[]))
        info={'animated_emoji_count':sum(e['name'].startswith('elvnx_live_') and e.get('animated',False) for e in emojis),'static_emoji_count':sum(e['name'].startswith('elvnx_') and not e.get('animated',False) for e in emojis),'private_log_deny_everyone':private,'honeypot_configured':cfg.get('honeypot_channel'),'honeypot_action_count':actions,'registration_enabled':settings.get('REGISTER_COMMANDS')=='true'}
        if panel:
            m=request('channels/'+panel[0]+'/messages/'+panel[1]);info['honeypot_panel']={'channel':panel[0],'message':panel[1],'owned_by_bot':m['author']['id']==bot,'titles':[e.get('title') for e in m.get('embeds',[])],'animated_icon':any('.gif' in e.get('author',{}).get('icon_url','') for e in m.get('embeds',[])),'cover_present':any(e.get('image') for e in m.get('embeds',[]))}
        print(json.dumps(info));return
    if len(sys.argv)>1 and sys.argv[1]=='honeypot-state':
        with sqlite3.connect('file:/var/lib/elvnx-bot/data/elvnx.db?mode=ro',uri=True) as db:
            row=db.execute('SELECT value FROM settings WHERE guild=?',(settings['DISCORD_GUILD_ID'],)).fetchone()
        cfg=json.loads(row[0]) if row else {}
        channels=request('guilds/'+settings['DISCORD_GUILD_ID']+'/channels')
        candidate=[c for c in channels if str(c['id'])==str(cfg.get('honeypot_channel')) or 'honeypot' in c['name'].lower()]
        print(json.dumps({'configured':cfg.get('honeypot_channel'),'log_channel':cfg.get('log_channel'),'channels':[{'id':c['id'],'name':c['name'],'type':c['type']} for c in candidate]}))
        return
    if len(sys.argv)>1 and sys.argv[1]=='verification-state':
        with sqlite3.connect('file:/var/lib/elvnx-bot/data/elvnx.db?mode=ro',uri=True) as db:
            row=db.execute('SELECT value FROM settings WHERE guild=?',(settings['DISCORD_GUILD_ID'],)).fetchone()
            cfg=json.loads(row[0]) if row else {}
            exists=db.execute("SELECT COUNT(*) FROM sqlite_master WHERE name='verification_policy'").fetchone()[0]
            policy=db.execute('SELECT value FROM verification_policy WHERE guild=?',(settings['DISCORD_GUILD_ID'],)).fetchone() if exists else None
            counts={table:db.execute('SELECT COUNT(*) FROM '+table).fetchone()[0] for table in ['verification_sessions','verification_requests']} if exists else {}
        print(json.dumps({'verification_role':cfg.get('verification_role'),'autorole':cfg.get('autorole'),'review_channel':cfg.get('report_channel'),'member_events':settings.get('ENABLE_MEMBER_EVENTS'),'policy':json.loads(policy[0]) if policy else None,'counts':counts}))
        return
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
    if len(sys.argv)>1 and sys.argv[1]=='toolbox-state':
        with sqlite3.connect('file:/var/lib/elvnx-bot/data/elvnx.db?mode=ro', uri=True) as db:
            tables = ['activity_optout', 'review_queue', 'scheduled_posts', 'community_events', 'health_alerts']
            counts = {table: db.execute(f'SELECT COUNT(*) FROM {table} WHERE guild=?', (settings['DISCORD_GUILD_ID'],)).fetchone()[0] for table in tables}
        print(json.dumps({'table_counts': counts}))
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
    elif len(sys.argv)>1 and sys.argv[1]=='verification-panel':
        with sqlite3.connect('file:/var/lib/elvnx-bot/data/elvnx.db?mode=ro',uri=True) as db:
            row=db.execute('SELECT value FROM settings WHERE guild=?',(guild_id,)).fetchone()
        cfg=json.loads(row[0]) if row else {}
        roles=request('guilds/'+guild_id+'/roles')
        channels=request('guilds/'+guild_id+'/channels')
        panels=[]
        for channel in channels:
            if channel['type']==0 and any(word in channel['name'].lower() for word in ['verif','welcome','rules']):
                for m in request('channels/'+channel['id']+'/messages?limit=100'):
                    if m['author']['id']==user['id'] and any(b.get('custom_id','').startswith('elvnx:verify:') for row in m.get('components',[]) for b in row.get('components',[])):
                        panels.append({'channel':channel['id'],'name':channel['name'],'message':m['id'],'buttons':[{'label':b.get('label'),'custom_emoji':b.get('emoji',{}).get('id')} for row in m.get('components',[]) for b in row.get('components',[])],'custom_author_icon':any('cdn.discordapp.com/emojis/' in e.get('author',{}).get('icon_url','') for e in m.get('embeds',[])),'attachment_names':[a.get('filename') for a in m.get('attachments',[])],'embed_image_paths':[e.get('image',{}).get('url','').split('?')[0] for e in m.get('embeds',[]) if e.get('image')]})
        print(json.dumps({'role':[{'id':r['id'],'name':r['name'],'permissions':r['permissions']} for r in roles if r['id']==str(cfg.get('verification_role'))], 'panels':panels}))
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
        expected.add('send-attachement')
        expected.update({'media-panel', 'schedule-post', 'event', 'mod-review', 'server-health', 'privacy'})
        playlist = next((command for command in commands if command['name'] == 'playlist'), {})
        help_command = next((command for command in commands if command['name'] == 'help'), {})
        help_options = [option['name'] for option in help_command.get('options', [])]
        report = next((command for command in commands if command['name'] == 'setup-report'), {})
        print(json.dumps({'command_count': len(commands), 'global_count': len(global_commands), 'duplicate_names': sorted(names & {c['name'] for c in global_commands}), 'missing_features': sorted(expected-names), 'playlist_subcommands': sorted(option['name'] for option in playlist.get('options', []) if option['type'] == 1), 'report_options': [{'name': o['name'], 'required': o.get('required', False)} for o in report.get('options', [])]}))
        print(json.dumps({'help_options': help_options, 'new_command_options': {c['name']: [{'name': o['name'], 'type': o['type'], 'required': o.get('required', False)} for o in c.get('options', [])] for c in commands if c['name'] in {'say', 'dm', 'media-panel', 'schedule-post', 'event', 'mod-review', 'server-health', 'privacy', 'report', 'verify'}}}))
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
