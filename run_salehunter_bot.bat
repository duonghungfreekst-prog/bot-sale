@echo off
setlocal EnableExtensions DisableDelayedExpansion
chcp 65001 > nul
cd /d "%~dp0"
title The Gioi Sieu Sale - Auto All-In-One PC Runner

echo ========================================================================
echo   THE GIOI SIEU SALE - TU DONG FULL TRONG 1 FILE (BOT + CLOUDFLARE)
echo ========================================================================

if not exist ".env" (
    if exist ".env.example" (
        echo [SETUP] Chua co file .env, dang khoi tao tu .env.example...
        copy /Y ".env.example" ".env" > nul
    ) else (
        type nul > ".env"
    )
)

echo [1/4] Dang tu dong tat ban bot ^& tunnel cu (neu dang chay^)...
taskkill /F /IM salehunter_bot.exe >nul 2>&1
if exist "target\cf_tunnel.pid" (
    for /f "usebackq delims=" %%P in ("target\cf_tunnel.pid") do (
        taskkill /F /PID %%P >nul 2>&1
    )
    del /Q "target\cf_tunnel.pid" >nul 2>&1
)
timeout /t 1 /nobreak >nul

echo [2/4] Dang kiem tra ban build Release...
set "NEED_BUILD=0"
if not exist "target\release\salehunter_bot.exe" set "NEED_BUILD=1"
if "%NEED_BUILD%"=="1" goto DO_BUILD

powershell -NoProfile -Command "$exe = (Get-Item 'target\release\salehunter_bot.exe').LastWriteTimeUtc; $files = @('Cargo.toml','Cargo.lock','cloudflare_worker.js') + (Get-ChildItem 'src','assets' -Recurse -File | Select-Object -ExpandProperty FullName); if ($files | Where-Object { Test-Path $_ } | Where-Object { (Get-Item $_).LastWriteTimeUtc -gt $exe } | Select-Object -First 1) { exit 1 } else { exit 0 }"
if errorlevel 1 set "NEED_BUILD=1"
if "%NEED_BUILD%"=="1" goto DO_BUILD

echo       Ban .exe da la ban moi nhat — khoi dong ngay khong can cho build!
goto CHECK_TUNNEL

:DO_BUILD
echo       Phat hien ma nguon moi -^> dang bien dich ban Release (khoang 30-60s^)...
cargo build --release
if errorlevel 1 (
    echo [LOI] Khong the bien dich ma nguon Rust! Hay kiem tra lai loi o tren.
    pause
    exit /b 1
)

:CHECK_TUNNEL
powershell -NoProfile -Command "$lines = Get-Content '.env' -Encoding UTF8 -ErrorAction SilentlyContinue; $hit = $lines | Where-Object { $_ -match '^\s*CLOUDFLARE_TUNNEL_TOKEN\s*=\s*(.+)$' -and $matches[1].Trim().Trim([char]34).Length -ge 20 }; if ($hit) { exit 0 } else { exit 1 }"
if not errorlevel 1 goto START_TUNNEL

echo.
echo ------------------------------------------------------------------------
echo [3/4] CHUA CO CLOUDFLARE TUNNEL TOKEN TRONG .env
echo       De link https://thegioisieusale.click/xxxx mo duoc tren Dien Thoai ^& PC:
echo       - Buoc A: Vao https://one.dash.cloudflare.com -^> Networks -^> Tunnels
echo                 Tao 1 Tunnel moi, tro origin.thegioisieusale.click -^> HTTP 127.0.0.1:10000
echo       - Buoc B: Vao https://dash.cloudflare.com -^> Workers -^> Edit code
echo                 Doi ORIGIN_URL thanh: https://origin.thegioisieusale.click roi Deploy
echo       - Buoc C: Dan doan token (eyJ...^) hoac nguyen lenh service install vao day!
echo ------------------------------------------------------------------------
echo       [M] Mo san 2 trang Cloudflare tren trinh duyet
echo       [Enter] Bo qua tam buoc Tunnel de chay Web Admin ^& Bot truoc
echo.
set "CF_INPUT="
set /p "CF_INPUT=>> Dan Tunnel Token / nhap M / hoac Enter: "

