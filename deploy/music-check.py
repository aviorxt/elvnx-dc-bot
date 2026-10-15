#!/usr/bin/env python3
"""Read-only local Lavalink health. Never prints credentials or track data."""
import json
import urllib.request
import urllib.parse
settings={}
with open('/etc/elvnx-bot/bot.env',encoding='utf-8') as source:
    for line in source:
        if '=' in line and not line.lstrip().startswith('#'):
            k,v=line.strip().split('=',1);settings[k]=v.strip().strip('"').strip("'")
url=settings['LAVALINK_URL'].rstrip('/')
parsed=urllib.parse.urlsplit(url)
if parsed.scheme!='http' or parsed.hostname!='127.0.0.1':raise SystemExit('Expected private local audio node')
try:
    req=urllib.request.Request(url+'/v4/stats',headers={'Authorization':settings['LAVALINK_PASSWORD']})
    with urllib.request.urlopen(req,timeout=10) as response:data=json.load(response)
    print(json.dumps({'reachable':True,'players':data.get('players'),'playing_players':data.get('playingPlayers'),'uptime_ms':data.get('uptime')}))
except Exception:
    raise SystemExit('Private audio health check failed; credentials were not printed')
