@echo off
chcp 65001 >nul
setlocal

rem Release build: six-gate verify + frontend + Rust + NSIS installer.
rem
rem AutoBee ships a vendored standalone pnpm because Windows symlink restrictions
rem break pnpm's default isolated linker.
rem
rem Keep this file CRLF and ASCII-only: cmd.exe mis-parses LF-only .bat files.
rem relies on a global `pnpm` whose version matches its own packageManager, so it
rem can call bare `pnpm`. Here a bare `pnpm` reads package.json's
rem `packageManager: pnpm@12.5.1` and tries to download/switch to that exact
rem packageManager and tries to download/switch to that exact version, which hangs
rem with no output on the first invocation. We drive the vendored standalone via
rem node instead: no PATH dependency, no version re-exec, no network hang.
rem
rem Keep this file CRLF and ASCII-only: cmd.exe mis-parses LF-only .bat files,
rem and OEM-code-page UTF-8 Chinese breaks parsing mid-line.

for %%R in ("%~dp0..") do set "ROOT=%%~fR\"

rem Vendored standalone pnpm. `node` must be on PATH (required for any Tauri
rem frontend build anyway).
set "PNPM_MJS=%ROOT%tools\pnpm-standalone\package\bin\pnpm.mjs"
where /q node || (
  echo ERROR: node not found on PATH. Tauri builds require Node.js 22.12 or newer. 1>&2
  goto fail
)

rem Make bare `pnpm` resolve to the vendored standalone. This also covers nested
rem `pnpm run ...` chains inside package.json scripts (e.g. `verify`), which would
rem otherwise fall through to a global pnpm and hang on the version mismatch.
set "PATH=%ROOT%tools\pnpm-standalone\package\bin;%PATH%"

set "SRC=%ROOT%src-tauri\target\release"
set "DIST=%ROOT%dist\AutoBee"
set "INSTALLER=%ROOT%dist\installer"

echo ============================================
echo   AutoBee - release build
echo.

rem ---------------------------------------------------------------------------
rem [1/6] Tauri CLI reachability
rem
rem pnpm on Windows may fail to create the symlink layer and leave an empty
rem package dir. `pnpm tauri build` then throws a long
rem "Cannot find module './cli.win32-x64-msvc.node'" stack that looks nothing
rem like a dependency problem. Probe first (about a second), reinstall only if
rem it fails. Note `pnpm install` alone does NOT fix it: it compares the
rem lockfile against its own metadata and reports "Already up to date" even when
rem files are missing. The directory has to go first.
rem ---------------------------------------------------------------------------
echo [1/6] Checking the Tauri CLI...
call node "%PNPM_MJS%" --dir "%ROOT%." tauri --version >nul 2>nul
if not errorlevel 1 goto tauri_ok

echo       CLI not loadable; reinstalling dependencies.
rd /s /q "%ROOT%node_modules" 2>nul
call node "%PNPM_MJS%" --dir "%ROOT%." install
if errorlevel 1 (
  echo ERROR: pnpm install failed. 1>&2
  goto fail
)
call node "%PNPM_MJS%" --dir "%ROOT%." tauri --version >nul 2>nul
if errorlevel 1 (
  echo ERROR: the Tauri CLI is still not loadable after pnpm install. 1>&2
  echo        This is almost always missing symlink permission. 1>&2
  echo        Turn on Windows Developer Mode and retry. 1>&2
  goto fail
)

:tauri_ok
echo       ok.
echo.

rem ---------------------------------------------------------------------------
rem [2/6] MSVC toolchain (same coreutils link.exe trap as cargo-msvc.bat)
rem ---------------------------------------------------------------------------
echo [2/6] Locating the MSVC toolchain...
set "MSVC_ROOT=%PORTABLE_MSVC_ROOT%"
if "%MSVC_ROOT%"=="" set "MSVC_ROOT=D:\Applications\msvc-portable\msvc"

where /q cl.exe
if not errorlevel 1 (
  echo       an MSVC compiler is already on PATH.
  goto msvc_ok
)

if not exist "%MSVC_ROOT%\setup_x64.bat" (
  echo ERROR: the MSVC linker was not found. 1>&2
  echo        Looked for: %MSVC_ROOT%\setup_x64.bat 1>&2
  echo        Run scripts\setup-msvc.bat or set PORTABLE_MSVC_ROOT. 1>&2
  goto fail
)
echo       Using portable MSVC toolchain: %MSVC_ROOT%
call "%MSVC_ROOT%\setup_x64.bat" >nul
where /q cl.exe
if errorlevel 1 (
  echo ERROR: cl.exe still not found after injection; toolchain incomplete. 1>&2
  goto fail
)

