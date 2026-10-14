"""Test decoding without connecting to or playing in a Discord voice channel."""
import json
import time
import urllib.parse
import urllib.request
import websocket
import sys

password = open('/etc/lavalink/lavalink.env', encoding='utf-8').read().strip().split('=', 1)[1]
headers = {'Authorization': password, 'Content-Type': 'application/json'}

def request(path, payload=None, method=None):
    req = urllib.request.Request('http://127.0.0.1:2333' + path, headers=headers,
        data=json.dumps(payload).encode() if payload is not None else None, method=method)
    with urllib.request.urlopen(req, timeout=20) as response:
        return json.load(response) if response.status != 204 else None

loaded = request('/v4/loadtracks?' + urllib.parse.urlencode({'identifier': sys.argv[1] if len(sys.argv) > 1 else 'ytsearch:Beethoven'}))
if loaded['loadType'] not in ('search', 'track') or not loaded['data']:
    raise RuntimeError('No source test track was returned')
track = loaded['data'][0] if loaded['loadType'] == 'search' else loaded['data']
print(json.dumps({'title': track['info']['title']}), flush=True)
socket = websocket.create_connection('ws://127.0.0.1:2333/v4/websocket',
    header={'Authorization': password, 'User-Id': '1474265288223621215', 'Client-Name': 'ELVNX-Decoder-Probe/0.1.0'}, timeout=12)
session = None
try:
    while session is None:
        event = json.loads(socket.recv())
        if event.get('op') == 'ready':
            session = event['sessionId']
    path = f'/v4/sessions/{session}/players/1554095197959491595'
    request(path, {'track': {'encoded': track['encoded'], 'userData': {'requestId': 1}}}, 'PATCH')
    events = []
    deadline = time.monotonic() + 12
    socket.settimeout(2)
    while time.monotonic() < deadline:
        try:
            event = json.loads(socket.recv())
        except websocket.WebSocketTimeoutException:
            continue
        if event.get('op') == 'event':
            events.append(event.get('type'))
            if event.get('exception'):
                message = event['exception'].get('message', 'Source failed')
                print(json.dumps({'error': [line for line in message.splitlines() if line and not line.startswith('\t')][:12]}))
    print(json.dumps({'events': events, 'discord_voice_connected': False}))
finally:
    if session:
        request(f'/v4/sessions/{session}/players/1554095197959491595', method='DELETE')
    socket.close()
