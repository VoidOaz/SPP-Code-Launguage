@echo off
setlocal EnableExtensions
set "SPP_DIR=%~dp0"
set "SPP_EXE=%SPP_DIR%spp.exe"
if exist "%SPP_EXE%" goto run
for %%P in ("%~dp0..\..\target\release\spp.exe" "%~dp0..\..\target\debug\spp.exe") do (
  if exist "%%~fP" (
    set "SPP_EXE=%%~fP"
    goto run
  )
)
where spp.exe >nul 2>nul
if not errorlevel 1 (
  set "SPP_EXE=spp.exe"
  goto run
)
echo SPP compiler not found. Build this project with: cargo build --release
echo Then rerun this command or set spp.executablePath in VS Code.
exit /b 1
:run
"%SPP_EXE%" %*
exit /b %ERRORLEVEL%
