<#
.SYNOPSIS
    Register Imperatorium to start at logon and stay up.

.DESCRIPTION
    The dispatcher has to outlive any single Claude Code session. Started from
    a session's shell it becomes a child of that session and dies with it,
    taking the board and the Stream Deck down too - which is exactly what
    happened on 2026-09-17.

    Two user-level tasks, deliberately shaped like the ClaudeMetricsReceiver
    task already on this machine: logon trigger, pythonw so there is no console
    window, restart three times at one-minute intervals, no execution time
    limit. No elevation required - these run as the current user.

    The Stream Deck task is separate because it needs the project venv, and
    because killing it must never take the dispatcher with it.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File service\tasks.ps1 -Action status
    powershell -ExecutionPolicy Bypass -File service\tasks.ps1 -Action install
    powershell -ExecutionPolicy Bypass -File service\tasks.ps1 -Action uninstall
#>
[CmdletBinding()]
param(
    [ValidateSet('install', 'uninstall', 'status', 'restart')]
    [string]$Action = 'status',

    # Skip the Stream Deck task, for a machine with no deck attached.
    [switch]$NoStreamDeck,

    # The pythonw.exe for the hotkey (and the dispatcher when there is no
    # venv). setup_machine.py passes the interpreter it was run with.
    [string]$PythonW = ''
)

$ErrorActionPreference = 'Stop'

$ProjectDir = Split-Path -Parent $PSScriptRoot
$DispatcherTask = 'CentralControlDispatcher'
$StreamDeckTask = 'CentralControlStreamDeck'
$PaletteTask = 'CentralControlHotkey'

$SystemPythonW = if ($PythonW) { $PythonW } else {
    Join-Path $env:LOCALAPPDATA 'Programs\Python\Python312\pythonw.exe'
}
$VenvPythonW = Join-Path $ProjectDir '.venv\Scripts\pythonw.exe'

function Resolve-Interpreter {
    param([string]$Preferred, [string]$What)
    if (Test-Path $Preferred) { return $Preferred }
    $fallback = (Get-Command pythonw.exe -ErrorAction SilentlyContinue).Source
    if ($fallback) {
        Write-Warning "$What not found at $Preferred; using $fallback"
        return $fallback
    }
    throw "No pythonw.exe found for $What (looked at $Preferred)."
}

function New-CcTask {
    param(
        [string]$Name,
        [string]$Exe,
        [string]$Arguments,
        [int]$DelaySeconds = 0
    )

    $action = New-ScheduledTaskAction -Execute $Exe -Argument $Arguments `
        -WorkingDirectory $ProjectDir

    $trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
    if ($DelaySeconds -gt 0) {
        # Give the dispatcher a moment to bind its port first. The surface
        # tolerates it being down, but starting into a working endpoint means
        # the keys are painted correctly on the first frame.
        $trigger.Delay = 'PT{0}S' -f $DelaySeconds
    }

    # RestartCount alone does not bring back a process that was killed: on
    # 2026-09-17 the dispatcher was force-killed, the task recorded
    # 0xFFFFFFFF, went to Ready and scheduled nothing. Nor is it enough to
    # hang a repetition off the logon trigger - that trigger's window opened
    # at logon, so a task registered later never repeats until the next one.
    # The heartbeat therefore gets its own trigger, starting now and repeating
    # forever. With MultipleInstances=IgnoreNew each repeat is discarded at no
    # cost while the task runs, and the first one after a crash restarts it.
    $heartbeat = New-ScheduledTaskTrigger -Once -At (Get-Date).AddSeconds(30) `
        -RepetitionInterval (New-TimeSpan -Minutes 1)
    $triggers = @($trigger, $heartbeat)

    # IgnoreNew matters: the dispatcher refuses to share its port, so a second
    # instance would fail noisily at bind rather than quietly split traffic.
    $settings = New-ScheduledTaskSettingsSet `
        -AllowStartIfOnBatteries `
        -DontStopIfGoingOnBatteries `
        -DontStopOnIdleEnd `
        -RestartCount 3 `
        -RestartInterval (New-TimeSpan -Minutes 1) `
        -MultipleInstances IgnoreNew `
        -ExecutionTimeLimit (New-TimeSpan -Seconds 0) `
        -StartWhenAvailable

    $principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" `
        -LogonType Interactive -RunLevel Limited

    if (Get-ScheduledTask -TaskName $Name -ErrorAction SilentlyContinue) {
        Unregister-ScheduledTask -TaskName $Name -Confirm:$false
    }
    Register-ScheduledTask -TaskName $Name -Action $action -Trigger $triggers `
        -Settings $settings -Principal $principal `
        -Description 'Imperatorium - see and dispatch to Claude Code sessions' | Out-Null
    Write-Host ("registered {0}" -f $Name)
    Write-Host ("  {0} {1}" -f $Exe, $Arguments)
}

