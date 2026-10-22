@echo off
rem Double-click to start MapleLens. Close this window (or press Ctrl+C) to stop it.
cd /d "%~dp0"
python -m maplelens
if errorlevel 1 (
    echo.
    echo Something went wrong. Take a look at the README, section "If something goes wrong".
)
pause
