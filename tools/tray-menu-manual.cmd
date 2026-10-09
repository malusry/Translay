@echo off
setlocal
title Translay - manual tray test
echo Starting the isolated Translay tray test...
echo Keep this window open. Choose Quit in the test tray menu when finished.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0tray-menu-acceptance.ps1" -SkipBuild -Manual
if errorlevel 1 (
    echo.
    echo The test did not finish normally. Reports are in tests\artifacts.
    pause
)
endlocal