function Stop-Existing {
    # Detached processes started by hand do not belong to the task, and would
    # hold the port against it.
    # python.exe too, not just pythonw: a palette started by hand from a
    # terminal holds the global hotkey against the one the task starts.
    Get-CimInstance Win32_Process -Filter "Name='pythonw.exe' or Name='python.exe'" |
        Where-Object { $_.CommandLine -match 'ccontrol\.dispatcher|streamdeck_surface|hotkey\.py|palette\.py' } |
        ForEach-Object {
            Write-Host ("stopping stray pid {0}" -f $_.ProcessId)
            Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
        }
}

function Show-Status {
    foreach ($name in @($DispatcherTask, $StreamDeckTask, $PaletteTask)) {
        $task = Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
        if (-not $task) {
            Write-Host ("{0,-28} not registered" -f $name)
            continue
        }
        $info = Get-ScheduledTaskInfo -TaskName $name
        Write-Host ("{0,-28} {1,-10} last={2} result={3}" -f `
                $name, $task.State, $info.LastRunTime, $info.LastTaskResult)
    }
    try {
        $health = Invoke-RestMethod -Uri 'http://127.0.0.1:8792/api/health' -TimeoutSec 4
        Write-Host ("dispatcher responding, pid {0}" -f $health.pid)
    } catch {
        Write-Host 'dispatcher NOT responding on 127.0.0.1:8792'
    }
}

switch ($Action) {
    'install' {
        # Unregister first, then kill strays. The other way round leaves a
        # window in which the still-registered task's one-minute heartbeat
        # restarts the very process that was just killed, and the reinstall
        # ends with two of them.
        foreach ($name in @($DispatcherTask, $StreamDeckTask, $PaletteTask)) {
            if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) {
                Stop-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
                Unregister-ScheduledTask -TaskName $name -Confirm:$false
            }
        }
        Stop-Existing
        # The venv interpreter: the conversation view reads sessions through
        # the Agent SDK, which lives there. Without the venv the dispatcher
        # still runs; the conversation pane just reports it cannot read.
        $dispatcherExe = if (Test-Path $VenvPythonW) { $VenvPythonW }
                         else { Resolve-Interpreter $SystemPythonW 'dispatcher' }
        New-CcTask -Name $DispatcherTask -Exe $dispatcherExe `
            -Arguments '-m ccontrol.dispatcher'
        if (-not $NoStreamDeck) {
            if (Test-Path $VenvPythonW) {
                New-CcTask -Name $StreamDeckTask -Exe $VenvPythonW `
                    -Arguments 'streamdeck_surface.py' -DelaySeconds 15
            } else {
                Write-Warning "no venv at $VenvPythonW; skipping the Stream Deck task"
            }
        }
        # Only a hotkey now: the window carries the prompt box, the session
        # list and the microphone, so nothing else needs to be resident.
        New-CcTask -Name $PaletteTask -Exe (Resolve-Interpreter $SystemPythonW 'hotkey') `
            -Arguments 'hotkey.py' -DelaySeconds 20
        Start-ScheduledTask -TaskName $DispatcherTask
        if (-not $NoStreamDeck -and (Get-ScheduledTask -TaskName $StreamDeckTask -ErrorAction SilentlyContinue)) {
            Start-Sleep -Seconds 3
            Start-ScheduledTask -TaskName $StreamDeckTask
        }
        Start-Sleep -Seconds 2
        Start-ScheduledTask -TaskName $PaletteTask
        Start-Sleep -Seconds 3
        Show-Status
    }
    'uninstall' {
        foreach ($name in @($DispatcherTask, $StreamDeckTask, $PaletteTask)) {
            if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) {
                Stop-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
                Unregister-ScheduledTask -TaskName $name -Confirm:$false
                Write-Host ("removed {0}" -f $name)
            }
        }
        Stop-Existing
    }
    'restart' {
        foreach ($name in @($DispatcherTask, $StreamDeckTask, $PaletteTask)) {
            if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) {
                Stop-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
            }
        }
        Stop-Existing
        Start-Sleep -Seconds 1
        Start-ScheduledTask -TaskName $DispatcherTask
        foreach ($name in @($StreamDeckTask, $PaletteTask)) {
            if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) {
                Start-Sleep -Seconds 2
                Start-ScheduledTask -TaskName $name
            }
        }
        Start-Sleep -Seconds 3
        Show-Status
    }
    default { Show-Status }
}
