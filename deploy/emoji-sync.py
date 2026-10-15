"""Idempotently upload only ELVNX UI emoji; keep the bot token server-side."""
import base64
import json
import sys
import time
import urllib.request
import urllib.error
from pathlib import Path

root=Path('/home/ubuntu/elvnx-build')
settings={}
for line in Path('/etc/elvnx-bot/bot.env').read_text().splitlines():
    if '=' in line and not line.lstrip().startswith('#'):
        key,value=line.split('=',1);settings[key]=value.strip().strip('"').strip("'")
def request(path,payload=None):
    body=None if payload is None else json.dumps(payload).encode()
    for attempt in range(4):
        req=urllib.request.Request('https://discord.com/api/v10/'+path,data=body,headers={'Authorization':'Bot '+settings['DISCORD_TOKEN'],'Content-Type':'application/json','User-Agent':'ELVNX-Emoji-Setup/1.0'})
        try:
            with urllib.request.urlopen(req,timeout=30) as response:return json.load(response)
        except urllib.error.HTTPError as error:
            if error.code==429 and attempt<3:
                value=json.loads(error.read());time.sleep(min(float(value.get('retry_after',2)),10));continue
            raise RuntimeError(f'Discord emoji operation failed with HTTP {error.code}') from None
app=request('applications/@me')['id']
existing=request(f'applications/{app}/emojis')['items']
if '--inspect' in sys.argv:
    print(json.dumps({'application':app,'application_emoji_count':len(existing),'animated_count':sum(bool(e.get('animated')) for e in existing),'elvnx_names':[e['name'] for e in existing if e['name'].startswith('elvnx_')]}));sys.exit(0)
manifest={};animated={};created=0
for path in sorted((root/'assets/emojis').glob('elvnx_*.png'))+sorted((root/'assets/emojis').glob('elvnx_live_*.gif')):
    name=path.stem
    found=next((e for e in existing if e['name']==name),None)
    if found is None:
        data=path.read_bytes()
        if len(data)>256*1024:raise RuntimeError('Emoji asset exceeds Discord upload limit')
        found=request(f'applications/{app}/emojis',{'name':name,'image':'data:image/'+('gif' if path.suffix=='.gif' else 'png')+';base64,'+base64.b64encode(data).decode()});created+=1
    if path.suffix=='.gif':
        if not found.get('animated'):raise RuntimeError('Expected an animated Discord emoji')
        animated[name.removeprefix('elvnx_live_')]=int(found['id'])
    else:manifest[name.removeprefix('elvnx_')]=int(found['id'])
if len(manifest)!=40 or len(animated)!=40:raise RuntimeError('Expected exactly 40 static and animated local emoji assets')
(root/'assets/emojis/manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
(root/'assets/emojis/animated.json').write_text(json.dumps(animated,indent=2)+'\n')
print(json.dumps({'application':app,'custom_emoji_count':len(manifest),'animated_count':len(animated),'created':created}))