:msvc_ok
echo.

rem ---------------------------------------------------------------------------
rem [3/6] Quality gate (frontend check/lint/test + Rust test/clippy/fmt)
rem ---------------------------------------------------------------------------
echo [3/6] Running the verify gate...
call node "%PNPM_MJS%" --dir "%ROOT%." run verify
if errorlevel 1 (
  echo ERROR: verify gate failed; aborting build. 1>&2
  goto fail
)
echo.

rem ---------------------------------------------------------------------------
rem [4/6] Build. `tauri build` runs `pnpm build` itself via beforeBuildCommand.
rem ---------------------------------------------------------------------------
echo [4/6] Building the application...
rd /s /q "%SRC%\bundle" 2>nul
call node "%PNPM_MJS%" --dir "%ROOT%." tauri build
if errorlevel 1 (
  echo ERROR: the build failed. 1>&2
  goto fail
)
if not exist "%SRC%\AutoBee.exe" (
  echo ERROR: build reported success but the EXE is missing: 1>&2
  echo        %SRC%\AutoBee.exe 1>&2
  goto fail
)

rem The CLI is a separate crate, so `tauri build` does not build it. Without
rem this step dist\ would ship only the GUI and the CLI smoke checks would be
rem run against a stale debug exe.
echo       Building the command-line tool...
call "%ROOT%scripts\cargo-msvc.bat" build --release -p autobee-cli
if errorlevel 1 (
  echo ERROR: the CLI build failed. 1>&2
  goto fail
)
if not exist "%SRC%\autobee-cli.exe" (
  echo ERROR: CLI build reported success but the EXE is missing: 1>&2
  echo        %SRC%\autobee-cli.exe 1>&2
  goto fail
)
echo.

rem ---------------------------------------------------------------------------
rem [5/6] Collect into dist\
rem ---------------------------------------------------------------------------
echo [5/6] Collecting artifacts into dist\ ...
rem Wipe the payload dir first. Runtime data lands next to the exe by design
rem (see shell\src\paths.rs), so launching the packaged build here seeds
rem %DIST%\data with a settings.db and logs. Compress-Archive packs whatever
rem is in the folder, so without this the portable zip ships the local
rem credential store and log file.
rd /s /q "%DIST%" 2>nul
if not exist "%DIST%" mkdir "%DIST%"
if not exist "%INSTALLER%" mkdir "%INSTALLER%"

copy /y "%SRC%\AutoBee.exe" "%DIST%\AutoBee.exe" >nul
if errorlevel 1 (
  echo ERROR: failed to copy the EXE into %DIST%. 1>&2
  goto fail
)

copy /y "%SRC%\autobee-cli.exe" "%DIST%\autobee-cli.exe" >nul
if errorlevel 1 (
  echo ERROR: failed to copy the CLI into %DIST%. 1>&2
  goto fail
)

del /q "%INSTALLER%\*-setup.exe" 2>nul
if exist "%SRC%\bundle\nsis\*-setup.exe" (
  copy /y "%SRC%\bundle\nsis\*-setup.exe" "%INSTALLER%\" >nul
) else (
  echo       NOTE: no NSIS setup found under %SRC%\bundle\nsis
)
echo.

rem ---------------------------------------------------------------------------
rem [6/6] Portable zip (Tauri has no zip target on Windows)
rem ---------------------------------------------------------------------------
echo [6/6] Packing the portable zip...
for /f "tokens=3 delims= " %%V in ('findstr /r /c:"^version = " "%ROOT%src-tauri\Cargo.toml"') do set "APP_VER=%%~V"
if "%APP_VER%"=="" (
  echo ERROR: could not read the version out of src-tauri\Cargo.toml. 1>&2
  goto fail
)
set "ZIP=%INSTALLER%\AutoBee_%APP_VER%_x64-portable.zip"
del /q "%ZIP%" 2>nul
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "Compress-Archive -Path '%DIST%' -DestinationPath '%ZIP%' -CompressionLevel Optimal -Force"
if errorlevel 1 (
  echo ERROR: Compress-Archive failed. 1>&2
  goto fail
)
echo.

echo ============================================
echo   Done. Artifacts:
echo ============================================
echo.
dir /b "%DIST%\*.exe"
dir /b "%INSTALLER%" 2>nul
echo.
echo   Portable folder: %DIST%
echo.
exit /b 0

:fail
echo.
exit /b 1
