use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, Notify};
use tracing::{info, warn};

use crate::config::{is_shortlink_disabled_value, Config};
use crate::db::repository::Repository;
use crate::db::AppDb;

const PID_FILE_PATH: &str = "target/cf_tunnel.pid";
const ISOLATED_CF_CONFIG_PATH: &str = "target/cloudflared_isolated.yml";

#[derive(Clone, Debug, Default)]
pub struct TunnelRuntimeSnapshot {
    pub quick_tunnel_url: Option<String>,
    pub has_named_token: bool,
    pub is_running: bool,
    pub is_starting: bool,
}

static TUNNEL_STATE: OnceLock<RwLock<TunnelRuntimeSnapshot>> = OnceLock::new();
static TUNNEL_PROCESS: OnceLock<Mutex<Option<Child>>> = OnceLock::new();
static TUNNEL_NOTIFY: OnceLock<Notify> = OnceLock::new();
static TUNNEL_GEN: AtomicU64 = AtomicU64::new(0);

fn state_lock() -> &'static RwLock<TunnelRuntimeSnapshot> {
    TUNNEL_STATE.get_or_init(|| RwLock::new(TunnelRuntimeSnapshot::default()))
}

fn proc_lock() -> &'static Mutex<Option<Child>> {
    TUNNEL_PROCESS.get_or_init(|| Mutex::new(None))
}

fn tunnel_notify() -> &'static Notify {
    TUNNEL_NOTIFY.get_or_init(Notify::new)
}

/// Đọc trạng thái Cloudflare Tunnel hiện tại đang chạy trong tiến trình bot.
pub fn snapshot_tunnel_state() -> TunnelRuntimeSnapshot {
    state_lock()
        .read()
        .map(|g| g.clone())
        .unwrap_or_default()
}

/// Chờ ngắn nếu Quick Tunnel đang trong quá trình kết nối lúc mới khởi động để tin nhắn đầu tiên nhận ngay link bọc.
pub async fn wait_for_tunnel_if_starting(config: &Config, max_wait: Duration) -> TunnelRuntimeSnapshot {
    if config.public_base_url.is_some() {
        return snapshot_tunnel_state();
    }
    let deadline = tokio::time::Instant::now() + max_wait;
    loop {
        let snap = snapshot_tunnel_state();
        if snap.has_named_token || snap.quick_tunnel_url.is_some() || !snap.is_starting {
            return snap;
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return snap;
        }
        let notified = tunnel_notify().notified();
        tokio::select! {
            _ = notified => {}
            _ = tokio::time::sleep(deadline - now) => {
                return snapshot_tunnel_state();
            }
        }
    }
}

/// Trích xuất token Cloudflare Tunnel (bắt đầu bằng `eyJ...`) ngay cả khi Admin dán cả câu lệnh `cloudflared.exe service install eyJ...`.
pub fn extract_tunnel_token(raw: &str) -> Option<String> {
    for part in raw.split_whitespace() {
        let cleaned = part.trim_matches(|c| c == '"' || c == '\'' || c == ',');
        if cleaned.starts_with("eyJ")
            && cleaned.len() >= 20
            && cleaned
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'_' | b'-'))
        {
            return Some(cleaned.to_string());
        }
    }
    None
}

