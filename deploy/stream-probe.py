"""Read a small public test stream without playing in Discord."""
import json
import urllib.request

password = open('/etc/lavalink/lavalink.env', encoding='utf-8').read().strip().split('=', 1)[1]
req = urllib.request.Request('http://127.0.0.1:2333/youtube/stream/aqz-KE-bpKQ', headers={'Authorization':password})
with urllib.request.urlopen(req, timeout=30) as response:
    data = response.read(1024)
    print(json.dumps({'status':response.status, 'type':response.headers.get('Content-Type'), 'stream_bytes_received':len(data), 'discord_voice_connected':False}))
