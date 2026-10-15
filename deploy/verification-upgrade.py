#!/usr/bin/env python3
"""Bind and refresh an existing bot-owned verification panel. Run as root once."""
import argparse
import json
import secrets
import datetime
import sqlite3
import urllib.request
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--channel', required=True)
    parser.add_argument('--message', required=True)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    env = {}
    for line in Path('/etc/elvnx-bot/bot.env').read_text().splitlines():
        if '=' in line and not line.lstrip().startswith('#'):
            key, value = line.strip().split('=', 1)
            env[key] = value.strip().strip('"').strip("'")

    def api(path, payload=None, png=None):
        headers = {'Authorization': 'Bot ' + env['DISCORD_TOKEN'], 'User-Agent': 'ELVNX/0.4'}
        body = None
        if png is not None:
            boundary = secrets.token_hex(24)
            headers['Content-Type'] = 'multipart/form-data; boundary=' + boundary
            body = (f'--{boundary}\r\nContent-Disposition: form-data; name="payload_json"\r\nContent-Type: application/json\r\n\r\n'.encode()
                    + json.dumps(payload).encode() + f'\r\n--{boundary}\r\nContent-Disposition: form-data; name="files[0]"; filename="verification.png"\r\nContent-Type: image/png\r\n\r\n'.encode()
                    + png + f'\r\n--{boundary}--\r\n'.encode())
        request = urllib.request.Request('https://discord.com/api/v10/' + path, data=body,
                                         headers=headers, method='PATCH' if payload is not None else 'GET')
        with urllib.request.urlopen(request, timeout=25) as response:
            return json.load(response)

    guild = env['DISCORD_GUILD_ID']
    bot = api('users/@me')['id']
    db = sqlite3.connect('/var/lib/elvnx-bot/data/elvnx.db', timeout=15)
    cfg = json.loads(db.execute('SELECT value FROM settings WHERE guild=?', (guild,)).fetchone()[0])
    if db.execute('SELECT 1 FROM verification_policy WHERE guild=?', (guild,)).fetchone():
        raise ValueError('An explicit verification policy already exists; use /verify Setup instead')
    role_id = str(cfg['verification_role'])
    roles = {r['id']: r for r in api('guilds/' + guild + '/roles')}
    role = roles[role_id]
    assert role_id != guild and not role['managed'] and int(role['permissions']) == 0, 'Unsafe whitelist role'
    assert cfg.get('autorole') != int(role_id), 'Autorole bypass must be removed first'
    assert env.get('ENABLE_MEMBER_EVENTS') == 'true', 'Member events required'
    member = api(f'guilds/{guild}/members/{bot}')
    highest = max((roles[r]['position'], -int(r)) for r in member['roles'])
    assert highest > (role['position'], -int(role_id)), 'Whitelist must be below the bot'
    permissions = int(roles[guild]['permissions'])
    for r in member['roles']:
        permissions |= int(roles[r]['permissions'])
    assert permissions & (8 | (1 << 28)), 'Bot needs Manage Roles'
    review = str(cfg['report_channel'])
    channel = api('channels/' + review)
    assert channel['guild_id'] == guild and channel['type'] == 0
    overwrites = channel.get('permission_overwrites', [])
    assert any(o['id'] == guild and int(o['deny']) & 1024 for o in overwrites), 'Review must deny everyone View Channel'
    staff = 8 | (1 << 5) | (1 << 40) | (1 << 13)
    for o in overwrites:
        if not int(o['allow']) & 1024:
            continue
        if o['type'] == 0:
            r = roles[o['id']]
            assert int(r['permissions']) & staff or r.get('tags', {}).get('bot_id') == bot, 'Non-staff review audience'
        else:
            assert o['id'] in (bot, api('guilds/' + guild)['owner_id']), 'Review member overwrite needs manual inspection'
    panel_channel = api('channels/' + args.channel)
    assert panel_channel['guild_id'] == guild and panel_channel['type'] == 0
    path = f'channels/{args.channel}/messages/{args.message}'
    old = api(path)
    assert old['author']['id'] == bot
    assert any(b.get('custom_id') == 'elvnx:verify:' + role_id for row in old.get('components', []) for b in row.get('components', [])), 'Wrong panel'
    policy = dict(role=int(role_id), channel=int(args.channel), message=int(args.message), review=int(review),
                  account_days=7, wait_minutes=10, revision=secrets.token_hex(16))
    if not args.apply:
        print(json.dumps({'ready': True, 'policy': policy, 'existing_member_roles_changed': False}))
        return
    ids = json.loads((Path(__file__).parent.parent / 'assets/emojis/manifest.json').read_text())

    def button(custom_id, label, icon, style):
        return dict(type=2, style=style, custom_id=custom_id, label=label,
                    emoji={'id': str(ids[icon]), 'name': 'elvnx_' + icon})

    embed = dict(title='ELVNX / VERIFICATION', color=0x111111,
                 author={'name': 'ELVNX / 11X', 'icon_url': f'https://cdn.discordapp.com/emojis/{ids["shield"]}.png'},
                 description='Welcome to ELVNX. Read the rules, then verify.\n\nComplete a one-time visual code. Members wait **10 minutes** after joining. Accounts younger than **7 days**, repeated rejoins and join surges require staff review.\n\nThese flags are not proof of an alternate account. No IP, email, phone, password or Discord login code is requested. Choose **Staff help** for an accessible alternative.\n\nChallenges expire after two minutes. Security review records are retained for up to 30 days.',
                 image={'url': 'attachment://verification.png'},
                 footer={'text': '11X • account screening, not proof of identity'})
    payload = dict(content='', embeds=[embed], allowed_mentions={'parse': []},
                   attachments=[{'id': 0, 'filename': 'verification.png'}],
                   components=[{'type': 1, 'components': [button('elvnx:verify:' + role_id, 'Verify access', 'shield', 3), button('elvnx:verification:help', 'Staff help', 'review', 2)]}])
    backup_dir=Path('/var/backups/elvnx-bot')
    backup_dir.mkdir(mode=0o700,parents=True,exist_ok=True)
    backup_dir.chmod(0o700)
    backup=backup_dir / ('before-verification-'+datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S')+'.db')
    with sqlite3.connect(backup) as target:
        db.backup(target)
    backup.chmod(0o600)
    # Bind first; a failed HTTP refresh still leaves the old Verify button protected.
    with db:
        db.execute('INSERT INTO verification_policy(guild,value) VALUES(?,?)', (guild, json.dumps(policy)))
        cfg['self_roles'] = [r for r in cfg.get('self_roles', []) if r != int(role_id)]
        db.execute('UPDATE settings SET value=? WHERE guild=?', (json.dumps(cfg), guild))
    api(path, payload, (Path(__file__).parent.parent / 'previews/verification.png').read_bytes())
    updated = api(path)
    assert len(updated['components'][0]['components']) == 2
    print(json.dumps({'upgraded': True, 'channel': args.channel, 'message': args.message, 'account_days': 7, 'join_wait_minutes': 10, 'existing_member_roles_changed': False}))


if __name__ == '__main__':
    main()
