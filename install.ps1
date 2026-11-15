<#
.SYNOPSIS
    Install Imperatorium on Windows in one line:

        irm https://github.com/hypertetrahedron/imperatorium/releases/latest/download/install.ps1 | iex

.DESCRIPTION
    Downloads the imperatorium binary for this machine from the release, checks
    its SHA-256 against the release's manifest, puts it in
    %LOCALAPPDATA%\Programs\imperatorium, and runs `imperatorium install`,
    which asks its few questions and sets everything up. Nothing else is
    downloaded; no administrator rights are needed.

    Piped into iex it takes no parameters; these environment variables stand
    in for them:
        IMPERATORIUM_VERSION   a specific release, 0.4.0 or v0.4.0 (default: the newest)
        IMPERATORIUM_CHANNEL   stable (default) or preview
        IMPERATORIUM_RELEASES  another release folder (a mirror, or a test server)
    IMPERATORIUM_RELEASES wins over IMPERATORIUM_VERSION, which wins over
    IMPERATORIUM_CHANNEL.

    To read it first, save it (irm <the same URL> -OutFile install.ps1),
    read it, then run .\install.ps1.

    The whole script is one function, called on the last line: a download
    that is cut short runs nothing.

    Saved and run as a file, parameters work, and anything after them is
    passed to `imperatorium install` (for example --dry-run). -Version and
    -Channel and -From take the same values as the variables above.
#>
param(
    [string]$Channel = $(if ($env:IMPERATORIUM_CHANNEL) { $env:IMPERATORIUM_CHANNEL } else { 'stable' }),
    [string]$Version = $env:IMPERATORIUM_VERSION,
    [string]$From = $env:IMPERATORIUM_RELEASES,
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$InstallArgs = @()
)

function Install-Imperatorium {
    param([string]$Channel, [string]$Version, [string]$From, [string[]]$InstallArgs)
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is many times slower with it
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = 'https://github.com/hypertetrahedron/imperatorium/releases'
    $base = if ($From) { $From.TrimEnd('/') }
            elseif ($Version) { "$repo/download/v$($Version.TrimStart('v'))" }
            elseif ($Channel -eq 'preview') { "$repo/download/preview" }
            else { "$repo/latest/download" }

    $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64' -or $env:PROCESSOR_ARCHITEW6432 -eq 'ARM64') { 'aarch64' } else { 'x86_64' }
    $triple = "$arch-pc-windows-msvc"

    Write-Host "Imperatorium: reading $base/manifest.json"
    $manifest = Invoke-RestMethod -Uri "$base/manifest.json" -UseBasicParsing
    $asset = $manifest.assets.$triple
    if (-not $asset) { throw "this release ($($manifest.version)) has no build for $triple" }

    $dir = Join-Path $env:LOCALAPPDATA 'Programs\imperatorium'
    New-Item -ItemType Directory -Force $dir | Out-Null
    $exe = Join-Path $dir 'imperatorium.exe'
    $tmp = Join-Path $dir 'imperatorium.download'

    Write-Host "Imperatorium $($manifest.version): downloading $($asset.file)"
    Invoke-WebRequest -Uri "$base/$($asset.file)" -OutFile $tmp -UseBasicParsing
    $got = (Get-FileHash $tmp -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($got -ne $asset.sha256.ToLowerInvariant()) {
        Remove-Item $tmp -Force
        throw "SHA-256 mismatch for $($asset.file): got $got, the manifest says $($asset.sha256). Nothing was installed."
    }
    Write-Host "   SHA-256 checked: $got"

    # A running copy (a hook, the dispatcher) cannot be overwritten, but it can
    # be renamed out of the way.
    # (Named as `imperatorium update` names them, so its next run prunes it.)
    if (Test-Path $exe) {
        Rename-Item $exe ("imperatorium.old-" + (Get-Date -Format 'yyyyMMdd-HHmmss-fff')) -Force
    }
    Move-Item $tmp $exe
    return $exe
}

# Only the download is in the function; `imperatorium install` runs here, on
# the console itself, so its questions (which end without a newline) show as
# they are asked.
$exe = Install-Imperatorium -Channel $Channel -Version $Version -From $From -InstallArgs $InstallArgs
& $exe install @InstallArgs
$code = $LASTEXITCODE
# Run as a file, the exit code is the script's. Piped into iex, `exit` would
# close the person's PowerShell window (or end the script that ran iex), so
# it is only reported. Under iex $PSCommandPath is the caller's, if any.
$asFile = $PSCommandPath -and ((Get-Content -Raw -LiteralPath $PSCommandPath) -match 'function Install-Imperatorium')
if ($asFile) { exit $code }
if ($code) { Write-Warning "imperatorium install exited with $code" }
