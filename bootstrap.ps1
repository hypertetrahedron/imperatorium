<#
.SYNOPSIS
    Imperatorium, from nothing to running, on Windows.

.DESCRIPTION
    Checks for Python 3.9+, git and Claude Code, offers to install whatever
    is missing (winget for Python and git, Anthropic's own installer for
    Claude Code), clones the repository - or updates it if it is already
    there - and hands over to setup_machine.py, which asks the rest.

    Nothing is installed without asking, unless -Yes is given.

.EXAMPLE
    # From a checkout you already have:
    powershell -ExecutionPolicy Bypass -File bootstrap.ps1

.EXAMPLE
    # Straight from GitHub, with options:
    & ([scriptblock]::Create((irm https://raw.githubusercontent.com/hypertetrahedron/imperatorium/main/bootstrap.ps1))) -Dir D:\code\imperatorium
#>
[CmdletBinding()]
param(
    # Where to clone. Ignored when run from inside a checkout.
    [string]$Dir = (Join-Path $HOME 'imperatorium'),
    # Accept every default, here and in setup_machine.py.
    [switch]$Yes,
    # Anything else is passed on to setup_machine.py (e.g. --voice --no-service).
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$SetupArgs
)

$ErrorActionPreference = 'Stop'
$Repo = 'https://github.com/hypertetrahedron/imperatorium.git'

function Confirm-Step([string]$Question) {
    if ($Yes) { return $true }
    $answer = Read-Host "$Question [Y/n]"
    return (-not $answer) -or ($answer -match '^(y|yes)$')
}

function Update-SessionPath {
    # winget and the Claude installer change PATH for new shells only.
    $machine = [Environment]::GetEnvironmentVariable('Path', 'Machine')
    $user = [Environment]::GetEnvironmentVariable('Path', 'User')
    $env:Path = "$machine;$user;$(Join-Path $HOME '.local\bin')"
}

function Find-Python {
    # `python` may be the Microsoft Store stub, which prints nothing useful
    # and opens the Store, so a candidate only counts if it reports 3.9+.
    foreach ($candidate in @(@('py', '-3'), @('python'), @('python3'))) {
        $exe = $candidate[0]
        if (-not (Get-Command $exe -ErrorAction SilentlyContinue)) { continue }
        $rest = @($candidate | Select-Object -Skip 1)
        # No quote characters in the snippet: Windows PowerShell 5.1 strips
        # embedded double quotes from native arguments, which turns any
        # string literal into a syntax error and hides a working Python.
        try {
            $ok = & $exe @rest -c 'import sys; print(sys.version_info >= (3, 9))' 2>$null
        } catch { continue }
        if ("$ok".Trim() -eq 'True') {
            return , $candidate
        }
    }
    return $null
}

function Install-WithWinget([string]$Id, [string]$What) {
    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        throw "$What is missing and winget is not available. Install $What yourself, then run this again."
    }
    if (-not (Confirm-Step "$What is missing. Install it with winget ($Id)?")) {
        throw "$What is required. Install it, then run this again."
    }
    winget install --exact --id $Id --scope user --accept-package-agreements --accept-source-agreements
    Update-SessionPath
}

Write-Host "Imperatorium bootstrap`n"

# 1. Python
$python = Find-Python
if (-not $python) {
    Install-WithWinget 'Python.Python.3.12' 'Python 3.9+'
    $python = Find-Python
    if (-not $python) { throw 'Python was installed but is not on PATH yet. Open a new terminal and run this again.' }
}
Write-Host ("ok  python: {0}" -f ($python -join ' '))

# 2. git
if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Install-WithWinget 'Git.Git' 'git'
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) { throw 'git was installed but is not on PATH yet. Open a new terminal and run this again.' }
}
Write-Host 'ok  git'

# 3. Claude Code
if (-not (Get-Command claude -ErrorAction SilentlyContinue)) {
    if (-not (Confirm-Step "Claude Code is missing. Install it with Anthropic's installer (irm https://claude.ai/install.ps1 | iex)?")) {
        throw 'Claude Code is required: https://claude.com/claude-code'
    }
    Invoke-RestMethod https://claude.ai/install.ps1 | Invoke-Expression
    Update-SessionPath
    if (-not (Get-Command claude -ErrorAction SilentlyContinue)) { throw 'Claude Code was installed but is not on PATH yet. Open a new terminal and run this again.' }
    Write-Host 'Run `claude` once and log in before using Imperatorium.'
}
Write-Host 'ok  claude'

# 4. The checkout
$here = if ($PSScriptRoot -and (Test-Path (Join-Path $PSScriptRoot 'setup_machine.py'))) { $PSScriptRoot } else { $null }
if ($here) {
    $Dir = $here
    Write-Host "ok  checkout: $Dir"
} elseif (Test-Path (Join-Path $Dir '.git')) {
    Write-Host "updating $Dir"
    git -C $Dir pull --ff-only
} else {
    Write-Host "cloning into $Dir"
    git clone $Repo $Dir
}

# 5. Everything else is setup_machine.py's job.
$exe = $python[0]
$rest = @($python | Select-Object -Skip 1) + @((Join-Path $Dir 'setup_machine.py'))
if ($Yes) { $rest += '--yes' }
if ($SetupArgs) { $rest += $SetupArgs }
Push-Location $Dir
try { & $exe @rest } finally { Pop-Location }
exit $LASTEXITCODE
