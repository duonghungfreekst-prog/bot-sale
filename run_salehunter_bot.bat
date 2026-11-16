@echo off
chcp 65001 > nul
echo ========================================================
echo   KHOI DONG BOT SAN SALE VA HOAN TIEN (RUST + LIBSQL)
echo ========================================================

if not exist .env (
    if exist .env.example (
        echo [THONG BAO] Chua co file .env, dang sao chep tu .env.example...
        copy .env.example .env > nul
        echo [CANH BAO] Hay mo file .env va dien TELEGRAM_BOT_TOKEN cua ban!
    )
)

echo [1/2] Dang kiem tra va bien dich du an...
cargo check --quiet
if %errorlevel% neq 0 (
    echo [LOI] Khong the bien dich ma nguon Rust!
    pause
    exit /b %errorlevel%
)

echo [2/2] Dang khoi chay SaleHunter Bot...
cargo run --release

pause
