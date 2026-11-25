@echo off
cd /d "%~dp0"
title The Gioi Sieu Sale - Auto PC Runner

echo ========================================================================
echo   THE GIOI SIEU SALE - TU DONG KICH HOAT 100%% (KHONG CAN BAM TAY)
echo ========================================================================

if not exist ".env" (
    if exist ".env.example" copy /Y ".env.example" ".env" > nul
)

echo [1/3] Dang don tien trinh bot ^& tunnel cu (neu co^)...
taskkill /F /IM salehunter_bot.exe >nul 2>&1
if exist "target\cf_tunnel.pid" (
    for /f "usebackq delims=" %%P in ("target\cf_tunnel.pid") do taskkill /F /PID %%P >nul 2>&1
    del /Q "target\cf_tunnel.pid" >nul 2>&1
)
timeout /t 1 /nobreak >nul

echo [2/3] Dang kiem tra va bien dich ban Release...
cargo build --release
if errorlevel 1 (
    echo [LOI] Khong the bien dich ma nguon Rust!
    pause
    exit /b 1
)

echo [3/3] Dang khoi chay The Gioi Sieu Sale Desktop App (Auto Bot + Tunnel)...
echo       Cua so giao dien Desktop + khay he thong (System Tray) se tu mo.
start "" ".\target\release\salehunter_bot.exe"
exit /b 0

