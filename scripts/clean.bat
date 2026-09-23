@echo off
chcp 65001 >nul
setlocal enabledelayedexpansion

rem Remove build artifacts.
rem
rem Keep this file CRLF and ASCII-only (cmd.exe mis-parses LF-only batch files,
rem and UTF-8 Chinese in the OEM code page can break parsing mid-line).
rem
rem This script lives in scripts ; the repo root is one level up.
for %%R in ("%~dp0..") do set "ROOT=%%~fR\"

set "DEEP="
set "YES="
set "KILL="
for %%F in (%*) do (
  if /i "%%~F"=="--deep" set "DEEP=1"
  if /i "%%~F"=="-d"     set "DEEP=1"
  if /i "%%~F"=="--yes"  set "YES=1"
  if /i "%%~F"=="-y"     set "YES=1"
  if /i "%%~F"=="--kill" set "KILL=1"
  if /i "%%~F"=="-k"     set "KILL=1"
  if /i "%%~F"=="--help" goto usage
  if /i "%%~F"=="-h"     goto usage
)
if "%~1"=="--strayPids" goto strayPids

echo ============================================
echo   Route Planner - clean build artifacts
echo ============================================
echo.
echo Root: %ROOT%
echo.
echo Will remove:
echo   - dist                 (collected release: portable folder + installer)
echo   - src-tauri\target     (Rust build cache and bundles)
echo   - build                (SvelteKit static output)
echo   - .svelte-kit          (SvelteKit generated types)
echo   - .tmp                 (test work files)
if defined DEEP (
  echo.
  echo   --deep also removes:
  echo   - node_modules
  echo   Needs 'pnpm install' afterwards.
)
echo.
echo NOT touched: source files, git history, docs, or anything the installer
echo has written outside this repo ^(runtimes, tool configs, %%LOCALAPPDATA%%^).
echo.

rem A running build or app holds files open; rd would half-succeed and leave a
rem confusing partial tree.
tasklist 2>nul | findstr /i "route-planner.exe cargo.exe rustc.exe" >nul
if not errorlevel 1 goto running

set "STRAY="
for /f "delims=" %%P in ('"%~f0" --strayPids') do (
  set "STRAY=1"
  echo   stale node process in this repo: PID %%P
)
if defined STRAY (
  echo.
  if defined KILL (
    echo   --kill will terminate the processes above.
  ) else (
    echo   Re-run with --kill, or stop these processes before cleaning.
  )
  echo.
)

if defined YES goto confirmed
choice /c YN /n /m "Proceed? [Y/N] "
if errorlevel 2 goto aborted

:confirmed
if defined STRAY if defined KILL (
  for /f "delims=" %%P in ('"%~f0" --strayPids') do taskkill /f /pid %%P >nul 2>nul
  ping -n 2 127.0.0.1 >nul 2>nul
)

call :killdir "%ROOT%dist"
call :killdir "%ROOT%src-tauri\target"
call :killdir "%ROOT%build"
call :killdir "%ROOT%.svelte-kit"
call :killdir "%ROOT%.tmp"

if defined DEEP call :killdir "%ROOT%node_modules"

echo.
echo ============================================
echo   Done.
echo ============================================
echo.
echo Next release build:
echo   scripts\build-app.bat
goto end

:strayPids
rem Vite's dev server survives a closed terminal often enough to be worth
rem detecting. Match on the command line containing this repo's path so we never
rem touch node processes from other projects.
powershell -NoProfile -Command "$root='%ROOT%'.TrimEnd('\').ToLower(); Get-CimInstance Win32_Process -Filter \"Name='node.exe'\" | Where-Object { $_.CommandLine -and $_.CommandLine.ToLower().Contains($root) } | ForEach-Object { $_.ProcessId }" 2>nul
exit /b 0

:killdir
if not exist "%~1" goto :eof
echo   removing %~1
rd /s /q "%~1" 2>nul
if exist "%~1" echo     WARNING: could not fully remove ^(file in use?^)
goto :eof

:usage
echo Usage: scripts\clean.bat [--deep] [--kill] [--yes]
echo.
echo   --deep, -d   also remove node_modules
echo   --kill, -k   terminate stale node processes from this repo
echo   --yes, -y    skip the confirmation prompt
exit /b 0

:running
echo ERROR: a build or the running application is holding files. 1>&2
echo        Close route-planner / cargo / rustc first. 1>&2
tasklist 2>nul | findstr /i "route-planner.exe cargo.exe rustc.exe"
echo.
if not defined YES pause
exit /b 1

:aborted
echo Aborted. Nothing was removed.
echo.
if not defined YES pause
exit /b 1

:end
echo.
if not defined YES pause
exit /b 0
