#!/usr/bin/env python3
"""Provision the two user-authorized Legacy music channels; no credentials printed."""
import json, sys, urllib.request, urllib.error, sqlite3, subprocess, os, pwd
settings={}
with open('/etc/elvnx-bot/bot.env',encoding='utf-8') as source:
    for line in source:
        if '=' in line and not line.lstrip().startswith('#'):
            k,v=line.strip().split('=',1);settings[k]=v.strip().strip('"').strip("'")
guild=settings['DISCORD_GUILD_ID'];parent='1554097393463599115'
def request(path,body=None):
    req=urllib.request.Request('https://discord.com/api/v10/'+path,data=json.dumps(body).encode() if body is not None else None,headers={'Authorization':'Bot '+settings['DISCORD_TOKEN'],'Content-Type':'application/json','User-Agent':'DiscordBot (https://www.avrxt.dev, 0.8.0)'})
    with urllib.request.urlopen(req,timeout=20) as response:return json.load(response)
def channels(create):
    all_channels=request('guilds/'+guild+'/channels');category=next(c for c in all_channels if c['id']==parent and c['type']==4)
    out={}
    voice=next(c for c in all_channels if c['id']=='1559607582090461326' and c['type']==2 and c.get('parent_id')==parent)
    bot=request('users/@me')['id'];member=request('guilds/'+guild+'/members/'+bot);roles=request('guilds/'+guild+'/roles')
    permissions=0
    for role in roles:
        if role['id']==guild or role['id'] in member['roles']:permissions|=int(role['permissions'])
    if not permissions&8:
        overrides=voice.get('permission_overwrites',[])
        for overwrite in overrides:
            if overwrite['type']==0 and overwrite['id']==guild:permissions=(permissions&~int(overwrite['deny']))|int(overwrite['allow'])
        deny=allow=0
        for overwrite in overrides:
            if overwrite['type']==0 and overwrite['id'] in member['roles']:deny|=int(overwrite['deny']);allow|=int(overwrite['allow'])
        permissions=(permissions&~deny)|allow
        for overwrite in overrides:
            if overwrite['type']==1 and overwrite['id']==bot:permissions=(permissions&~int(overwrite['deny']))|int(overwrite['allow'])
        if permissions&((1<<10)|(1<<20)|(1<<21))!=((1<<10)|(1<<20)|(1<<21)):raise RuntimeError('Bot lacks voice permissions in Kona')
    for name,key,topic in [('music-requests','music_request_channel','Join voice, then post a song title or supported music link. Use Add track for a private form.'),('playlist-fav','music_favorite_channel','Post one supported music link to save it to shared favorites. Saving does not start playback.')]:
        found=[c for c in all_channels if c['name']==name and c['type']==0 and c.get('parent_id')==parent]
        if len(found)>1:raise RuntimeError('Duplicate music channels require review')
        if found:channel=found[0]
        elif create:channel=request('guilds/'+guild+'/channels',{'name':name,'type':0,'parent_id':parent,'permission_overwrites':category['permission_overwrites'],'rate_limit_per_user':5,'topic':topic})
        else:raise RuntimeError('Music channel missing')
        out[key]=channel['id']
    return out,all_channels
try:
    if '--inspect' in sys.argv:
        print(json.dumps({'voice_channels':[{'id':c['id'],'name':c['name'],'parent_id':c.get('parent_id')} for c in request('guilds/'+guild+'/channels') if c['type']==2]}))
    elif '--configure' in sys.argv:
        if subprocess.run(['systemctl','is-active','--quiet','elvnx-bot']).returncode==0:raise RuntimeError('Stop the bot before changing persisted configuration')
        ids,_=channels(False)
        with sqlite3.connect('/var/lib/elvnx-bot/data/elvnx.db') as db:
            row=db.execute('SELECT value FROM settings WHERE guild=?',(guild,)).fetchone();cfg=json.loads(row[0]) if row else {};cfg.update({k:int(v) for k,v in ids.items()});cfg['music_stay_channel']=1559607582090461326;db.execute('INSERT INTO settings(guild,value) VALUES(?,?) ON CONFLICT(guild) DO UPDATE SET value=excluded.value',(guild,json.dumps(cfg)));db.commit()
        user=pwd.getpwnam('elvnx')
        for suffix in ['', '-wal','-shm']:
            path='/var/lib/elvnx-bot/data/elvnx.db'+suffix
            if os.path.exists(path):os.chown(path,user.pw_uid,user.pw_gid)
        print(json.dumps({'configured':ids,'stay_mode':'Kona / enabled'}))
    elif '--create' in sys.argv:
        ids,_=channels(True);print(json.dumps({'channels':ids,'parent':parent,'permissions':'copied from Legacy','slowmode_seconds':5}))
    else:raise RuntimeError('Choose --inspect, --create or --configure')
except urllib.error.HTTPError as error:
    print(json.dumps({'error':'Discord API request failed','status':error.code}));sys.exit(1)
except Exception as error:
    print(json.dumps({'error':type(error).__name__,'details':'Music setup did not complete. No credentials printed.'}));sys.exit(1)