/// Trích xuất URL `https://<slug>.trycloudflare.com` từ dòng log của `cloudflared`.
pub fn extract_trycloudflare_url(line: &str) -> Option<String> {
    let idx = line.find("https://")?;
    let rest = &line[idx..];
    let url_end = rest
        .find(|c: char| c.is_whitespace() || matches!(c, '|' | '"' | '\'' | ')' | ']'))
        .unwrap_or(rest.len());
    let candidate = rest[..url_end].trim_end_matches('/');
    if candidate.starts_with("https://") && candidate.ends_with(".trycloudflare.com") {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn find_cloudflared_executable() -> Option<PathBuf> {
    let candidates = [
        r"C:\Program Files (x86)\cloudflared\cloudflared.EXE",
        r"C:\Program Files\cloudflared\cloudflared.EXE",
        r"C:\Program Files (x86)\cloudflared\cloudflared.exe",
        r"C:\Program Files\cloudflared\cloudflared.exe",
    ];
    for c in candidates {
        let p = Path::new(c);
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }
    #[cfg(target_os = "windows")]
    let which_cmd = "where";
    #[cfg(not(target_os = "windows"))]
    let which_cmd = "which";
    if let Ok(out) = std::process::Command::new(which_cmd)
        .arg("cloudflared")
        .output()
    {
        if out.status.success() {
            if let Some(first) = String::from_utf8_lossy(&out.stdout).lines().next() {
                let trimmed = first.trim();
                if !trimmed.is_empty() {
                    return Some(PathBuf::from(trimmed));
                }
            }
        }
    }
    None
}

fn cleanup_stale_pid_file() {
    if let Ok(raw) = std::fs::read_to_string(PID_FILE_PATH) {
        if let Ok(pid) = raw.trim().parse::<u32>() {
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("taskkill")
                    .args(["/F", "/PID", &pid.to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = std::process::Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .status();
            }
        }
        let _ = std::fs::remove_file(PID_FILE_PATH);
    }
}

fn write_pid_file(pid: u32) {
    if let Some(parent) = Path::new(PID_FILE_PATH).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(PID_FILE_PATH, pid.to_string());
}

/// Tạo file cấu hình riêng tối thiểu để `cloudflared` không đọc nhầm `~/.cloudflared/config.yml` của dự án khác.
fn ensure_isolated_cf_config() -> PathBuf {
    let path = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(ISOLATED_CF_CONFIG_PATH);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, "no-autoupdate: true\n");
    path
}

async fn kill_managed_child() {
    let child_opt = { proc_lock().lock().await.take() };
    if let Some(mut child) = child_opt {
        let _ = child.kill().await;
    }
    cleanup_stale_pid_file();
}

/// Dừng sạch tiến trình Cloudflare Tunnel con do bot quản lý và hủy supervisor tự khởi động lại.
pub async fn stop_active_tunnel() {
    TUNNEL_GEN.fetch_add(1, Ordering::SeqCst);
    kill_managed_child().await;
    if let Ok(mut st) = state_lock().write() {
        st.quick_tunnel_url = None;
        st.has_named_token = false;
        st.is_running = false;
        st.is_starting = false;
    }
    tunnel_notify().notify_waiters();
}

#[derive(Default)]
struct QuickReadyState {
    url: Option<String>,
    registered: bool,
    announced: bool,
}

fn is_important_cf_error_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    line.contains(" ERR ")
        || lower.contains("failed to request quick tunnel")
        || lower.contains("too many")
        || lower.contains("unable to reach")
        || lower.contains("unauthorized")
}

fn spawn_quick_stream_reader<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    reader: R,
    gen_id: u64,
    ready: Arc<Mutex<QuickReadyState>>,
) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if TUNNEL_GEN.load(Ordering::SeqCst) != gen_id {
                break;
            }
            let found_url = extract_trycloudflare_url(&line);
            let is_reg = line.contains("Registered tunnel connection");
            if found_url.is_none() && !is_reg {
                if is_important_cf_error_line(&line) {
                    warn!("[TUNNEL] {}", line.trim());
                }
                continue;
            }
            let mut guard = ready.lock().await;
            if let Some(u) = found_url {
                guard.url = Some(u);
            }
            if is_reg {
                guard.registered = true;
            }
            if guard.registered && !guard.announced {
                if let Some(live_url) = guard.url.clone() {
                    guard.announced = true;
                    drop(guard);
                    tokio::spawn(async move {
                        wait_for_quick_tunnel_dns_ready(&live_url, gen_id).await;
                        if TUNNEL_GEN.load(Ordering::SeqCst) != gen_id {
                            return;
                        }
                        if let Ok(mut st) = state_lock().write() {
                            st.quick_tunnel_url = Some(live_url.clone());
                            st.has_named_token = false;
                            st.is_running = true;
                            st.is_starting = false;
                        }
                        tunnel_notify().notify_waiters();
                        info!(
                            "[TUNNEL] 🌐 Đã tự động kích hoạt Quick Tunnel: {} (điền Cloudflare Tunnel Token trong Web Admin để dùng cố định https://thegioisieusale.click)",
                            live_url
                        );
                    });
                }
            }
        }
    });
}

/// Xác minh qua Cloudflare DoH rằng bản ghi DNS của Quick Tunnel đã lan truyền trước khi phát link cho người dùng.
async fn wait_for_quick_tunnel_dns_ready(live_url: &str, gen_id: u64) {
    let Some(host) = live_url.strip_prefix("https://").map(str::trim).filter(|h| !h.is_empty()) else {
        return;
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let doh_url = format!("https://cloudflare-dns.com/dns-query?name={}&type=A", host);

    for attempt in 0..8 {
        if TUNNEL_GEN.load(Ordering::SeqCst) != gen_id {
            return;
        }
        if let Ok(resp) = client
            .get(&doh_url)
            .header("Accept", "application/dns-json")
            .send()
            .await
        {
            if let Ok(text) = resp.text().await {
                if text.contains("\"Answer\"") {
                    tokio::time::sleep(Duration::from_millis(800)).await;
                    return;
                }
            }
        } else if attempt >= 3 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(900)).await;
    }
}

