@echo off
rem Forward a cargo command after injecting the MSVC environment.
rem
rem Why this wrapper exists: Git ships a coreutils link.exe that sits earlier on
rem PATH than the MSVC linker. Running cargo build directly picks the wrong one
rem and fails with "extra operand ... Try 'link --help'", which gives no hint
rem that the linker itself was the problem.
rem
rem Comments are in English on purpose: cmd.exe reads .bat files in the OEM code
rem page, so UTF-8 Chinese comments turn into garbage and can break parsing.
rem
rem No setlocal: setup_x64.bat must keep its PATH changes in this process.
rem
rem Usage: scripts\cargo-msvc.bat build --release
rem        scripts\cargo-msvc.bat test -p route-core

set "MANIFEST=%~dp0..\src-tauri\Cargo.toml"

set "MSVC_ROOT=%PORTABLE_MSVC_ROOT%"
if "%MSVC_ROOT%"=="" set "MSVC_ROOT=D:\Applications\msvc-portable\msvc"

rem Probe cl.exe, not link.exe: coreutils also has link.exe, so it proves nothing.
where /q cl.exe && goto run

if not exist "%MSVC_ROOT%\setup_x64.bat" (
  echo ERROR: MSVC toolchain not found at %MSVC_ROOT%\setup_x64.bat 1>&2
  echo Run scripts\setup-msvc.bat to download one, or set PORTABLE_MSVC_ROOT to 1>&2
  echo the folder containing setup_x64.bat if you already have it elsewhere. 1>&2
  exit /b 1
)
call "%MSVC_ROOT%\setup_x64.bat" >nul

rem Verify again. Falling through on a failed injection yields that cryptic
rem coreutils link error instead of a clear message.
where /q cl.exe || (
  echo ERROR: cl.exe still not found after injecting MSVC; toolchain incomplete. 1>&2
  exit /b 1
)

:run
set "SUBCOMMAND=%1"
shift
set "ARGS="
:collect
if "%1"=="" goto dispatch
set "ARGS=%ARGS% %1"
shift
goto collect

:dispatch
cargo %SUBCOMMAND% --manifest-path "%MANIFEST%"%ARGS%
exit /b %ERRORLEVEL%
