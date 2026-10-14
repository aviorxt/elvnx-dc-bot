param([string]$Binary='target\debug\elvnx-bot.exe')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
Push-Location $projectRoot
try {
    $resolvedBinary=(Resolve-Path -LiteralPath $Binary).Path
    $env:BOT_SUPERVISED='true'
    do {
        & $resolvedBinary
        $botExit=$LASTEXITCODE
        if ($botExit -eq 75) { Write-Host 'Owner requested restart; restarting in 3 seconds.'; Start-Sleep -Seconds 3 }
    } while ($botExit -eq 75)
    if ($botExit -ne 0) { Write-Error "Bot stopped with exit $botExit. Fix the error before restarting." }
} finally { Pop-Location }
