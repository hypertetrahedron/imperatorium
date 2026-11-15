@echo off
rem Imperatorium for a machine whose PowerShell policy blocks `irm ... | iex`:
rem   curl -fsSLo install.cmd https://github.com/hypertetrahedron/imperatorium/releases/latest/download/install.cmd && install.cmd
rem Runs install.ps1 from the same release with this one process's policy
rem bypassed; arguments go on to `imperatorium install`.
rem
rem   IMPERATORIUM_VERSION   a specific release, 0.4.0 or v0.4.0 (default: the newest)
rem   IMPERATORIUM_CHANNEL   stable (default) or preview
rem   IMPERATORIUM_RELEASES  another release folder (a mirror, or a test server)
rem RELEASES wins over VERSION, which wins over CHANNEL.
setlocal
set "IMPERATORIUM_BASE=%IMPERATORIUM_RELEASES%"
if not "%IMPERATORIUM_BASE%"=="" goto have_base
set "IMPERATORIUM_V=%IMPERATORIUM_VERSION%"
if "%IMPERATORIUM_V%"=="" goto no_version
if /i "%IMPERATORIUM_V:~0,1%"=="v" set "IMPERATORIUM_V=%IMPERATORIUM_V:~1%"
set "IMPERATORIUM_BASE=https://github.com/hypertetrahedron/imperatorium/releases/download/v%IMPERATORIUM_V%"
goto have_base
:no_version
if /i "%IMPERATORIUM_CHANNEL%"=="preview" (
  set "IMPERATORIUM_BASE=https://github.com/hypertetrahedron/imperatorium/releases/download/preview"
) else (
  set "IMPERATORIUM_BASE=https://github.com/hypertetrahedron/imperatorium/releases/latest/download"
)
:have_base
set "IMPERATORIUM_PS1=%TEMP%\imperatorium-install-%RANDOM%.ps1"
curl.exe -fsSL -o "%IMPERATORIUM_PS1%" "%IMPERATORIUM_BASE%/install.ps1" || exit /b 1
powershell -NoProfile -ExecutionPolicy Bypass -File "%IMPERATORIUM_PS1%" -From "%IMPERATORIUM_BASE%" %*
set "RC=%ERRORLEVEL%"
del "%IMPERATORIUM_PS1%" >nul 2>&1
exit /b %RC%
