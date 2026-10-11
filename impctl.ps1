<#
.SYNOPSIS
    Run impctl.py from anywhere: .\impctl.ps1 help

.DESCRIPTION
    Uses the project's venv when there is one (the test suites need its Agent
    SDK), otherwise the first Python on PATH. Every argument is passed
    through unchanged and impctl's exit code is returned.

.EXAMPLE
    .\impctl.ps1 status
    .\impctl.ps1 send -s central-control "run the tests"
#>
# No param() block on purpose: with one, PowerShell would try to bind impctl's
# own flags (-s, -p, -n) itself instead of passing them through in $args.

$script = Join-Path $PSScriptRoot 'impctl.py'
$python = @(
    (Join-Path $PSScriptRoot '.venv\Scripts\python.exe'),
    (Join-Path $PSScriptRoot '.venv/bin/python')
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $python) {
    # 'python' before 'python3': on Windows python3 is often the Store stub.
    foreach ($name in 'python', 'python3') {
        $found = Get-Command $name -CommandType Application -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($found) { $python = $found.Source; break }
    }
}
if (-not $python) {
    Write-Error 'impctl: no Python found - install Python 3.9+ or run setup_machine.py'
    exit 1
}

& $python $script @args
exit $LASTEXITCODE
