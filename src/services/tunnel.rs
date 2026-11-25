use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
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
}

static TUNNEL_STATE: OnceLock<RwLock<TunnelRuntimeSnapshot>> = OnceLock::new();
static TUNNEL_PROCESS: OnceLock<Mutex<Option<Child>>> = OnceLock::new();

fn state_lock() -> &'static RwLock<TunnelRuntimeSnapshot> {
    TUNNEL_STATE.get_or_init(|| RwLock::new(TunnelRuntimeSnapshot::default()))
}

fn proc_lock() -> &'static Mutex<Option<Child>> {
    TUNNEL_PROCESS.get_or_init(|| Mutex::new(None))
}

/// Đọc trạng thái Cloudflare Tunnel hiện tại đang chạy trong tiến trình bot.
pub fn snapshot_tunnel_state() -> TunnelRuntimeSnapshot {
    state_lock()
        .read()
        .map(|g| g.clone())
        .unwrap_or_default()
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
fn ensure_isolated_cf_config() -> &'static str {
    if let Some(parent) = Path::new(ISOLATED_CF_CONFIG_PATH).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(ISOLATED_CF_CONFIG_PATH, "no-autoupdate: true\n");
    ISOLATED_CF_CONFIG_PATH
}

/// Dừng sạch tiến trình Cloudflare Tunnel con do bot quản lý.
pub async fn stop_active_tunnel() {
    let mut guard = proc_lock().lock().await;
    if let Some(mut child) = guard.take() {
        let _ = child.kill().await;
    }
    cleanup_stale_pid_file();
    if let Ok(mut st) = state_lock().write() {
        st.quick_tunnel_url = None;
        st.is_running = false;
    }
}

/// Tự động khởi động hoặc đồng bộ lại Cloudflare Tunnel khi chạy bot trên PC (không cần thao tác tay).
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

    let Some(cf_exe) = find_cloudflared_executable() else {
        warn!("[TUNNEL] Không tìm thấy cloudflared.exe trên máy — bỏ qua tự động bật Tunnel.");
        return;
    };

    stop_active_tunnel().await;

    let isolated_cfg = ensure_isolated_cf_config();

    if let Some(token) = named_token {
        let mut cmd = Command::new(&cf_exe);
        cmd.args(["--config", isolated_cfg, "tunnel", "--no-autoupdate", "run"])
            .env("TUNNEL_TOKEN", token)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

        match cmd.spawn() {
            Ok(mut child) => {
                if let Some(pid) = child.id() {
                    write_pid_file(pid);
                }
                if let Some(stderr) = child.stderr.take() {
                    tokio::spawn(async move {
                        let mut lines = BufReader::new(stderr).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            if line.contains("Registered tunnel connection") {
                                info!("[TUNNEL] ✅ Cloudflare Named Tunnel đã kết nối thông suốt!");
                            }
                        }
                    });
                }
                if let Ok(mut st) = state_lock().write() {
                    st.quick_tunnel_url = None;
                    st.has_named_token = true;
                    st.is_running = true;
                }
                *proc_lock().lock().await = Some(child);
                info!("[TUNNEL] 🌐 Đã tự động kích hoạt Cloudflare Named Tunnel cố định.");
            }
            Err(err) => {
                warn!("[TUNNEL] Không thể khởi chạy Named Tunnel: {}", err);
            }
        }
        return;
    }

    // Chưa có Token cố định: tự động mở Cloudflare Quick Tunnel (trycloudflare.com) ngay lập tức để link bọc chạy 100% không cần bấm tay!
    let local_origin = format!("http://127.0.0.1:{}", config.port);
    let mut cmd = Command::new(&cf_exe);
    cmd.args([
        "--config",
        isolated_cfg,
        "tunnel",
        "--no-autoupdate",
        "--url",
        &local_origin,
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW

    let Ok(mut child) = cmd.spawn() else {
        warn!("[TUNNEL] Không thể khởi chạy cloudflared Quick Tunnel tự động.");
        return;
    };

    if let Some(pid) = child.id() {
        write_pid_file(pid);
    }

    struct ReadyTracker {
        url: Option<String>,
        registered: bool,
        sender: Option<tokio::sync::oneshot::Sender<String>>,
    }

    let tracker = std::sync::Arc::new(tokio::sync::Mutex::new(None::<ReadyTracker>));
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    *tracker.lock().await = Some(ReadyTracker {
        url: None,
        registered: false,
        sender: Some(tx),
    });

    fn spawn_stream_reader<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
        reader: R,
        tracker: std::sync::Arc<tokio::sync::Mutex<Option<ReadyTracker>>>,
    ) {
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let found_url = extract_trycloudflare_url(&line);
                let is_reg = line.contains("Registered tunnel connection");
                if found_url.is_none() && !is_reg {
                    continue;
                }
                let mut guard = tracker.lock().await;
                if let Some(ref mut st) = *guard {
                    if let Some(u) = found_url {
                        st.url = Some(u);
                    }
                    if is_reg {
                        st.registered = true;
                    }
                    if st.registered {
                        if let Some(url) = st.url.clone() {
                            if let Some(sender) = st.sender.take() {
                                let _ = sender.send(url);
                            }
                        }
                    }
                }
            }
        });
    }

    if let Some(stderr) = child.stderr.take() {
        spawn_stream_reader(stderr, tracker.clone());
    }
    if let Some(stdout) = child.stdout.take() {
        spawn_stream_reader(stdout, tracker.clone());
    }

    *proc_lock().lock().await = Some(child);

    let ready_url = match tokio::time::timeout(Duration::from_secs(15), rx).await {
        Ok(Ok(url)) => Some(url),
        _ => tracker.lock().await.as_ref().and_then(|st| st.url.clone()),
    };

    match ready_url {
        Some(live_url) => {
            if let Ok(mut st) = state_lock().write() {
                st.quick_tunnel_url = Some(live_url.clone());
                st.has_named_token = false;
                st.is_running = true;
            }
            info!(
                "[TUNNEL] 🌐 Đã tự động kích hoạt Quick Tunnel: {} (điền Cloudflare Tunnel Token trong Web Admin để dùng cố định https://thegioisieusale.click)",
                live_url
            );
        }
        None => {
            warn!("[TUNNEL] Quá thời gian chờ Quick Tunnel — kiểm tra lại mạng hoặc thử dán Tunnel Token vào Web Admin.");
        }
    }
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
}