async fn run_tunnel_supervisor(
    gen_id: u64,
    port: u16,
    named_token: Option<String>,
    cf_exe: PathBuf,
) {
    let isolated_cfg = ensure_isolated_cf_config();
    let cfg_arg = isolated_cfg.to_string_lossy().to_string();
    let mut backoff_secs: u64 = 3;

    while TUNNEL_GEN.load(Ordering::SeqCst) == gen_id {
        kill_managed_child().await;
        if TUNNEL_GEN.load(Ordering::SeqCst) != gen_id {
            break;
        }

        if let Ok(mut st) = state_lock().write() {
            st.quick_tunnel_url = None;
            st.has_named_token = named_token.is_some();
            st.is_running = false;
            st.is_starting = true;
        }

        let attempt_started = Instant::now();
        let spawn_res = if let Some(ref token) = named_token {
            let mut cmd = Command::new(&cf_exe);
            cmd.args(["--config", &cfg_arg, "tunnel", "--no-autoupdate", "run"])
                .env("TUNNEL_TOKEN", token)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            #[cfg(target_os = "windows")]
            cmd.creation_flags(0x08000000);
            cmd.spawn()
        } else {
            let local_origin = format!("http://127.0.0.1:{}", port);
            let mut cmd = Command::new(&cf_exe);
            cmd.args([
                "--config",
                &cfg_arg,
                "tunnel",
                "--no-autoupdate",
                "--url",
                &local_origin,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
            #[cfg(target_os = "windows")]
            cmd.creation_flags(0x08000000);
            cmd.spawn()
        };

        let mut child = match spawn_res {
            Ok(c) => c,
            Err(err) => {
                warn!("[TUNNEL] Không thể khởi chạy cloudflared ({}), thử lại sau {}s...", err, backoff_secs);
                if let Ok(mut st) = state_lock().write() {
                    st.is_starting = false;
                }
                tunnel_notify().notify_waiters();
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(30);
                continue;
            }
        };

        if let Some(pid) = child.id() {
            write_pid_file(pid);
        }

        if named_token.is_some() {
            if let Some(stderr) = child.stderr.take() {
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        if TUNNEL_GEN.load(Ordering::SeqCst) != gen_id {
                            break;
                        }
                        if line.contains("Registered tunnel connection") {
                            if let Ok(mut st) = state_lock().write() {
                                st.has_named_token = true;
                                st.is_running = true;
                                st.is_starting = false;
                            }
                            tunnel_notify().notify_waiters();
                            info!("[TUNNEL] ✅ Cloudflare Named Tunnel đã kết nối thông suốt!");
                        } else if is_important_cf_error_line(&line) {
                            warn!("[TUNNEL] {}", line.trim());
                        }
                    }
                });
            }
            if let Ok(mut st) = state_lock().write() {
                st.quick_tunnel_url = None;
                st.has_named_token = true;
                st.is_running = true;
                st.is_starting = false;
            }
            tunnel_notify().notify_waiters();
            info!("[TUNNEL] 🌐 Đã tự động kích hoạt Cloudflare Named Tunnel cố định.");
        } else {
            let ready = Arc::new(Mutex::new(QuickReadyState::default()));
            if let Some(stderr) = child.stderr.take() {
                spawn_quick_stream_reader(stderr, gen_id, ready.clone());
            }
            if let Some(stdout) = child.stdout.take() {
                spawn_quick_stream_reader(stdout, gen_id, ready);
            }
        }

        *proc_lock().lock().await = Some(child);

        // Giám sát tiến trình: nếu Quick Tunnel chưa lên sau 20s hoặc cloudflared tự thoát sẽ tự động khởi động lại
        let mut timed_out_without_connect = false;
        let mut exited_status: Option<std::process::ExitStatus> = None;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if TUNNEL_GEN.load(Ordering::SeqCst) != gen_id {
                break;
            }
            {
                let mut guard = proc_lock().lock().await;
                if let Some(ref mut ch) = *guard {
                    match ch.try_wait() {
                        Ok(Some(status)) => {
                            exited_status = Some(status);
                            *guard = None;
                            break;
                        }
                        Ok(None) => {}
                        Err(_) => {
                            *guard = None;
                            break;
                        }
                    }
                } else {
                    break;
                }
            }
            if named_token.is_none()
                && attempt_started.elapsed() >= Duration::from_secs(20)
                && snapshot_tunnel_state().quick_tunnel_url.is_none()
            {
                timed_out_without_connect = true;
                break;
            }
        }

        if TUNNEL_GEN.load(Ordering::SeqCst) != gen_id {
            break;
        }

        if timed_out_without_connect {
            warn!(
                "[TUNNEL] Quick Tunnel chờ quá 20s chưa cấp URL — tự động khởi động lại sau {}s...",
                backoff_secs
            );
            kill_managed_child().await;
        } else if let Some(status) = exited_status {
            warn!(
                "[TUNNEL] cloudflared đã dừng ({}) — tự động kết nối lại sau {}s...",
                status, backoff_secs
            );
        }

        if let Ok(mut st) = state_lock().write() {
            st.quick_tunnel_url = None;
            st.is_running = false;
            st.is_starting = false;
        }
        tunnel_notify().notify_waiters();

        if attempt_started.elapsed() >= Duration::from_secs(60) {
            backoff_secs = 3;
        }
        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(30);
    }
}

