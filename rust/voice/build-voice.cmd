@echo off
rem Builds imperatorium-voice with CUDA: MSVC, VS's own CMake and Ninja, nvcc.
for /f "usebackq tokens=*" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set VSDIR=%%i
call "%VSDIR%\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1
set "PATH=%VSDIR%\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;%VSDIR%\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja;%USERPROFILE%\.cargo\bin;%PATH%"
set CMAKE_GENERATOR=Ninja
rem bindgen needs libclang.dll: set LIBCLANG_PATH to its folder (LLVM, VS's Clang
rem tools, or pip install --target DIR libclang - DIR\clang\native). The
rem bindings whisper-rs-sys ships were made on Linux and do not fit Windows.
if not defined LIBCLANG_PATH (echo set LIBCLANG_PATH to the folder holding libclang.dll & exit /b 1)
cd /d "%~dp0"
rem No arguments: the release build. Otherwise a cargo command (test --release ...).
if "%~1"=="" (cargo build --release) else (cargo %*)
