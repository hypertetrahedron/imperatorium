<#
.SYNOPSIS
    Build a release on this machine and package it into dist\<version>.

.DESCRIPTION
    The Windows exe (C runtime linked in), the two Linux hosts' static musl
    builds, and - when -Voice is given - the dictation worker, then
    package.py. The release workflow does the same per runner; this is for
    a local release folder to test the installers and `imperatorium update`
    against (serve dist\<version> over HTTP and point IMPERATORIUM_RELEASES
    or --from at it).

.EXAMPLE
    powershell -File rust\release\build.ps1 -Version 0.3.1-preview.1
#>
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Out = '',
    [switch]$Voice
)
$ErrorActionPreference = 'Stop'
$rust = Split-Path -Parent $PSScriptRoot
$root = Split-Path -Parent $rust
if (-not $Out) { $Out = Join-Path $root "dist\$Version" }
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:IMPERATORIUM_BUILD_VERSION = $Version.TrimStart('v')

Push-Location $rust
try {
    cargo build --release -p imperatorium
    if ($LASTEXITCODE) { throw 'windows build failed' }
    foreach ($t in 'x86_64-unknown-linux-musl', 'aarch64-unknown-linux-musl') {
        cargo build --release -p imperatorium --no-default-features --target $t
        if ($LASTEXITCODE) { throw "$t build failed" }
    }
    $assets = @(
        "x86_64-pc-windows-msvc=$rust\target\release\imperatorium.exe",
        "x86_64-unknown-linux-musl=$rust\target\x86_64-unknown-linux-musl\release\imperatorium",
        "aarch64-unknown-linux-musl=$rust\target\aarch64-unknown-linux-musl\release\imperatorium"
    )
    if ($Voice) {
        & cmd.exe /c "`"$rust\voice\build-voice.cmd`""
        if ($LASTEXITCODE) { throw 'voice build failed' }
        $assets += "voice-x86_64-pc-windows-msvc=$rust\voice\target\release\imperatorium-voice.exe"
    }
} finally {
    Pop-Location
    Remove-Item Env:\IMPERATORIUM_BUILD_VERSION -ErrorAction SilentlyContinue
}
python (Join-Path $PSScriptRoot 'package.py') --version $Version --out $Out @assets
