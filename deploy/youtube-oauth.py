"""Official youtube-source device flow. Never prints access/refresh tokens."""
import json
import os
from pathlib import Path
import sys
import time
import urllib.error
import urllib.request
import uuid

# Public TV-client constants used by lavalink-devs/youtube-source itself.
CLIENT_ID = '861556708454-d6dlm3lh05idd8npek18k6be8ba3oc68.apps.googleusercontent.com'
CLIENT_SECRET = 'SboVhoG9s0rNafixCSGGKXAT'
PENDING = Path('/etc/lavalink/oauth-pending.json')

def post(url, payload):
    req = urllib.request.Request(url, data=json.dumps(payload).encode(), headers={'Content-Type': 'application/json'})
    try:
        with urllib.request.urlopen(req, timeout=20) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        try:
            return json.load(error)
        except Exception:
            return {'error': 'http_' + str(error.code)}

if sys.argv[1] == 'begin':
    data = post('https://www.youtube.com/o/oauth2/device/code', {'client_id': CLIENT_ID,
        'scope': 'http://gdata.youtube.com https://www.googleapis.com/auth/youtube',
        'device_id': uuid.uuid4().hex, 'device_model': 'ytlr::'})
    if 'device_code' not in data:
        print(json.dumps({'status': 'failed', 'error': data.get('error', 'Google request failed')}))
        sys.exit(1)
    data['expires_at'] = time.time() + data['expires_in']
    data['last_poll'] = 0
    PENDING.write_text(json.dumps(data))
    os.chmod(PENDING, 0o600)
    print(json.dumps({'verification_url': data['verification_url'], 'user_code': data['user_code'], 'expires_in': data['expires_in']}))
elif sys.argv[1] == 'poll':
    pending = json.loads(PENDING.read_text())
    if time.time() > pending['expires_at']:
        print(json.dumps({'status': 'expired'}))
        sys.exit(1)
    if time.time() - pending['last_poll'] < pending.get('interval', 5):
        print(json.dumps({'status': 'waiting'}))
        sys.exit(0)
    pending['last_poll'] = time.time()
    PENDING.write_text(json.dumps(pending))
    result = post('https://www.youtube.com/o/oauth2/token', {'client_id': CLIENT_ID,
        'client_secret': CLIENT_SECRET, 'code': pending['device_code'],
        'grant_type': 'http://oauth.net/grant_type/device/1.0'})
    if 'refresh_token' in result:
        target = Path('/etc/lavalink/youtube.env')
        target.write_text('YOUTUBE_REFRESH_TOKEN=' + result['refresh_token'] + '\n')
        os.chmod(target, 0o600)
        PENDING.unlink()
        print(json.dumps({'status': 'authorized', 'stored': 'root-only VPS environment'}))
    else:
        error = result.get('error', 'unknown')
        if error == 'slow_down':
            pending['interval'] = pending.get('interval', 5) + 5
            PENDING.write_text(json.dumps(pending))
        print(json.dumps({'status': 'waiting' if error in ('authorization_pending', 'slow_down') else 'failed', 'error': error}))