/// Tự động khởi động hoặc đồng bộ lại Cloudflare Tunnel chạy nền khi chạy bot trên PC (tự phục hồi khi rớt mạng).
pub async fn ensure_tunnel_running(db: &AppDb, config: &Config) {
    if config.public_base_url.is_some() {
        return;
    }

    let (db_short, db_token) = match db.conn() {
        Ok(conn) => {
            let short = Repository::get_setting(&conn, "shortlink_base_url")
                .await
                .ok()
                .flatten();
            let tok = Repository::get_setting(&conn, "cloudflare_tunnel_token")
                .await
                .ok()
                .flatten()
                .filter(|s| !s.trim().is_empty());
            (short, tok)
        }
        Err(_) => (None, None),
    };

    if db_short
        .as_deref()
        .map(is_shortlink_disabled_value)
        .unwrap_or(false)
        || config.shortlink_base_url.is_none()
    {
        stop_active_tunnel().await;
        return;
    }

    let env_token = std::env::var("CLOUDFLARE_TUNNEL_TOKEN")
        .ok()
        .and_then(|s| extract_tunnel_token(&s));
    let named_token = db_token
        .as_deref()
        .and_then(extract_tunnel_token)
        .or(env_token);

    stop_active_tunnel().await;

    let Some(cf_exe) = find_cloudflared_executable() else {
        warn!("[TUNNEL] Không tìm thấy cloudflared.exe trên máy — bot sẽ gửi link sạch trực tiếp.");
        return;
    };

    let gen_id = TUNNEL_GEN.load(Ordering::SeqCst);
    if let Ok(mut st) = state_lock().write() {
        st.has_named_token = named_token.is_some();
        st.is_starting = true;
    }

    let port = config.port;
    tokio::spawn(async move {
        run_tunnel_supervisor(gen_id, port, named_token, cf_exe).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tunnel_token_supports_raw_and_full_command() {
        let raw_tok = "eyJhIjoiMTIzNDU2Nzg5MDEyMzQ1Njc4OTAifQ==";
        assert_eq!(extract_tunnel_token(raw_tok).as_deref(), Some(raw_tok));
        let cmd_str = format!("  cloudflared.exe service install \"{}\"  ", raw_tok);
        assert_eq!(extract_tunnel_token(&cmd_str).as_deref(), Some(raw_tok));
        assert_eq!(extract_tunnel_token("khong-hop-le"), None);
    }

    #[test]
    fn test_extract_trycloudflare_url() {
        let line = "2026-11-25T08:06:35Z INF |  https://deployment-effect-meters-vacuum.trycloudflare.com  |";
        assert_eq!(
            extract_trycloudflare_url(line).as_deref(),
            Some("https://deployment-effect-meters-vacuum.trycloudflare.com")
        );
        assert_eq!(extract_trycloudflare_url("no url here"), None);
    }

    #[tokio::test]
    async fn test_wait_for_tunnel_skips_in_cloud_mode() {
        let cfg = Config {
            telegram_token: "t".to_string(),
            bot_username: "b".to_string(),
            database_path: ":memory:".to_string(),
            turso_url: None,
            turso_auth_token: None,
            default_cashback_rate: 70.0,
            default_referral_bonus_rate: 10.0,
            min_withdrawal_vnd: 50000.0,
            accesstrade_deeplink_template: String::new(),
            accesstrade_api_token: None,
            shopee_affiliate_id: None,
            shopee_app_id: None,
            shopee_app_secret: None,
            shopee_social_params: None,
            admin_username: "admin".to_string(),
            admin_password: "pass".to_string(),
            admin_telegram_id: None,
            port: 10000,
            public_base_url: Some("https://example.onrender.com".to_string()),
            shortlink_base_url: None,
            telegram_webhook_secret: "sec".to_string(),
        };
        let started = Instant::now();
        let _snap = wait_for_tunnel_if_starting(&cfg, Duration::from_secs(5)).await;
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
