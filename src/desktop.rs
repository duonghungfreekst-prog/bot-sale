use std::path::PathBuf;
use std::process::Command;
use std::thread::JoinHandle;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{Icon as WindowIcon, WindowBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon as TrayIconImage, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use wry::{NewWindowResponse, WebContext, WebViewBuilder};

const BOT_LOGO_WEBP: &[u8] = include_bytes!("../assets/bot-logo.webp");

#[derive(Debug, Clone)]
pub enum DesktopEvent {
    BackendReady,
    ShowWindow,
    ReloadPage,
    OpenInBrowser,
    QuitApp,
}

fn load_logo_rgba() -> Option<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory(BOT_LOGO_WEBP).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Some((img.into_raw(), w, h))
}

fn open_external_url(raw: &str) {
    let trimmed = raw.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let _ = Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", trimmed])
            .spawn();
    }
}

fn is_allowed_local_navigation(raw_url: &str, port: u16) -> bool {
    let lower = raw_url.trim().to_ascii_lowercase();
    if lower == "about:blank" || lower.starts_with("data:") {
        return true;
    }
    let prefix_ip = format!("http://127.0.0.1:{}/", port);
    let exact_ip = format!("http://127.0.0.1:{}", port);
    let prefix_host = format!("http://localhost:{}/", port);
    let exact_host = format!("http://localhost:{}", port);
    lower.starts_with(&prefix_ip)
        || lower == exact_ip
        || lower.starts_with(&prefix_host)
        || lower == exact_host
}

