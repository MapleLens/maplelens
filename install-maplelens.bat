@echo off
rem Double-click once to install MapleLens and everything it needs.
cd /d "%~dp0"
python --version >nul 2>&1
if errorlevel 1 (
    echo Python is not installed yet. Install it from https://www.python.org/downloads/
    echo IMPORTANT: tick "Add python.exe to PATH" on the first installer screen, then run this file again.
    pause
    exit /b 1
)
python -m pip install -e .
echo.
echo Done. Now double-click start-maplelens.bat (demo mode; see README step 3 about real price data).
pause
