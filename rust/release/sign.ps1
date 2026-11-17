<#
.SYNOPSIS
    Sign Windows binaries built on this machine (Azure Artifact Signing),
    then check each signature.

.DESCRIPTION
    Signs with the maintainer's signing script (signtool + the Artifact
    Signing dlib, authenticated by the Azure CLI's sign-in): the path in
    IMPERATORIUM_SIGN_SCRIPT, else C:\Tools\signing\sign.ps1. Releases are
    signed by release.yml on GitHub's runner instead (docs/releasing.md,
    "Code signing"); this is for builds made here - the live deploy, and
    build.ps1 -Sign.

    Sign before anything records a file's SHA-256 (release-package, the
    deploy's backups): signing changes the file.

    After signing, every file must have a Valid Authenticode signature whose
    signer is $Subject, or this fails.

    If signing fails with an authentication error, sign in again:
      & "C:\Program Files\Microsoft SDKs\Azure\CLI2\wbin\az.cmd" login --tenant 18b17d67-bd7b-45bc-abb0-5cc440040688

.EXAMPLE
    powershell -File rust\release\sign.ps1
    (rust\target\release\imperatorium.exe)

.EXAMPLE
    powershell -File rust\release\sign.ps1 C:\Users\me\.ivk\release\imperatorium-voice.exe
#>
[CmdletBinding(PositionalBinding = $false)]
param(
    # Only the files are positional: a path must never bind to -Subject.
    [Parameter(Position = 0, ValueFromRemainingArguments = $true)][string[]]$Files,
    # The certificate's subject, as the signing account's profile issues it.
    [string]$Subject = 'CN=Lester Wilhelm, O=Lester Wilhelm, L=Sandy Springs, S=ga, C=US',
    # Only check the signatures already on the files.
    [switch]$VerifyOnly
)
$ErrorActionPreference = 'Stop'
# Get-AuthenticodeSignature, from this PowerShell's own copy: Windows
# PowerShell started from PowerShell 7 inherits 7's module path and cannot
# autoload it from there.
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Security') -ErrorAction SilentlyContinue
$rust = Split-Path -Parent $PSScriptRoot
if (-not $Files) { $Files = @(Join-Path $rust 'target\release\imperatorium.exe') }
$Files = @($Files | ForEach-Object { (Resolve-Path $_).Path })

if (-not $VerifyOnly) {
    $script = if ($env:IMPERATORIUM_SIGN_SCRIPT) { $env:IMPERATORIUM_SIGN_SCRIPT } else { 'C:\Tools\signing\sign.ps1' }
    if (-not (Test-Path $script)) {
        throw "no signing script at $script (set IMPERATORIUM_SIGN_SCRIPT to it)"
    }
    # The signing dlib signs in through the Azure CLI, which it looks for on
    # PATH ("AzureCliCredential authentication failed: Azure CLI not
    # installed" otherwise); its installer does not always put it there.
    if (-not (Get-Command az -ErrorAction SilentlyContinue)) {
        $wbin = 'C:\Program Files\Microsoft SDKs\Azure\CLI2\wbin'
        if (-not (Test-Path (Join-Path $wbin 'az.cmd'))) { throw "the Azure CLI is not installed (no az on PATH or in $wbin)" }
        $env:PATH = "$wbin;$env:PATH"
    }
    & $script @Files
    if (-not $?) { throw "signing failed: $script" }
}

# Checked here too, not left to the script: a file that came out unsigned,
# or signed by someone else, never goes on.
$bad = @()
foreach ($f in $Files) {
    $sig = Get-AuthenticodeSignature -LiteralPath $f
    $who = if ($sig.SignerCertificate) { $sig.SignerCertificate.Subject } else { '(none)' }
    $stamped = [bool]$sig.TimeStamperCertificate
    Write-Host ("{0}: {1}, signer {2}, timestamped {3}" -f (Split-Path -Leaf $f), $sig.Status, $who, $stamped)
    if ($sig.Status -ne 'Valid' -or $who -ne $Subject -or -not $stamped) { $bad += $f }
}
if ($bad) { throw "not validly signed by $Subject (with a timestamp): $($bad -join ', ')" }