powershell -NoProfile -Command "$raw = [string]$env:CF_INPUT; if ($raw.Trim() -ieq 'M') { exit 2 }; if ([string]::IsNullOrWhiteSpace($raw)) { exit 3 }; if ($raw -match '(eyJ[A-Za-z0-9+/=_-]{20,})') { $tok = $matches[1]; $lines = @(Get-Content '.env' -Encoding UTF8 -ErrorAction SilentlyContinue | Where-Object { $_ -notmatch '^\s*CLOUDFLARE_TUNNEL_TOKEN\s*=' }); $lines += ('CLOUDFLARE_TUNNEL_TOKEN=' + $tok); [System.IO.File]::WriteAllLines((Resolve-Path '.env').Path, $lines, [System.Text.UTF8Encoding]::new($false)); Write-Host '[OK] Da tu dong luu CLOUDFLARE_TUNNEL_TOKEN vao .env!'; exit 0 } else { Write-Host '[LOI] Khong tim thay chuoi Token hop le (bat dau bang eyJ...)!'; exit 1 }"
set "CF_RC=%errorlevel%"
set "CF_INPUT="
if "%CF_RC%"=="2" (
    start "" "https://one.dash.cloudflare.com"
    start "" "https://dash.cloudflare.com"
    goto CHECK_TUNNEL
)
if "%CF_RC%"=="3" (
    echo [CANH BAO] Dang bo qua Tunnel — chi dung duoc http://127.0.0.1:10000/admin tren PC!
    goto START_BOT
)
if not "%CF_RC%"=="0" goto CHECK_TUNNEL

:START_TUNNEL
set "CF_EXE="
if exist "C:\Program Files (x86)\cloudflared\cloudflared.EXE" set "CF_EXE=C:\Program Files (x86)\cloudflared\cloudflared.EXE"
if not defined CF_EXE (
    for /f "delims=" %%C in ('where cloudflared 2^>nul') do if not defined CF_EXE set "CF_EXE=%%C"
)
if not defined CF_EXE (
    echo [CANH BAO] Da co Token nhung khong tim thay cloudflared.exe tren may!
    goto START_BOT
)

if not exist "target" mkdir "target"
echo [3/4] Dang bat Cloudflare Tunnel tu dong noi https://thegioisieusale.click ve PC...
powershell -NoProfile -Command "$line = (Get-Content '.env' -Encoding UTF8 | Where-Object { $_ -match '^\s*CLOUDFLARE_TUNNEL_TOKEN\s*=\s*(.+)$' } | Select-Object -First 1); if ($line -match '^\s*CLOUDFLARE_TUNNEL_TOKEN\s*=\s*(.+)$') { $tok = $matches[1].Trim().Trim([char]34); $env:TUNNEL_TOKEN = $tok; $p = Start-Process -FilePath $env:CF_EXE -ArgumentList @('tunnel','--no-autoupdate','run') -WindowStyle Minimized -PassThru; [System.IO.File]::WriteAllText((Join-Path (Get-Location) 'target\cf_tunnel.pid'), $p.Id.ToString()); Write-Host ('       Cloudflare Tunnel dang chay ngam (PID ' + $p.Id + ')!') }"
timeout /t 2 /nobreak >nul

:START_BOT
echo [4/4] Dang khoi chay The Gioi Sieu Sale Bot (Long Polling)...
echo       Web Admin PC: http://127.0.0.1:10000/admin
echo       Giu cua so nay mo khi bot hoat dong. Nhan Ctrl+C de dung toan bo an toan.
echo ========================================================================
start "" /b powershell -NoProfile -WindowStyle Hidden -Command "for ($i=0; $i -lt 30; $i++) { try { $r = Invoke-WebRequest -Uri 'http://127.0.0.1:10000/health' -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { Start-Process 'http://127.0.0.1:10000/admin'; break } } catch {}; Start-Sleep -Milliseconds 500 }"

.\target\release\salehunter_bot.exe

:CLEANUP
if exist "target\cf_tunnel.pid" (
    for /f "usebackq delims=" %%P in ("target\cf_tunnel.pid") do (
        taskkill /F /PID %%P >nul 2>&1
    )
    del /Q "target\cf_tunnel.pid" >nul 2>&1
)
echo.
echo [DUNG] Bot va Tunnel da duoc dong an toan.
pause
