@echo off
rem Imperatorium for a machine whose PowerShell policy blocks `irm ... | iex`:
rem   curl -fsSLo install.cmd https://github.com/hypertetrahedron/imperatorium/releases/latest/download/install.cmd && install.cmd
rem Runs install.ps1 from the same release with this one process's policy
rem bypassed; arguments go on to `imperatorium install`.
setlocal
if "%IMPERATORIUM_RELEASES%"=="" (
  if /i "%IMPERATORIUM_CHANNEL%"=="preview" (
    set "IMPERATORIUM_BASE=https://github.com/hypertetrahedron/imperatorium/releases/download/preview"
  ) else (
    set "IMPERATORIUM_BASE=https://github.com/hypertetrahedron/imperatorium/releases/latest/download"
  )
) else (
  set "IMPERATORIUM_BASE=%IMPERATORIUM_RELEASES%"
)
set "IMPERATORIUM_PS1=%TEMP%\imperatorium-install-%RANDOM%.ps1"
curl.exe -fsSL -o "%IMPERATORIUM_PS1%" "%IMPERATORIUM_BASE%/install.ps1" || exit /b 1
powershell -NoProfile -ExecutionPolicy Bypass -File "%IMPERATORIUM_PS1%" -From "%IMPERATORIUM_BASE%" %*
set "RC=%ERRORLEVEL%"
del "%IMPERATORIUM_PS1%" >nul 2>&1
exit /b %RC%
