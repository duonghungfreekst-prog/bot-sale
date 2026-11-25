@echo off
chcp 65001 > nul
title The Gioi Sieu Sale - PC Local Runner
echo ========================================================
echo   KHOI DONG BOT SAN SALE TRUC TIEP TREN PC (RUST + LIBSQL)
echo ========================================================

if not exist .env (
    if exist .env.example (
        echo [THONG BAO] Chua co file .env, dang sao chep tu .env.example...
        copy .env.example .env > nul
        echo [CANH BAO] Hay kiem tra file .env truoc khi van hanh!
    )
)

echo [1/2] Dang kiem tra va bien dich ban Release (mat khoang 30-60 giay neu co sua code)...
tasklist /FI "IMAGENAME eq salehunter_bot.exe" 2>nul | find /I "salehunter_bot.exe" >nul
if %errorlevel% equ 0 (
    echo [CANH BAO] Phat hien salehunter_bot.exe cu dang chay nen khong the ghi de file .exe!
    echo            Vui long DONG cua so bot cu truoc roi nhan phim bat ky de tiep tuc...
    pause
)
cargo build --release
if %errorlevel% neq 0 (
    echo [LOI] Khong the bien dich ma nguon Rust!
    pause
    exit /b %errorlevel%
)

set "CF_TUNNEL_TOKEN="
if exist .env (
    for /f "usebackq tokens=1,* delims==" %%A in (".env") do (
        if /I "%%A"=="CLOUDFLARE_TUNNEL_TOKEN" set "CF_TUNNEL_TOKEN=%%B"
    )
)
if defined CF_TUNNEL_TOKEN set "CF_TUNNEL_TOKEN=%CF_TUNNEL_TOKEN:"=%"
if defined CF_TUNNEL_TOKEN set "CF_TUNNEL_TOKEN=%CF_TUNNEL_TOKEN: =%"
if defined CF_TUNNEL_TOKEN (
    set "CF_EXE="
    if exist "C:\Program Files (x86)\cloudflared\cloudflared.EXE" set "CF_EXE=C:\Program Files (x86)\cloudflared\cloudflared.EXE"
    if not defined CF_EXE (
        for /f "delims=" %%C in ('where cloudflared 2^>nul') do if not defined CF_EXE set "CF_EXE=%%C"
    )
    if defined CF_EXE (
        echo [TUNNEL] Dang khoi dong Cloudflare Tunnel cho ten mien boc link rieng...
        start "The Gioi Sieu Sale - Cloudflare Tunnel" /min "%CF_EXE%" tunnel run --token "%CF_TUNNEL_TOKEN%"
    ) else (
        echo [CANH BAO] Co CLOUDFLARE_TUNNEL_TOKEN trong .env nhung khong tim thay cloudflared.exe!
    )
)

echo [2/2] Dang khoi chay SaleHunter Bot (Long Polling)...
echo       Web Admin PC: http://127.0.0.1:10000/admin
echo       Giu cua so nay mo khi su dung. Nhan Ctrl+C de dung bot an toan.
echo ========================================================
start "" /b cmd /c "timeout /t 2 /nobreak >nul && start http://127.0.0.1:10000/admin"
.\target\release\salehunter_bot.exe

pause
