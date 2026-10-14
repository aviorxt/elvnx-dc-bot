import json
import sys
import urllib.parse
import urllib.request

password = open('/etc/lavalink/lavalink.env', encoding='utf-8').read().strip().split('=', 1)[1]
path = '/v4/info' if len(sys.argv) < 2 else '/v4/loadtracks?' + urllib.parse.urlencode({'identifier': sys.argv[1]})
if len(sys.argv) > 1 and sys.argv[1] == '--stats':
    path = '/v4/stats'
request = urllib.request.Request('http://127.0.0.1:2333' + path, headers={'Authorization': password})
with urllib.request.urlopen(request, timeout=30) as response:
    data = json.load(response)
if path == '/v4/stats':
    print(json.dumps({key: data[key] for key in ['players', 'playingPlayers', 'uptime', 'memory', 'cpu']}))
elif path == '/v4/info':
    print(json.dumps({'version': data['version']['semver'], 'sources': data['sourceManagers'], 'plugins': data['plugins']}))
else:
    result = {'load_type': data['loadType']}
    if data['loadType'] == 'search' and data['data']:
        result['first_result'] = data['data'][0]['info']['title']
    elif data['loadType'] == 'error':
        result['error'] = data['data'].get('message', 'Source unavailable')
    print(json.dumps(result))
