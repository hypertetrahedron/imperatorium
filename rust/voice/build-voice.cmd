@echo off
rem Builds imperatorium-voice: MSVC, VS's own CMake and Ninja, and nvcc (the
rem default, CUDA) or the Vulkan SDK (--no-default-features --features vulkan).
for /f "usebackq tokens=*" %%i in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set VSDIR=%%i
call "%VSDIR%\VC\Auxiliary\Build\vcvars64.bat" >nul || exit /b 1
set "PATH=%VSDIR%\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;%VSDIR%\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja;%USERPROFILE%\.cargo\bin;%PATH%"
set CMAKE_GENERATOR=Ninja
rem rust\.cargo\config.toml links the C runtime statically; whisper.cpp must
rem too. Its CMakeLists asks for CMake 3.5, which leaves CMP0091 OLD and /MD
rem in the flags, so the policy is set here (whisper-rs-sys passes CMAKE_*
rem variables through to CMake).
set CMAKE_POLICY_DEFAULT_CMP0091=NEW
set CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded
rem The Vulkan build nests ~225 characters below the target folder (its
rem shader generator is a CMake sub-build): past Windows' 260 unless the
rem target folder is under ~35, so pass --target-dir to a short one.
rem bindgen needs libclang.dll: set LIBCLANG_PATH to its folder (LLVM, VS's Clang
rem tools, or pip install --target DIR libclang - DIR\clang\native). The
rem bindings whisper-rs-sys ships were made on Linux and do not fit Windows.
if not defined LIBCLANG_PATH (echo set LIBCLANG_PATH to the folder holding libclang.dll & exit /b 1)
cd /d "%~dp0"
rem No arguments: the release build. Otherwise a cargo command (test --release ...).
if "%~1"=="" (cargo build --release) else (cargo %*)
