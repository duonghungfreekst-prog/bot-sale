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

echo [1/2] Dang kiem tra va bien dich ban Release...
cargo build --release --quiet
if %errorlevel% neq 0 (
    echo [LOI] Khong the bien dich ma nguon Rust!
    pause
    exit /b %errorlevel%
)

echo [2/2] Dang khoi chay SaleHunter Bot (Long Polling + Direct Links)...
echo       Web Admin PC: http://127.0.0.1:10000/admin
echo       Nhan Ctrl+C de dung bot an toan.
echo ========================================================
.\target\release\salehunter_bot.exe

pause
