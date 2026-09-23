@echo off
chcp 65001 >nul

rem Start `tauri dev` with the MSVC environment injected.
rem
rem Keep this file CRLF and ASCII-only (cmd.exe mis-parses LF-only batch files;
rem UTF-8 Chinese in the OEM code page can break parsing mid-line).
rem
rem No setlocal: setup_x64.bat must keep its PATH changes in this process so the
rem cargo child sees them. Without it, cc-rs fails with `failed to find tool
rem "cl.exe"` while building Tauri's native deps.
rem
rem Layout mirrors agent_installer/scripts: this file lives in scripts\, the
rem repo root is one level up. Like build-app.bat, this project uses a VENDORED
rem standalone pnpm (tools/pnpm-standalone/.../pnpm.mjs), so we call it via
rem `node` rather than bare `pnpm`. A bare `pnpm` would read package.json's
rem `packageManager: pnpm@12.5.1` and try to download/switch to that version,
rem which hangs with no output. The standalone needs no PATH entry and no
rem network.
for %%R in ("%~dp0..") do set "ROOT=%%~fR\"

set "PNPM_MJS=%ROOT%tools\pnpm-standalone\package\bin\pnpm.mjs"
where /q node || (
  echo ERROR: node not found on PATH. Tauri dev requires Node.js 22.12 or newer. 1>&2
  exit /b 1
)

rem Make bare `pnpm` resolve to the vendored standalone. Covers nested
rem `pnpm run ...` chains and matches what tauri.conf.json expects when it runs
rem beforeDevCommand/beforeBuildCommand.
set "PATH=%ROOT%tools\pnpm-standalone\package\bin;%PATH%"

set "MSVC_ROOT=%PORTABLE_MSVC_ROOT%"
if "%MSVC_ROOT%"=="" set "MSVC_ROOT=D:\Applications\msvc-portable\msvc"

rem Probe cl.exe, not link.exe: coreutils also ships link.exe.
where /q cl.exe
if not errorlevel 1 goto run

if not exist "%MSVC_ROOT%\setup_x64.bat" (
  echo ERROR: MSVC toolchain not found at %MSVC_ROOT%\setup_x64.bat 1>&2
  echo Run scripts\setup-msvc.bat to download one, or set 1>&2
  echo PORTABLE_MSVC_ROOT to the folder containing setup_x64.bat. 1>&2
  exit /b 1
)
echo Using portable MSVC toolchain: %MSVC_ROOT%
call "%MSVC_ROOT%\setup_x64.bat" >nul
where /q cl.exe
if errorlevel 1 (
  echo ERROR: cl.exe still not found after injection; toolchain incomplete. 1>&2
  exit /b 1
)

:run
call node "%PNPM_MJS%" --dir "%ROOT%." tauri dev
exit /b %ERRORLEVEL%
