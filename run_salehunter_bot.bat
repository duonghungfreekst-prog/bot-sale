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

echo [3/3] Dang khoi chay Bot + Tu Dong Bat Cloudflare Tunnel...
echo       Web Admin PC: http://127.0.0.1:10000/admin
echo       Giu cua so nay mo khi su dung. Nhan Ctrl+C de dung toan bo an toan.
echo ========================================================================
start "" /b powershell -NoProfile -WindowStyle Hidden -Command "for ($i=0; $i -lt 30; $i++) { try { $r = Invoke-WebRequest -Uri 'http://127.0.0.1:10000/health' -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { Start-Process 'http://127.0.0.1:10000/admin'; break } } catch {}; Start-Sleep -Milliseconds 500 }"

.\target\release\salehunter_bot.exe

if exist "target\cf_tunnel.pid" (
    for /f "usebackq delims=" %%P in ("target\cf_tunnel.pid") do taskkill /F /PID %%P >nul 2>&1
    del /Q "target\cf_tunnel.pid" >nul 2>&1
)
