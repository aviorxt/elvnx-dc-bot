param(
    [string]$ServerAddress='13.203.104.97',
    [string]$SshUser='ubuntu',
    [string]$KeyPath='C:\Users\IRSHAD\Downloads\Others\aws-legion-ssh.pem'
)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
$envPath=Join-Path $projectRoot '.env'
if (!(Test-Path -LiteralPath $envPath)) {throw 'Add DISCORD_TOKEN to the ignored local .env first.'}
$values=@{}
foreach ($line in Get-Content -LiteralPath $envPath) {
    if ($line -match '^\s*([A-Z_]+)\s*=\s*(.*?)\s*$') {
        $values[$Matches[1]]=$Matches[2].Trim('"').Trim("'")
    }
}
$token=$values['DISCORD_TOKEN']
if ([string]::IsNullOrWhiteSpace($token) -or $token.Length -lt 30) {throw 'A valid dedicated bot token is required.'}
$guild='1554095197959491595'
Write-Host 'Preparing private environment. Discord checks run from the VPS.'
$memberEvents=if ($values['ENABLE_MEMBER_EVENTS'] -eq 'true') {'true'}else{'false'}
$contentEvents=if ($values['ENABLE_MESSAGE_CONTENT'] -eq 'true') {'true'}else{'false'}
$presenceEvents=if ($values['ENABLE_PRESENCE_EVENTS'] -eq 'true') {'true'}else{'false'}
$environment=@(
    "DISCORD_TOKEN=$token",
    "DISCORD_GUILD_ID=$guild",
    'DATABASE_URL=sqlite://data/elvnx.db',
    'BOT_SUPERVISED=true',
    'REGISTER_COMMANDS=true',
    "ENABLE_MEMBER_EVENTS=$memberEvents",
    "ENABLE_MESSAGE_CONTENT=$contentEvents",
    "ENABLE_PRESENCE_EVENTS=$presenceEvents",
    'RUST_LOG=elvnx_bot=info,warn'
) -join "`n"
$temporary=Join-Path $env:TEMP ("elvnx-"+[guid]::NewGuid().ToString()+'.env')
$destination="$SshUser@$ServerAddress"
try {
    [IO.File]::WriteAllText($temporary,$environment+"`n",[Text.UTF8Encoding]::new($false))
    & ssh -i $KeyPath -o BatchMode=yes $destination 'umask 077; mkdir -p /home/ubuntu/.elvnx-deploy; chmod 720 /home/ubuntu/.elvnx-deploy'
    if ($LASTEXITCODE -ne 0) {throw 'Could not create private deployment directory'}
    & scp -i $KeyPath $temporary "${destination}:/home/ubuntu/.elvnx-deploy/bot.env"
    if ($LASTEXITCODE -ne 0) {throw 'Encrypted environment transfer failed'}
    & ssh -i $KeyPath -o BatchMode=yes $destination 'sudo install -o root -g root -m 600 /home/ubuntu/.elvnx-deploy/bot.env /etc/elvnx-bot/bot.env && rm -f /home/ubuntu/.elvnx-deploy/bot.env && (test ! -f /etc/lavalink/lavalink.env || sudo python3 /opt/lavalink/lavalink-secrets.py) && sudo python3 /opt/elvnx-bot/discord-check.py && sudo systemctl reset-failed elvnx-bot && sudo systemctl restart elvnx-bot'
    if ($LASTEXITCODE -ne 0) {throw 'Service startup failed'}
    $registered=$false
    for ($attempt=0;$attempt -lt 12;$attempt++) {
        Start-Sleep -Seconds 5
        try {
            $status=& ssh -i $KeyPath -o BatchMode=yes $destination 'sudo python3 /opt/elvnx-bot/discord-check.py commands'
            if ($LASTEXITCODE -eq 0 -and ($status | ConvertFrom-Json).command_count -eq 100) {$registered=$true;break}
        } catch { }
    }
    if (!$registered) {throw 'Service started, but registration of all 100 commands is unconfirmed. Inspect journalctl; do not print its environment.'}
    & ssh -i $KeyPath -o BatchMode=yes $destination "sudo sed -i 's/^REGISTER_COMMANDS=true$/REGISTER_COMMANDS=false/' /etc/elvnx-bot/bot.env && sudo systemctl is-active elvnx-bot"
    if ($LASTEXITCODE -ne 0) {throw 'Service is not active'}
    Write-Host 'Bot is active and all 100 guild commands are registered. Registration on reboot is disabled.'
} finally {
    if (Test-Path -LiteralPath $temporary) {Remove-Item -LiteralPath $temporary -Force}
    & ssh -i $KeyPath -o BatchMode=yes $destination 'rm -f /home/ubuntu/.elvnx-deploy/bot.env'
    $token=$null
    $environment=$null
    $headers=$null
}
