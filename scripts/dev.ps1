param([ValidateSet('check','test','fmt','clippy','build','run')][string]$Action='check')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
$toolRoot=Join-Path (Split-Path $projectRoot -Parent) 'work\rust-tools'
$cargoDir=Join-Path $env:USERPROFILE '.cargo\bin'
$rustLib=Join-Path $env:USERPROFILE '.rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained'
$env:PATH="$cargoDir;$rustLib;$env:PATH"
if (Test-Path (Join-Path $toolRoot 'w64devkit\bin\gcc.exe')) {
    $nativeBin=Join-Path $toolRoot 'w64devkit\bin'
    $env:PATH="$nativeBin;$env:PATH"
    $env:CC=Join-Path $nativeBin 'gcc.exe'
    $env:AR=Join-Path $nativeBin 'ar.exe'
    $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=Join-Path $rustLib 'x86_64-w64-mingw32-gcc.exe'
    $env:CARGO_ENCODED_RUSTFLAGS="-C$([char]31)dlltool=$(Join-Path $nativeBin 'dlltool.exe')"
}
Push-Location $projectRoot
try {
    if ($Action -eq 'fmt') { & cargo fmt --all }
    elseif ($Action -eq 'clippy') { & cargo clippy --all-targets -- -D warnings }
    else { & cargo $Action --locked }
    if ($LASTEXITCODE -ne 0) { throw "cargo $Action failed ($LASTEXITCODE)" }
} finally { Pop-Location }


