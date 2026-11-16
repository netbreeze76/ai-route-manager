@echo off
setlocal
title AI Route Manager - Emergency Domestic Recovery
echo Restoring direct domestic network access...
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "%~dp0Restore-Domestic-Network.ps1"
set "RC=%ERRORLEVEL%"
echo.
if "%RC%"=="0" (
  echo Recovery finished. Kimi, GLM and DeepSeek can now use the domestic connection.
) else (
  echo Recovery did not fully pass. Exit code: %RC%
  echo Share the newest recovery.log and result.json from the records folder.
)
echo.
pause >nul
exit /b %RC%
