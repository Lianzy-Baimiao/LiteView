@echo off
setlocal
powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "%~dp0collect_windows_support.ps1"
set "support_exit=%ERRORLEVEL%"
echo.
if not "%support_exit%"=="0" echo Collection failed. Please send the crash details screenshot instead.
echo Nothing was uploaded. Read README.txt before sharing your report.
echo Press any key to close.
pause >nul
exit /b %support_exit%
