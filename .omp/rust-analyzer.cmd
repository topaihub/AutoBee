@echo off
setlocal
rem Match scripts\cargo-msvc.bat so analysis uses the same MSVC toolchain.
set "MSVC_ROOT=%PORTABLE_MSVC_ROOT%"
if "%MSVC_ROOT%"=="" set "MSVC_ROOT=D:\Applications\msvc-portable\msvc"
where /q cl.exe && goto run
if not exist "%MSVC_ROOT%\setup_x64.bat" (
  echo ERROR: Set PORTABLE_MSVC_ROOT to the MSVC toolchain directory. 1>&2
  exit /b 1
)
call "%MSVC_ROOT%\setup_x64.bat" >nul
where /q cl.exe || exit /b 1
:run
rust-analyzer %*
exit /b %ERRORLEVEL%