fn splash_loading_html(port: u16) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="vi">
<head>
<meta charset="utf-8">
<title>Thế Giới Siêu Sale Desktop</title>
<style>
  :root {{
    color-scheme: light;
  }}
  body {{
    margin: 0; min-height: 100vh; display: flex; align-items: center; justify-content: center;
    background: #F4F6F9; color: #0F172A;
    font-family: "Be Vietnam Pro", "Segoe UI", system-ui, -apple-system, sans-serif;
    padding: 20px;
  }}
  .box {{
    text-align: center; padding: 36px 40px; border-radius: 18px;
    background: #FFFFFF; border: 1px solid #E2E8F0;
    box-shadow: 0 12px 32px rgba(15, 23, 42, 0.08); max-width: 440px; width: 100%;
  }}
  .spinner {{
    width: 42px; height: 42px; margin: 0 auto 18px; border-radius: 999px;
    border: 3px solid #FED7AA; border-top-color: #C2410C;
    animation: spin 0.85s linear infinite;
  }}
  h1 {{ margin: 0 0 8px; font-size: 18px; font-weight: 800; letter-spacing: -0.02em; color: #0F172A; }}
  p {{ margin: 0; font-size: 13.5px; color: #475569; line-height: 1.55; }}
  .port-tag {{
    display: inline-block; margin-top: 14px; padding: 4px 10px; border-radius: 999px;
    background: #FFF7ED; border: 1px solid #FED7AA; color: #C2410C;
    font-size: 12px; font-weight: 700;
  }}
  @keyframes spin {{ to {{ transform: rotate(360deg); }} }}
  @media (prefers-reduced-motion: reduce) {{
    .spinner {{ animation: none; border-top-color: #C2410C; }}
  }}
</style>
</head>
<body>
  <div class="box">
    <div class="spinner" aria-hidden="true"></div>
    <h1>THẾ GIỚI SIÊU SALE</h1>
    <p>Đang khởi động Bot Telegram, Web Quản Trị và tự động kết nối Cloudflare Tunnel...</p>
    <div class="port-tag">Cổng cục bộ: {port}</div>
  </div>
</body>
</html>"#,
        port = port
    )
}

/// Kiểm tra xem Desktop App có đang chạy sẵn ở khay hệ thống không; nếu có sẽ đánh thức cửa sổ lên.
pub fn try_wake_existing_instance(port: u16) -> bool {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::time::Duration;

    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(250)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(400)));
    let req = format!(
        "GET /internal/show HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
        port
    );
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 64];
    let n = stream.read(&mut buf).unwrap_or(0);
    let head = String::from_utf8_lossy(&buf[..n]);
    head.contains("200 OK")
}

/// Đảm bảo chỉ chạy 1 tiến trình Desktop duy nhất; nếu cổng đang bận lúc khởi động thì chờ đánh thức tiến trình hiện có.
pub fn ensure_single_instance_or_wake(port: u16) -> bool {
    if try_wake_existing_instance(port) {
        return true;
    }
    match std::net::TcpListener::bind(("0.0.0.0", port)) {
        Ok(listener) => {
            drop(listener);
            false
        }
        Err(_) => {
            for _ in 0..6 {
                std::thread::sleep(std::time::Duration::from_millis(150));
                if try_wake_existing_instance(port) {
                    return true;
                }
            }
            true
        }
    }
}

fn try_build_webview_with_dir(
    window: &tao::window::Window,
    port: u16,
    data_dir: PathBuf,
) -> Result<wry::WebView, wry::Error> {
    let _ = std::fs::create_dir_all(&data_dir);
    let mut web_context = WebContext::new(Some(data_dir));
    WebViewBuilder::new_with_web_context(&mut web_context)
        .with_html(splash_loading_html(port))
        .with_new_window_req_handler(move |url, _features| {
            open_external_url(&url);
            NewWindowResponse::Deny
        })
        .with_navigation_handler(move |url| {
            if is_allowed_local_navigation(&url, port) {
                true
            } else {
                open_external_url(&url);
                false
            }
        })
        .build(window)
}

fn build_desktop_webview(
    window: &tao::window::Window,
    port: u16,
) -> anyhow::Result<wry::WebView> {
    let ver = wry::webview_version()
        .map_err(|e| format!("{:?}", e))
        .unwrap_or_else(|e| format!("unavailable ({})", e));
    tracing::info!("Phiên bản Edge WebView2 Runtime trên máy: {}", ver);

    let base_target = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("target");
    let primary_dir = base_target.join("webview2_data");
    tracing::info!("Đang khởi tạo WebView2 tại: {}", primary_dir.display());

    let mut last_err = None;
    for attempt in 1..=3 {
        match try_build_webview_with_dir(window, port, primary_dir.clone()) {
            Ok(wv) => return Ok(wv),
            Err(err) => {
                tracing::warn!(
                    "Thử khởi tạo WebView2 lần {}/3 chưa thành công ({:?})...",
                    attempt,
                    err
                );
                last_err = Some(err);
                std::thread::sleep(std::time::Duration::from_millis(90));
            }
        }
    }

    let fallback_dir = base_target.join(format!("webview2_session_{}", std::process::id()));
    try_build_webview_with_dir(window, port, fallback_dir).map_err(|second_err| {
        anyhow::anyhow!(
            "Không thể tạo WebView2 window (primary={:?}; fallback={:?})",
            last_err,
            second_err
        )
    })
}

/// Khởi chạy cửa sổ Desktop WebView2 + System Tray trên luồng chính Windows.
pub fn run_desktop_app<F>(port: u16, start_backend: F) -> anyhow::Result<()>
where
    F: FnOnce(tao::event_loop::EventLoopProxy<DesktopEvent>, tokio::sync::oneshot::Receiver<()>) -> JoinHandle<()>,
{
    let event_loop = EventLoopBuilder::<DesktopEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let (win_icon, tray_icon_img) = match load_logo_rgba() {
        Some((rgba, w, h)) => (
            WindowIcon::from_rgba(rgba.clone(), w, h).ok(),
            TrayIconImage::from_rgba(rgba, w, h).ok(),
        ),
        None => (None, None),
    };

    let admin_url = format!("http://127.0.0.1:{}/admin", port);

    let window = WindowBuilder::new()
        .with_title("Thế Giới Siêu Sale — Quản Trị & Bot Săn Sale")
        .with_inner_size(tao::dpi::LogicalSize::new(1280.0, 820.0))
        .with_min_inner_size(tao::dpi::LogicalSize::new(960.0, 640.0))
        .with_window_icon(win_icon)
        .build(&event_loop)?;

    let browser_url = admin_url.clone();

    // Xây dựng System Tray Menu
    let tray_menu = Menu::new();
    let item_show = MenuItem::with_id("show", "📊 Mở Giao Diện Quản Trị", true, None);
    let item_reload = MenuItem::with_id("reload", "🔄 Tải Lại Trang Quản Trị", true, None);
    let item_browser = MenuItem::with_id("browser", "🌐 Mở Trên Trình Duyệt", true, None);
    let item_quit = MenuItem::with_id("quit", "❌ Thoát Hoàn Toàn Bot & Tunnel", true, None);
    let _ = tray_menu.append(&item_show);
    let _ = tray_menu.append(&item_reload);
    let _ = tray_menu.append(&item_browser);
    let _ = tray_menu.append(&PredefinedMenuItem::separator());
    let _ = tray_menu.append(&item_quit);

    let mut tray_builder = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("Thế Giới Siêu Sale — Đang chạy ngầm (Bot + Auto Tunnel)");
    if let Some(icon) = tray_icon_img {
        tray_builder = tray_builder.with_icon(icon);
    }
    let mut tray_icon = match tray_builder.build() {
        Ok(icon) => Some(icon),
        Err(err) => {
            tracing::warn!("Không thể tạo biểu tượng khay hệ thống (System Tray): {:?}", err);
            None
        }
    };

    let proxy_menu = proxy.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        match event.id.0.as_str() {
            "show" => {
                let _ = proxy_menu.send_event(DesktopEvent::ShowWindow);
            }
            "reload" => {
                let _ = proxy_menu.send_event(DesktopEvent::ReloadPage);
            }
            "browser" => {
                let _ = proxy_menu.send_event(DesktopEvent::OpenInBrowser);
            }
            "quit" => {
                let _ = proxy_menu.send_event(DesktopEvent::QuitApp);
            }
            _ => {}
        }
    }));

    let proxy_tray = proxy.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| match event {
        TrayIconEvent::DoubleClick { .. }
        | TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } => {
            let _ = proxy_tray.send_event(DesktopEvent::ShowWindow);
        }
        _ => {}
    }));

    let proxy_wake = proxy.clone();
    crate::web::set_desktop_wake_hook(move || {
        let _ = proxy_wake.send_event(DesktopEvent::ShowWindow);
    });

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let backend_handle = start_backend(proxy, shutdown_rx);
    let mut backend_handle_opt = Some(backend_handle);
    let mut shutdown_tx_opt = Some(shutdown_tx);
    let mut webview: Option<wry::WebView> = None;
    let mut webview_init_attempted = false;
    let mut webview_failed = false;
    let mut backend_ready = false;
    let mut backend_loaded = false;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => {
                if !webview_init_attempted {
                    webview_init_attempted = true;
                    match build_desktop_webview(&window, port) {
                        Ok(wv) => {
                            tracing::info!("✅ Đã tạo cửa sổ Desktop WebView2 thành công.");
                            if backend_ready && !backend_loaded {
                                backend_loaded = true;
                                let _ = wv.load_url(&admin_url);
                            }
                            webview = Some(wv);
                        }
                        Err(err) => {
                            tracing::warn!(
                                "⚠️ Không thể nhúng WebView2 (chuyển sang chế độ System Tray + Trình duyệt nền): {}",
                                err
                            );
                            webview_failed = true;
                            window.set_visible(false);
                            if backend_ready && !backend_loaded {
                                backend_loaded = true;
                                open_external_url(&browser_url);
                            }
                        }
                    }
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                // Bấm dấu X chỉ thu gọn xuống khay hệ thống (System Tray), giữ Bot & Tunnel chạy liên tục
                window.set_visible(false);
            }
            Event::UserEvent(DesktopEvent::BackendReady) => {
                backend_ready = true;
                if !backend_loaded {
                    if let Some(wv) = webview.as_ref() {
                        backend_loaded = true;
                        let _ = wv.load_url(&admin_url);
                    } else if webview_failed {
                        backend_loaded = true;
                        open_external_url(&browser_url);
                    }
                }
            }
            Event::UserEvent(DesktopEvent::ShowWindow) => {
                if let Some(wv) = webview.as_ref() {
                    window.set_visible(true);
                    window.set_minimized(false);
                    window.set_focus();
                    if backend_ready && !backend_loaded {
                        backend_loaded = true;
                        let _ = wv.load_url(&admin_url);
                    }
                } else {
                    open_external_url(&browser_url);
                }
            }
            Event::UserEvent(DesktopEvent::ReloadPage) => {
                if let Some(wv) = webview.as_ref() {
                    window.set_visible(true);
                    window.set_focus();
                    let _ = wv.load_url(&admin_url);
                } else {
                    open_external_url(&browser_url);
                }
            }
            Event::UserEvent(DesktopEvent::OpenInBrowser) => {
                open_external_url(&browser_url);
            }
            Event::UserEvent(DesktopEvent::QuitApp) => {
                if let Some(tx) = shutdown_tx_opt.take() {
                    let _ = tx.send(());
                }
                if let Some(handle) = backend_handle_opt.take() {
                    let _ = handle.join();
                }
                tray_icon.take();
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    });
}
