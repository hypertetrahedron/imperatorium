<#
.SYNOPSIS
    Build a release on this machine and package it into dist\<version>.

.DESCRIPTION
    The Windows exe (C runtime linked in), the two Linux hosts' static musl
    builds, and - when -Voice is given - the dictation worker (Vulkan), then
    `imperatorium release-package` (the exe just built). The release workflow does the same per runner; this is for
    a local release folder to test the installers and `imperatorium update`
    against (serve dist\<version> over HTTP and point IMPERATORIUM_RELEASES
    or --from at it).

    The manifest is signed. With IMPERATORIUM_SIGNING_KEY set (the base64
    seed from `imperatorium release-keygen`) that key signs it. Without it, a
    throwaway key is made for this build only: its public half goes in
    <out>\TEST-PUBKEY.txt, and whoever tests the release sets
    IMPERATORIUM_RELEASE_PUBKEY to it, or their binary refuses the manifest.

.EXAMPLE
    powershell -File rust\release\build.ps1 -Version 0.3.1-preview.1
#>
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Out = '',
    [switch]$Voice,
    [string]$Notes = ''
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
        # The released worker is the Vulkan build (needs the LunarG SDK and
        # LIBCLANG_PATH). Built in a short folder: its shader generator's
        # compiler checks nest ~225 characters deep, so the folder itself
        # must stay under ~35 to keep inside Windows' 260.
        if (-not $env:VULKAN_SDK) { throw 'voice: set VULKAN_SDK (the LunarG Vulkan SDK)' }
        $vk = Join-Path $env:USERPROFILE '.ivk'
        & cmd.exe /c "`"$rust\voice\build-voice.cmd`" build --release --no-default-features --features vulkan --target-dir `"$vk`""
        if ($LASTEXITCODE) { throw 'voice build failed' }
        $assets += "voice-x86_64-pc-windows-msvc=$vk\release\imperatorium-voice.exe"
    }
} finally {
    Pop-Location
    Remove-Item Env:\IMPERATORIUM_BUILD_VERSION -ErrorAction SilentlyContinue
}
$exe = "$rust\target\release\imperatorium.exe"
$throwaway = $false
if (-not $env:IMPERATORIUM_SIGNING_KEY) {
    $key = (& $exe release-keygen) -join "`n" | ConvertFrom-Json
    if ($LASTEXITCODE -or -not $key.secret) { throw 'release-keygen failed' }
    $env:IMPERATORIUM_SIGNING_KEY = $key.secret
    $throwaway = $true
}
$extra = @(); if ($Notes) { $extra = @('--notes', $Notes) }
try {
    & $exe release-package --version $Version --out $Out --scripts $root --sign-key-env IMPERATORIUM_SIGNING_KEY @extra @assets
    if ($LASTEXITCODE) { throw 'packaging failed' }
} finally {
    if ($throwaway) { Remove-Item Env:\IMPERATORIUM_SIGNING_KEY -ErrorAction SilentlyContinue }
}
if ($throwaway) {
    Set-Content -Path (Join-Path $Out 'TEST-PUBKEY.txt') -Value $key.public -Encoding ascii
    Write-Host ''
    Write-Host 'Signed with a throwaway key (this build only). Testers must set'
    Write-Host "    IMPERATORIUM_RELEASE_PUBKEY=$($key.public)"
    Write-Host "(also in $Out\TEST-PUBKEY.txt) before installing or updating from it."
}
