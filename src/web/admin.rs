use super::*;

fn render_platform_status(label: &str, matched: &Option<(String, String)>, manual_id: &str, backup_note: Option<&str>) -> String {
    if !manual_id.trim().is_empty() {
        return format!(
            "<div class=\"plat-row\">\
                <div class=\"plat-row__left\">\
                    <div class=\"plat-row__name\">{}</div>\
                    <div class=\"plat-row__sub\">ID thủ công: <code>{}</code></div>\
                </div>\
                <span class=\"badge badge--ok\">🟢 Thủ công</span>\
             </div>",
            label,
            escape_html(manual_id.trim())
        );
    }
    match matched {
        Some((id, _)) => format!(
            "<div class=\"plat-row\">\
                <div class=\"plat-row__left\">\
                    <div class=\"plat-row__name\">{}</div>\
                    <div class=\"plat-row__sub\">Campaign: <code>{}</code></div>\
                </div>\
                <span class=\"badge badge--ok\">🟢 Đã duyệt</span>\
             </div>",
            label,
            escape_html(id)
        ),
        None => {
            if let Some(note) = backup_note {
                format!(
                    "<div class=\"plat-row\">\
                        <div class=\"plat-row__left\">\
                            <div class=\"plat-row__name\">{}</div>\
                            <div class=\"plat-row__sub\">{}</div>\
                        </div>\
                        <span class=\"badge badge--info\">🔵 Sẵn sàng</span>\
                     </div>",
                    label,
                    escape_html(note)
                )
            } else {
                format!(
                    "<div class=\"plat-row\">\
                        <div class=\"plat-row__left\">\
                            <div class=\"plat-row__name\">{}</div>\
                            <div class=\"plat-row__sub\">Chưa duyệt trên pub2.accesstrade.vn</div>\
                        </div>\
                        <span class=\"badge badge--warn\">🟡 Chưa duyệt AT</span>\
                     </div>",
                    label
                )
            }
        }
    }
}

#[derive(serde::Deserialize, Default)]
pub(super) struct DashboardQuery {
    notice: Option<String>,
    q: Option<String>,
    plat: Option<String>,
}

async fn resolve_accesstrade_token(conn: &libsql::Connection, config: &Config) -> Option<String> {
    Repository::get_setting(conn, "accesstrade_api_token")
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| config.accesstrade_api_token.clone())
}

fn format_timestamp_vn(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "-".to_string();
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        let vn = dt.with_timezone(&Utc) + chrono::Duration::hours(7);
        return vn.format("%H:%M:%S · %d/%m/%Y").to_string();
    }
    let no_frac = trimmed.split('.').next().unwrap_or(trimmed);
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(no_frac, "%Y-%m-%dT%H:%M:%S") {
        let vn = naive + chrono::Duration::hours(7);
        return vn.format("%H:%M:%S · %d/%m/%Y").to_string();
    }
    trimmed.to_string()
}

const ADMIN_SVG_SPRITE: &str = r#"<svg aria-hidden="true" style="position:absolute;width:0;height:0;overflow:hidden">
    <defs>
        <symbol id="ico-overview" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <rect x="3" y="3" width="7" height="7" rx="1.8"/><rect x="14" y="3" width="7" height="7" rx="1.8"/>
            <rect x="14" y="14" width="7" height="7" rx="1.8"/><rect x="3" y="14" width="7" height="7" rx="1.8"/>
        </symbol>
        <symbol id="ico-platforms" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/>
            <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>
        </symbol>
        <symbol id="ico-vouchers" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M2 9a3 3 0 0 1 0 6v2a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-2a3 3 0 0 1 0-6V7a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2Z"/>
            <path d="M13 5v2"/><path d="M13 17v2"/><path d="M13 11v2"/>
        </symbol>
        <symbol id="ico-orders" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/>
        </symbol>
        <symbol id="ico-users" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/>
            <path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/>
        </symbol>
        <symbol id="ico-clicks" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>
        </symbol>
        <symbol id="ico-flame" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.072-2.143-.224-4.054 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.153.433-2.294 1-3a2.5 2.5 0 0 0 2.5 2.5z"/>
        </symbol>
        <symbol id="ico-clock" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="9"/><polyline points="12 7 12 12 15.5 14"/>
        </symbol>
        <symbol id="ico-bot" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 8V4H8"/><rect width="16" height="12" x="4" y="8" rx="2"/>
            <path d="M2 14h2"/><path d="M20 14h2"/><path d="M15 13v2"/><path d="M9 13v2"/>
        </symbol>
        <symbol id="ico-history" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l4 2"/>
        </symbol>
        <symbol id="ico-chart" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <line x1="18" y1="20" x2="18" y2="10"/><line x1="12" y1="20" x2="12" y2="4"/><line x1="6" y1="20" x2="6" y2="14"/>
        </symbol>
        <symbol id="ico-pie" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M21.21 15.89A10 10 0 1 1 8 2.83"/><path d="M22 12A10 10 0 0 0 12 2v10z"/>
        </symbol>
        <symbol id="ico-rocket" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z"/>
            <path d="m12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z"/>
            <path d="M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0"/><path d="M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5"/>
        </symbol>
        <symbol id="ico-crown" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="m2 4 3 12h14l3-12-6 7-4-7-4 7-6-7zm3 16h14"/>
        </symbol>
        <symbol id="ico-globe" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/>
            <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/>
        </symbol>
        <symbol id="ico-shield" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="m9 12 2 2 4-4"/>
        </symbol>
        <symbol id="ico-moon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>
        </symbol>
        <symbol id="ico-sun" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/>
            <path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/>
        </symbol>
    </defs>
</svg>"#;

fn render_signal_meter_svg(strength: u8) -> String {
    let lvl = strength.min(3);
    let color = if lvl >= 3 {
        "var(--ok-text)"
    } else if lvl >= 1 {
        "var(--brand-bright)"
    } else {
        "var(--border-strong)"
    };
    let bar = |idx: u8, h: u8, y: u8| -> String {
        let fill = if lvl >= idx { color } else { "var(--border-strong)" };
        format!(
            r#"<rect x="{}" y="{}" width="4" height="{}" rx="1.5" fill="{}"/>"#,
            (idx - 1) * 6 + 1,
            y,
            h,
            fill
        )
    };
    format!(
        r#"<span class="signal-meter" title="Độ mạnh chữ ký FB/IG: {}/3">
            <svg width="20" height="16" viewBox="0 0 20 16" aria-hidden="true">{}{}{}</svg>
            <span>Mức {}/3</span>
        </span>"#,
        lvl,
        bar(1, 6, 10),
        bar(2, 10, 6),
        bar(3, 14, 2),
        lvl
    )
}

fn render_dashboard_charts_html(
    stats: &crate::db::models::AdminStats,
    total_market_vouchers: usize,
    high_pct_vouchers: usize,
    shopee_vouchers: usize,
    tiki_vouchers: usize,
    exclusive_count: usize,
) -> String {
    let shopee_c = stats.shopee_clicks.max(0) as u64;
    let lazada_c = stats.lazada_clicks.max(0) as u64;
    let tiktok_c = stats.tiktok_clicks.max(0) as u64;
    let tiki_c = stats.tiki_clicks.max(0) as u64;
    let sum_clicks = (shopee_c + lazada_c + tiktok_c + tiki_c).max(stats.total_clicks.max(0) as u64);

    let platforms = [
        ("Shopee", shopee_c, "#EA580C"),
        ("Lazada", lazada_c, "#2563EB"),
        ("TikTok Shop", tiktok_c, "#DB2777"),
        ("Tiki", tiki_c, "#0284C7"),
    ];
    let mut bars_html = String::new();
    for (name, count, color) in platforms {
        let pct = if sum_clicks > 0 {
            ((count as f64 / sum_clicks as f64) * 100.0).round() as u64
        } else {
            0
        };
        let width_pct = if count > 0 { pct.max(4) } else { 0 };
        bars_html.push_str(&format!(
            r#"<div class="chart-bar-row">
                <div class="chart-bar-label">
                    <span class="plat-dot" style="background:{};"></span>
                    <span>{}</span>
                </div>
                <div class="bar-track" role="img" aria-label="{}: {} lượt ({}%)">
                    <div class="bar-fill" style="width:{}%;background:{};"></div>
                </div>
                <div class="chart-bar-val"><strong>{}</strong> <span>{}%</span></div>
            </div>"#,
            color,
            name,
            name,
            count,
            pct,
            width_pct,
            color,
            count,
            pct
        ));
    }

    // Donut chart SVG (r = 44 => C = 2 * PI * 44 = 276.46)
    let circumference = 276.46_f64;
    let other_v = total_market_vouchers
        .saturating_sub(shopee_vouchers)
        .saturating_sub(tiki_vouchers)
        + exclusive_count;
    let pool_total = shopee_vouchers + tiki_vouchers + other_v;
    let seg_calc = |val: usize| -> (f64, u64) {
        if pool_total == 0 {
            return (0.0, 0);
        }
        let ratio = val as f64 / pool_total as f64;
        (ratio * circumference, (ratio * 100.0).round() as u64)
    };
    let (len_s, pct_s) = seg_calc(shopee_vouchers);
    let (len_t, pct_t) = seg_calc(tiki_vouchers);
    let (len_o, pct_o) = seg_calc(other_v);
    let deep_pct = if total_market_vouchers > 0 {
        ((high_pct_vouchers as f64 / total_market_vouchers as f64) * 100.0).round() as u64
    } else {
        0
    };

    let donut_segments = if pool_total == 0 {
        String::new()
    } else {
        format!(
            r##"<circle cx="60" cy="60" r="44" fill="none" stroke="#EA580C" stroke-width="12" stroke-dasharray="{:.2} {:.2}" stroke-dashoffset="0" stroke-linecap="butt"/>
               <circle cx="60" cy="60" r="44" fill="none" stroke="#0284C7" stroke-width="12" stroke-dasharray="{:.2} {:.2}" stroke-dashoffset="{:.2}" stroke-linecap="butt"/>
               <circle cx="60" cy="60" r="44" fill="none" stroke="#7C3AED" stroke-width="12" stroke-dasharray="{:.2} {:.2}" stroke-dashoffset="{:.2}" stroke-linecap="butt"/>"##,
            len_s,
            circumference,
            len_t,
            circumference,
            -len_s,
            len_o,
            circumference,
            -(len_s + len_t)
        )
    };

    format!(
        r##"<section class="charts-grid" aria-label="Biểu đồ trực quan hệ thống">
            <article class="card chart-card">
                <div class="chart-head">
                    <div class="chart-head__title">
                        <span class="chart-ico chart-ico--brand"><svg class="ico"><use href="#ico-chart"/></svg></span>
                        <div>
                            <h3>Lưu Lượng Dán Link Theo Sàn TMĐT</h3>
                            <p>Tỷ trọng chuyển đổi thực tế trên toàn hệ thống</p>
                        </div>
                    </div>
                    <span class="badge badge--muted font-mono">Tổng: {} lượt</span>
                </div>
                <div class="chart-bars">{}</div>
            </article>

            <article class="card chart-card">
                <div class="chart-head">
                    <div class="chart-head__title">
                        <span class="chart-ico chart-ico--purple"><svg class="ico"><use href="#ico-pie"/></svg></span>
                        <div>
                            <h3>Cơ Cấu Kho Mã &amp; Sức Mạnh Săn Sale</h3>
                            <p>Phân bổ kho ACCESSTRADE &amp; mã độc quyền FB/IG</p>
                        </div>
                    </div>
                    <span class="badge badge--orange">Giảm ≥20%: {} mã ({}%)</span>
                </div>
                <div class="donut-layout">
                    <div class="donut-wrap">
                        <svg class="chart-donut" viewBox="0 0 120 120" width="120" height="120" role="img" aria-label="Biểu đồ tròn phân bổ kho mã">
                            <circle cx="60" cy="60" r="44" fill="none" stroke="var(--bg-inset)" stroke-width="12"/>
                            <g transform="rotate(-90 60 60)">{}</g>
                        </svg>
                        <div class="donut-center">
                            <strong>{}</strong>
                            <span>Tổng mã</span>
                        </div>
                    </div>
                    <div class="donut-legend">
                        <div class="legend-item">
                            <span class="legend-dot" style="background:#EA580C;"></span>
                            <span class="legend-name">Shopee Thị Trường</span>
                            <strong>{}</strong><span class="legend-pct">({}%)</span>
                        </div>
                        <div class="legend-item">
                            <span class="legend-dot" style="background:#0284C7;"></span>
                            <span class="legend-name">Tiki Creator / Mall</span>
                            <strong>{}</strong><span class="legend-pct">({}%)</span>
                        </div>
                        <div class="legend-item">
                            <span class="legend-dot" style="background:#7C3AED;"></span>
                            <span class="legend-name">Độc Quyền &amp; Sàn Khác</span>
                            <strong>{}</strong><span class="legend-pct">({}%)</span>
                        </div>
                    </div>
                </div>
            </article>
        </section>"##,
        sum_clicks,
        bars_html,
        high_pct_vouchers,
        deep_pct,
        donut_segments,
        pool_total,
        shopee_vouchers,
        pct_s,
        tiki_vouchers,
        pct_t,
        other_v,
        pct_o
    )
}

/// Giao diện chính Dashboard Quản Trị
pub(super) async fn handle_dashboard(
    headers: HeaderMap,
    Query(query): Query<DashboardQuery>,
    State(state): State<AppState>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    let conn = match state.db.conn() {
        Ok(c) => c,
        Err(e) => {
            return Html(format!("<h1>Lỗi kết nối cơ sở dữ liệu: {}</h1>", escape_html(&e.to_string()))).into_response();
        }
    };

    let stats = Repository::get_admin_dashboard_stats(&conn).await.unwrap_or_default();
    let clicks = Repository::get_recent_clicks_for_admin(&conn, 50).await.unwrap_or_default();
    let saved_deals = Repository::get_all_deals_for_admin(&conn).await.unwrap_or_default();

    let settings_map = state.db.get_settings_map_cached().await.unwrap_or_default();
    let get_setting_val = |key: &str| -> Option<String> {
        settings_map
            .get(key)
            .cloned()
            .filter(|s| !s.trim().is_empty())
    };

    let affiliate_template = settings_map
        .get("affiliate_template")
        .cloned()
        .unwrap_or_default();

    let db_at_token = get_setting_val("accesstrade_api_token")
        .or_else(|| state.config.accesstrade_api_token.clone());

    let has_turso = state.config.turso_url.is_some() && state.config.turso_auth_token.is_some();
    let has_env_token = state.config.accesstrade_api_token.is_some();
    let is_cloud_runtime = state.config.public_base_url.is_some();
    let csrf_token = crate::web::auth::extract_cookie_value(&headers, "dealx_csrf")
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let auth_action_html = if is_trusted_local_admin_header(&state, &headers) {
        r#"<span class="badge badge--ok">🖥️ PC Local (Miễn đăng nhập)</span>"#.to_string()
    } else {
        r#"<a href="/admin/logout" class="btn btn--danger btn--sm">Đăng Xuất</a>"#.to_string()
    };
    let persistence_banner = if !is_cloud_runtime && !has_turso {
        format!(
            r#"<div class="notice-banner notice-banner--ok">
                <span>💻</span>
                <span><b>Đang chạy trực tiếp trên PC (Local):</b> Dữ liệu được lưu bền vững trên máy tính tại <code>{}</code> · Bot chạy chế độ <b>Long Polling</b> và gọi API trực tiếp từ IP của bạn.</span>
            </div>"#,
            escape_html(&state.config.database_path)
        )
    } else if !has_turso {
        r#"<div class="notice-banner notice-banner--warn">
            <span>⚠️</span>
            <span><b>Chưa gắn Turso Cloud Database:</b> Khi chạy Hosting/Cloud bắt buộc phải thêm <code>TURSO_DATABASE_URL</code> &amp; <code>TURSO_AUTH_TOKEN</code> vào Environment (hệ thống không còn fallback về file SQLite tạm).</span>
        </div>"#.to_string()
    } else if !has_env_token {
        r#"<div class="notice-banner notice-banner--warn">
            <span>⚠️</span>
            <span><b>Đã kết nối Turso Cloud:</b> Nên thêm cố định <code>ACCESSTRADE_API_TOKEN</code> vào Environment của Hosting để không phải nhập lại sau mỗi lần deploy.</span>
        </div>"#.to_string()
    } else {
        r#"<div class="notice-banner notice-banner--ok">
            <span>🟢</span>
            <span><b>Hệ thống lưu trữ bền vững:</b> Đã kết nối Turso Cloud Database &amp; cố định API Token an toàn.</span>
        </div>"#.to_string()
    };

    let default_pwd_warning = if is_cloud_runtime && state.config.admin_password == "DealX@Admin2026" {
        r#"<div class="notice-banner notice-banner--warn"><span>🚨</span><span><b>CẢNH BÁO BẢO MẬT CLOUD:</b> Hệ thống đang chạy Cloud với mật khẩu mặc định! Hãy đặt biến môi trường <code>ADMIN_PASSWORD</code> mạnh ngay để bảo vệ trang quản trị.</span></div>"#
    } else {
        ""
    };

    let notice_banner = match query.notice.as_deref() {
        Some(n) if !n.trim().is_empty() => format!(
            r#"{}<div class="notice-banner notice-banner--info"><span>ℹ️</span><span>{}</span></div>"#,
            default_pwd_warning,
            escape_html(n)
        ),
        _ => default_pwd_warning.to_string(),
    };

    let camp_shopee = settings_map.get("campaign_shopee").cloned().unwrap_or_default();
    let camp_lazada = settings_map.get("campaign_lazada").cloned().unwrap_or_default();
    let camp_tiki = settings_map.get("campaign_tiki").cloned().unwrap_or_default();
    let camp_tiktok = settings_map.get("campaign_tiktok").cloned().unwrap_or_default();

    let shopee_affiliate_id = get_setting_val("shopee_affiliate_id")
        .or_else(|| state.config.shopee_affiliate_id.clone())
        .unwrap_or_default();
    let shopee_app_id = get_setting_val("shopee_app_id")
        .or_else(|| state.config.shopee_app_id.clone())
        .unwrap_or_default();
    let shopee_app_secret = get_setting_val("shopee_app_secret")
        .or_else(|| state.config.shopee_app_secret.clone())
        .unwrap_or_default();
    let shopee_social_params = get_setting_val("shopee_social_params")
        .or_else(|| state.config.shopee_social_params.clone())
        .unwrap_or_default();
    let social_deals_count = saved_deals
        .iter()
        .filter(|d| {
            d.is_active
                && d.platform.eq_ignore_ascii_case("Shopee")
                && d.coupon_code
                    .as_deref()
                    .map(|c| c.starts_with("KOL_") || c.starts_with("SHP_AUTO_"))
                    .unwrap_or(false)
        })
        .count();
    let social_sync_snap = crate::services::social_harvester::snapshot_social_status();
    let social_sig_strength = crate::services::shopee_affiliate::social_param_strength(&shopee_social_params);
    let social_harvester_status_html = if social_sig_strength >= 3 {
        format!(
            r#"<span class="badge badge--ok">🟢 Auto FB/IG đầy đủ (sm_sig_id · mức 3/3) · {} mã FB/IG/KOL</span>"#,
            social_deals_count
        )
    } else if social_sig_strength >= 2 {
        format!(
            r#"<span class="badge badge--ok">🟢 Auto FB/IG cơ bản (mức 2/3) · {} mã FB/IG/KOL</span>"#,
            social_deals_count
        )
    } else if social_deals_count > 0 {
        format!(
            r#"<span class="badge badge--info">🎁 Đã tự nhặt {} mã Shopee FB/IG/KOL</span>"#,
            social_deals_count
        )
    } else {
        r#"<span class="badge badge--info">🔄 Đang tự động quét kênh FB/IG...</span>"#.to_string()
    };
    let social_last_display = if !social_sync_snap.last_sync_vn.is_empty() {
        format!("{} (Chu kỳ #{})", social_sync_snap.last_sync_vn, social_sync_snap.sync_cycles)
    } else if social_sync_snap.is_syncing {
        "Đang quét ngay khi khởi động...".to_string()
    } else {
        "Đang chạy ngầm...".to_string()
    };
    let signal_meter_html = render_signal_meter_svg(social_sig_strength);
    let social_auto_panel_html = format!(
        r##"<div class="social-meta">
            <div style="display:flex;align-items:center;gap:0.5rem;flex-wrap:wrap;justify-content:flex-end;">
                {}
                <span class="badge badge--ok"><svg class="ico ico--xs"><use href="#ico-bot"/></svg> Tự Động Mỗi 5 Phút</span>
            </div>
            <span class="muted-text">Lần cuối: <strong class="meta-time">{}</strong></span>
        </div>"##,
        signal_meter_html,
        escape_html(&social_last_display)
    );
    let shortlink_db_raw = settings_map.get("shortlink_base_url").cloned();
    let db_cf_token = get_setting_val("cloudflare_tunnel_token");
    let env_cf_token = std::env::var("CLOUDFLARE_TUNNEL_TOKEN")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let has_cf_tunnel_token = db_cf_token.is_some() || env_cf_token.is_some();
    let tunnel_snap = crate::services::tunnel::snapshot_tunnel_state();
    let shortlink_base_url = crate::config::resolve_pc_runtime_shortlink_base_url(
        shortlink_db_raw.as_deref(),
        state.config.shortlink_base_url.as_deref(),
        None,
        tunnel_snap.quick_tunnel_url.as_deref(),
        tunnel_snap.has_named_token,
    )
    .unwrap_or_default();
    let shortlink_mobile_only = settings_map
        .get("shortlink_mobile_only")
        .map(|s| s.trim() == "1")
        .unwrap_or(false);
    let public_home_cloak = settings_map
        .get("public_home_cloak")
        .map(|s| s.trim() == "1")
        .unwrap_or(false);
    let derived_tunnel_origin = extract_host_only(&shortlink_base_url)
        .map(|h| format!("https://origin.{}", h))
        .unwrap_or_else(|| "https://origin.thegioisieusale.click".to_string());
    let origin_base_for_worker = state
        .config
        .public_base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&derived_tunnel_origin);

    let admin_telegram_id_val = get_setting_val("admin_telegram_id")
        .or_else(|| state.config.admin_telegram_id.map(|id| id.to_string()))
        .unwrap_or_default();
    let tracked_orders = Repository::get_recent_tracked_orders(&conn, 20)
        .await
        .unwrap_or_default();

    let shopee_card_html = render_shopee_card(ShopeeCardView {
        aff_id: &shopee_affiliate_id,
        app_id: &shopee_app_id,
        app_secret: &shopee_app_secret,
        social_params: &shopee_social_params,
        shortlink_base_url: &shortlink_base_url,
        shortlink_mobile_only,
        public_home_cloak,
        origin_base_url: origin_base_for_worker,
        bot_username: &state.config.bot_username,
        has_cf_tunnel_token,
        quick_tunnel_url: tunnel_snap.quick_tunnel_url.as_deref(),
        tunnel_is_running: tunnel_snap.is_running,
    });
    let order_alerts_card_html = render_order_alerts_card(&admin_telegram_id_val, &tracked_orders);

    let mut approved_opt: Option<ApprovedCampaigns> = None;
    let (at_status_badge, campaign_status_html) = if let Some(ref token) = db_at_token {
        let masked = mask_token(token);
        let campaigns_res = if let Some(cached) = state.accesstrade.peek_approved_campaigns().await {
            Ok(cached)
        } else {
            match tokio::time::timeout(
                Duration::from_millis(1200),
                state.accesstrade.get_approved_campaigns(token, false),
            )
            .await
            {
                Ok(res) => res,
                Err(_) => Err("Đang tải danh sách chiến dịch ACCESSTRADE ở nền...".to_string()),
            }
        };
        match campaigns_res {
            Ok(approved) => {
                let badge = format!(
                    "<span class=\"badge badge--ok\">🟢 Đã kết nối ({})</span>",
                    masked
                );
                let dyn_list = approved.dynamic_campaigns();
                let dyn_line = if dyn_list.is_empty() {
                    r#"<div class="inset-note">✨ <b>Tự động nhận diện đa sàn:</b> Sẵn sàng — đăng ký thêm <i>Shein, CellphoneS, FPT Shop, Con Cưng, Fado, Klook...</i> trên <code>pub2.accesstrade.vn</code> sẽ tự kích hoạt không cần sửa code.</div>"#.to_string()
                } else {
                    let labels: Vec<String> = dyn_list
                        .iter()
                        .take(8)
                        .map(|c| format!("{} (<code>{}</code>)", escape_html(&c.platform_group), escape_html(&c.domain)))
                        .collect();
                    format!(
                        "<div class=\"inset-note inset-note--ok\">✨ <b>Đã tự mở thêm {} sàn/hãng mở rộng:</b> {}</div>",
                        dyn_list.len(),
                        labels.join(", ")
                    )
                };
                let has_shopee_direct = !shopee_affiliate_id.trim().is_empty() || state.config.shopee_affiliate_id.is_some();
                let shopee_backup_note = if has_shopee_direct {
                    Some("Shopee Chính Chủ (100% Hoạt Động)")
                } else {
                    None
                };
                let rows = format!(
                    "<div class=\"plat-list\">{}{}{}{}{}</div>{}",
                    render_platform_status("🟠 Shopee", &approved.shopee, &camp_shopee, shopee_backup_note),
                    render_platform_status("🔵 Lazada", &approved.lazada, &camp_lazada, Some("Deeplink Template Sẵn Sàng")),
                    render_platform_status("🔹 Tiki", &approved.tiki, &camp_tiki, None),
                    render_platform_status("🎵 TikTok Shop", &approved.tiktok, &camp_tiktok, None),
                    render_platform_status("🌐 Alibaba", &approved.alibaba, "", None),
                    dyn_line,
                );
                approved_opt = Some(approved);
                (badge, rows)
            }
            Err(err) => {
                let badge = format!(
                    "<span class=\"badge badge--err\">🔴 Lỗi kết nối ({})</span>",
                    masked
                );
                let err_box = format!("<p class=\"err-text\">{}</p>", escape_html(&err));
                (badge, err_box)
            }
        }
    } else {
        (
            "<span class=\"badge badge--muted\">⚪ Chưa nhập API Key</span>".to_string(),
            "<p class=\"muted-text\">Vui lòng nhập <b>ACCESSTRADE API Access Key</b> bên dưới hoặc thiết lập <code>ACCESSTRADE_API_TOKEN</code> trong file <code>.env</code>.</p>".to_string(),
        )
    };

    // Kho mã toàn thị trường đang cache (tự động cập nhật mỗi 60 giây)
    let sync_meta = state.accesstrade.get_voucher_sync_meta().await;
    let last_sync_display = if sync_meta.last_sync_vn.is_empty() {
        "Đang khởi tạo chu kỳ đầu tiên...".to_string()
    } else {
        format!("{} (Chu kỳ #{})", sync_meta.last_sync_vn, sync_meta.sync_cycles)
    };

    let cached_vouchers = state.accesstrade.peek_cached_high_discount_vouchers().await;
    let total_market_vouchers = cached_vouchers.len();
    let high_pct_vouchers = cached_vouchers.iter().filter(|v| v.discount_percent >= 20).count();
    let shopee_vouchers = cached_vouchers.iter().filter(|v| v.platform == "Shopee").count();
    let tiki_vouchers = cached_vouchers.iter().filter(|v| v.platform == "Tiki").count();

    let search_q = query.q.as_deref().unwrap_or("").trim().to_string();
    let filter_plat = query.plat.as_deref().unwrap_or("all").trim().to_string();
    let search_q_lower = search_q.to_lowercase();

    let filtered_vouchers: Vec<_> = cached_vouchers
        .iter()
        .filter(|v| {
            let plat_ok = match filter_plat.as_str() {
                "pct20" => v.discount_percent >= 20,
                "Shopee" => v.platform == "Shopee",
                "Tiki" => v.platform == "Tiki",
                "other" => v.discount_percent < 20,
                _ => true,
            };
            if !plat_ok {
                return false;
            }
            if search_q_lower.is_empty() {
                return true;
            }
            v.shop_name.to_lowercase().contains(&search_q_lower)
                || v.coupon_code.to_lowercase().contains(&search_q_lower)
                || v.discount_desc.to_lowercase().contains(&search_q_lower)
                || v.platform.to_lowercase().contains(&search_q_lower)
        })
        .collect();

    let mut voucher_rows = String::new();
    for v in filtered_vouchers.iter().take(50) {
        let target_url = v.best_target_url(approved_opt.as_ref());
        voucher_rows.push_str(&format!(
            r#"<tr>
                <td><span class="badge badge--orange">{}</span></td>
                <td class="cell-strong">{}</td>
                <td><code class="code-pill">{}</code></td>
                <td><div class="cell-strong">{}</div><div class="cell-sub">{}</div></td>
                <td class="cell-muted">{}</td>
                <td><a href="{}" target="_blank" rel="noopener noreferrer" class="table-link">Mở Link Chuẩn ↗</a></td>
            </tr>"#,
            escape_html(&v.discount_badge()),
            escape_html(&v.platform),
            escape_html(&v.coupon_code),
            escape_html(&v.shop_name),
            escape_html(&v.discount_desc),
            escape_html(&v.end_time),
            escape_html(&target_url)
        ));
    }
    if voucher_rows.is_empty() {
        let empty_hint = if db_at_token.is_none() {
            r#"Chưa cấu hình ACCESSTRADE API Access Key. Vào tab <b>"Kết Nối Sàn &amp; API"</b> để nhập Token hoặc nạp mã vào mục <b>"Kho Mã Độc Quyền Hợp Tác"</b> bên dưới."#.to_string()
        } else if approved_opt.is_none() {
            r#"⚠️ <b>ACCESSTRADE API đang bị lỗi kết nối (HTTP 403 Forbidden / Sai Key):</b> Bot chưa được cấp quyền kéo mã tự động từ sàn về.<br><strong class="warn-hint">👉 Cách khắc phục:</strong> Vào <a href="https://pub2.accesstrade.vn" target="_blank" rel="noopener noreferrer" class="table-link">pub2.accesstrade.vn</a> &gt; <b>Công cụ &gt; API</b> lấy lại Access Key mới rồi dán vào tab <b>"Kết Nối Sàn &amp; API"</b>."#.to_string()
        } else {
            r#"Không có mã nào khớp bộ lọc (hoặc đang nạp chu kỳ đầu tiên). Bấm nút <b>Quét Sâu Toàn Bộ Ngay</b> nếu cần làm mới ngay."#.to_string()
        };
        voucher_rows = format!(r#"<tr><td colspan="6" class="empty-cell">{}</td></tr>"#, empty_hint);
    }

    // Bảng danh sách Mã Độc Quyền Hợp Tác & Deals trong DB
    let mut exclusive_rows = String::new();
    for d in &saved_deals {
        let kind_badge = if d.is_exclusive {
            r#"<span class="badge badge--purple">👑 Độc Quyền</span>"#
        } else {
            r#"<span class="badge badge--muted">Thường</span>"#
        };
        let status_badge = if d.is_active {
            r#"<span class="badge badge--ok">🟢 Đang bật</span>"#
        } else {
            r#"<span class="badge badge--muted">⚫ Đang tắt</span>"#
        };
        let code_cell_html = match d.coupon_code.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
            Some(c) if crate::services::social_harvester::is_synthetic_tracking_code(c) => format!(
                r#"<span class="badge badge--info" title="Ưu đãi áp trực tiếp khi mở link sàn, không phải mã gõ tay">Áp qua link · {}</span>"#,
                escape_html(c)
            ),
            Some(c) => format!(r#"<code class="code-pill">{}</code>"#, escape_html(c)),
            None => r#"<span class="badge badge--muted">Theo link</span>"#.to_string(),
        };
        let min_note = if d.min_spend > 0.0 {
            format!(" · Đơn từ {}", escape_html(&format_vnd(d.min_spend)))
        } else {
            String::new()
        };
        exclusive_rows.push_str(&format!(
            r#"<tr>
                <td>{}</td>
                <td class="cell-strong">{}</td>
                <td><div class="cell-strong">{}</div><div class="cell-sub">{}{}</div></td>
                <td>{}</td>
                <td>{}</td>
                <td>
                    <div class="row-actions">
                        <form method="POST" action="/admin/deals/toggle">
                            <input type="hidden" name="deal_id" value="{}">
                            <button type="submit" class="btn btn--secondary btn--sm">Bật/Tắt</button>
                        </form>
                        <form method="POST" action="/admin/deals/delete">
                            <input type="hidden" name="deal_id" value="{}">
                            <button type="submit" class="btn btn--danger btn--sm">Xoá</button>
                        </form>
                    </div>
                </td>
            </tr>"#,
            kind_badge,
            escape_html(&d.platform),
            escape_html(&d.title),
            escape_html(&d.discount_desc),
            min_note,
            code_cell_html,
            status_badge,
            d.id,
            d.id
        ));
    }
    if exclusive_rows.is_empty() {
        exclusive_rows = r#"<tr><td colspan="6" class="empty-cell">Chưa có Mã Độc Quyền Hợp Tác nào. Hãy dán danh sách mã từ AM ACCESSTRADE / MCN / Nhãn hàng vào ô bên trên.</td></tr>"#.to_string();
    }

    // Render danh sách clicks gần đây (chuẩn giờ VN + phân biệt link đã gắn tracking vs link sạch)
    let mut clicks_rows = String::new();
    for c in &clicks {
        let plat_badge = match c.platform.as_str() {
            "Shopee" => r#"<span class="badge badge--orange">Shopee</span>"#,
            "TikTok Shop" | "TikTok" => r#"<span class="badge badge--pink">TikTok Shop</span>"#,
            "Lazada" => r#"<span class="badge badge--info">Lazada</span>"#,
            "Tiki" => r#"<span class="badge badge--cyan">Tiki</span>"#,
            _ => r#"<span class="badge badge--muted">Khác</span>"#,
        };
        let is_tracked = c.affiliate_url != c.clean_url
            || c.affiliate_url.contains("go.isclix.com")
            || c.affiliate_url.contains("s.shopee.vn")
            || c.affiliate_url.contains("tiktok.com/t/");
        let out_link_label = if is_tracked {
            "🔗 Link Đã Gắn Tracking ↗"
        } else {
            "🌐 Link Sạch (Chưa bật Aff) ↗"
        };
        let vn_time = format_timestamp_vn(&c.created_at);
        clicks_rows.push_str(&format!(
            r#"<tr>
                <td class="font-mono cell-muted">{}</td>
                <td>{}</td>
                <td><div class="truncate-url" title="{}">{}</div></td>
                <td><a href="{}" target="_blank" rel="noopener noreferrer" class="table-link table-link--amber">{}</a></td>
                <td class="font-mono cell-muted">{}</td>
            </tr>"#,
            c.user_id,
            plat_badge,
            escape_html(&c.clean_url),
            escape_html(&c.clean_url),
            escape_html(&c.affiliate_url),
            out_link_label,
            escape_html(&vn_time)
        ));
    }

    if clicks_rows.is_empty() {
        clicks_rows = r#"<tr><td colspan="5" class="empty-cell">Chưa có lượt dán link nào từ khách hàng. Dán link sản phẩm vào Bot Telegram @vvipass_vip_bot để kiểm tra.</td></tr>"#.to_string();
    }

    let bot_mode_badge = if state.webhook_tx.is_some() {
        r#"<span class="mode-tag mode-tag--ok">🟢 Webhook 24/7</span>"#
    } else {
        r#"<span class="mode-tag mode-tag--info">🔵 Polling PC</span>"#
    };

    let sel_all = if filter_plat == "all" { "selected" } else { "" };
    let sel_pct20 = if filter_plat == "pct20" { "selected" } else { "" };
    let sel_other = if filter_plat == "other" { "selected" } else { "" };
    let sel_shopee = if filter_plat == "Shopee" { "selected" } else { "" };
    let sel_tiki = if filter_plat == "Tiki" { "selected" } else { "" };
    let shown_vouchers_count = filtered_vouchers.len().min(50);
    let matched_vouchers_count = filtered_vouchers.len();
    let other_vouchers_count = total_market_vouchers.saturating_sub(high_pct_vouchers);

    let charts_html = render_dashboard_charts_html(
        &stats,
        total_market_vouchers,
        high_pct_vouchers,
        shopee_vouchers,
        tiki_vouchers,
        saved_deals.len(),
    );

    let dashboard_html = format!(
        r##"<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Thế Giới Siêu Sale - Trung Tâm Quản Trị Hệ Thống</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Be+Vietnam+Pro:wght@400;500;600;700;800&family=JetBrains+Mono:wght@400;500;600&display=swap" rel="stylesheet">
    <script>
        (function () {{
            try {{
                if (localStorage.getItem('dealx-theme') === 'dark') {{
                    document.documentElement.setAttribute('data-theme', 'dark');
                }}
            }} catch (e) {{}}
        }})();
    </script>
    <style>
        :root {{
            color-scheme: light;
            --bg-base: #F4F6F9;
            --bg-sidebar: #FFFFFF;
            --bg-topbar: rgba(255, 255, 255, 0.94);
            --bg-surface: #FFFFFF;
            --bg-elevated: #F8FAFC;
            --bg-inset: #F1F5F9;
            --bg-input: #FFFFFF;
            --bg-table-head: #F8FAFC;
            --bg-table-hover: #F1F5F9;
            --border-subtle: #E2E8F0;
            --border-strong: #CBD5E1;
            --text-primary: #0F172A;
            --text-secondary: #334155;
            --text-muted: #475569;
            --text-placeholder: #94A3B8;
            --brand-primary: #C2410C;
            --brand-hover: #9A3412;
            --brand-bright: #EA580C;
            --brand-accent: #C2410C;
            --brand-contrast: #FFFFFF;
            --brand-soft-bg: #FFF7ED;
            --brand-soft-border: #FED7AA;
            --brand-soft-text: #C2410C;
            --brand-ring: rgba(194, 65, 12, 0.22);
            --link-text: #0369A1;
            --ok-text: #15803D;
            --ok-bg: #DCFCE7;
            --ok-border: #86EFAC;
            --warn-text: #92400E;
            --warn-bg: #FEF3C7;
            --warn-border: #FCD34D;
            --err-text: #B91C1C;
            --err-bg: #FEE2E2;
            --err-border: #FCA5A5;
            --info-text: #0369A1;
            --info-bg: #E0F2FE;
            --info-border: #7DD3FC;
            --purple-text: #6D28D9;
            --purple-bg: #EDE9FE;
            --purple-border: #C4B5FD;
            --pink-text: #BE185D;
            --pink-bg: #FCE7F3;
            --pink-border: #F9A8D4;
            --code-bg: #F1F5F9;
            --code-text: #0F172A;
            --code-border: #E2E8F0;
            --radius-sm: 0.5rem;
            --radius-md: 0.75rem;
            --radius-lg: 1rem;
            --shadow-card: 0 1px 2px rgba(15, 23, 42, 0.05), 0 4px 16px rgba(15, 23, 42, 0.04);
        }}
        html[data-theme="dark"] {{
            color-scheme: dark;
            --bg-base: #0B1120;
            --bg-sidebar: #0F172A;
            --bg-topbar: rgba(15, 23, 42, 0.92);
            --bg-surface: #131C31;
            --bg-elevated: #18233C;
            --bg-inset: #0D1526;
            --bg-input: #18233C;
            --bg-table-head: #0F172A;
            --bg-table-hover: rgba(51, 65, 85, 0.35);
            --border-subtle: rgba(148, 163, 184, 0.16);
            --border-strong: rgba(148, 163, 184, 0.32);
            --text-primary: #F8FAFC;
            --text-secondary: #E2E8F0;
            --text-muted: #94A3B8;
            --text-placeholder: #64748B;
            --brand-primary: #F97316;
            --brand-hover: #EA580C;
            --brand-bright: #FB923C;
            --brand-accent: #FB923C;
            --brand-contrast: #090D16;
            --brand-soft-bg: rgba(249, 115, 22, 0.16);
            --brand-soft-border: rgba(249, 115, 22, 0.38);
            --brand-soft-text: #FDBA74;
            --brand-ring: rgba(249, 115, 22, 0.28);
            --link-text: #7DD3FC;
            --ok-text: #86EFAC;
            --ok-bg: rgba(34, 197, 94, 0.16);
            --ok-border: rgba(34, 197, 94, 0.38);
            --warn-text: #FDE68A;
            --warn-bg: rgba(245, 158, 11, 0.16);
            --warn-border: rgba(245, 158, 11, 0.38);
            --err-text: #FCA5A5;
            --err-bg: rgba(239, 68, 68, 0.16);
            --err-border: rgba(239, 68, 68, 0.38);
            --info-text: #7DD3FC;
            --info-bg: rgba(14, 165, 233, 0.16);
            --info-border: rgba(14, 165, 233, 0.38);
            --purple-text: #D8B4FE;
            --purple-bg: rgba(168, 85, 247, 0.16);
            --purple-border: rgba(168, 85, 247, 0.38);
            --pink-text: #F9A8D4;
            --pink-bg: rgba(236, 72, 153, 0.16);
            --pink-border: rgba(236, 72, 153, 0.38);
            --code-bg: #1E293B;
            --code-text: #FDE68A;
            --code-border: rgba(148, 163, 184, 0.22);
            --shadow-card: 0 8px 24px -8px rgba(0, 0, 0, 0.45);
        }}

        *, *::before, *::after {{ box-sizing: border-box; }}
        html, body {{ margin: 0; padding: 0; overflow-x: clip; }}
        body {{
            min-height: 100vh;
            background-color: var(--bg-base);
            color: var(--text-primary);
            font-family: 'Be Vietnam Pro', "Segoe UI", system-ui, -apple-system, sans-serif;
            font-size: 14px;
            line-height: 1.5;
            -webkit-font-smoothing: antialiased;
        }}
        a {{ color: inherit; text-decoration: none; }}
        button, input, select, textarea {{ font-family: inherit; }}
        button:focus-visible, a:focus-visible, input:focus-visible, select:focus-visible, textarea:focus-visible, summary:focus-visible {{
            outline: 2px solid var(--brand-primary);
            outline-offset: 2px;
        }}
        code {{
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.78rem;
            background: var(--code-bg);
            color: var(--code-text);
            border: 1px solid var(--code-border);
            padding: 0.1rem 0.4rem;
            border-radius: 0.35rem;
        }}

        /* Desktop App Shell Layout */
        .app-shell {{
            display: grid;
            grid-template-columns: 260px minmax(0, 1fr);
            min-height: 100vh;
        }}

        /* Sidebar Navigation */
        .sidebar {{
            position: sticky;
            top: 0;
            height: 100vh;
            background: var(--bg-sidebar);
            border-right: 1px solid var(--border-subtle);
            display: flex;
            flex-direction: column;
            padding: 1.25rem 1rem;
            gap: 1.25rem;
            overflow-y: auto;
            z-index: 45;
        }}
        .brand {{
            display: flex;
            align-items: center;
            gap: 0.75rem;
            padding: 0.25rem 0.25rem 1rem;
            border-bottom: 1px solid var(--border-subtle);
        }}
        .brand__logo-wrap {{
            position: relative;
            width: 42px;
            height: 42px;
            flex-shrink: 0;
        }}
        .brand__logo {{
            width: 42px;
            height: 42px;
            border-radius: 9999px;
            object-fit: cover;
            display: block;
            border: 2px solid var(--brand-bright);
        }}
        .brand__dot {{
            position: absolute;
            bottom: 0;
            right: 0;
            width: 11px;
            height: 11px;
            border-radius: 9999px;
            background: #22C55E;
            border: 2px solid var(--bg-sidebar);
        }}
        .brand__title-row {{
            display: flex;
            align-items: center;
            gap: 0.4rem;
            flex-wrap: wrap;
        }}
        .brand__title {{
            margin: 0;
            font-size: 0.92rem;
            font-weight: 800;
            letter-spacing: -0.02em;
            color: var(--text-primary);
        }}
        .brand__tag {{
            padding: 0.1rem 0.42rem;
            border-radius: 0.35rem;
            font-size: 0.62rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            color: var(--brand-soft-text);
            background: var(--brand-soft-bg);
            border: 1px solid var(--brand-soft-border);
        }}
        .brand__sub {{
            margin: 0.18rem 0 0;
            font-size: 0.72rem;
            color: var(--text-muted);
            line-height: 1.35;
        }}
        .sidebar__section-label {{
            font-size: 0.68rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.08em;
            color: var(--text-muted);
            padding: 0 0.65rem;
            margin-bottom: 0.4rem;
        }}
        .tab-nav {{
            display: flex;
            flex-direction: column;
            gap: 0.35rem;
        }}
        .tab-btn {{
            display: flex;
            align-items: center;
            gap: 0.7rem;
            width: 100%;
            min-height: 44px;
            padding: 0.62rem 0.85rem;
            border-radius: var(--radius-md);
            font-size: 0.86rem;
            font-weight: 600;
            color: var(--text-secondary);
            background: transparent;
            border: 1px solid transparent;
            cursor: pointer;
            text-align: left;
            white-space: nowrap;
            transition: background-color 0.15s ease, color 0.15s ease, border-color 0.15s ease;
        }}
        .tab-btn__icon {{
            width: 28px;
            height: 28px;
            border-radius: 0.5rem;
            display: inline-flex;
            align-items: center;
            justify-content: center;
            background: var(--bg-inset);
            font-size: 0.92rem;
            flex-shrink: 0;
        }}
        .tab-btn:hover {{
            color: var(--text-primary);
            background: var(--bg-elevated);
            border-color: var(--border-subtle);
        }}
        .tab-btn.active {{
            color: var(--brand-soft-text);
            background: var(--brand-soft-bg);
            border-color: var(--brand-soft-border);
            font-weight: 700;
            box-shadow: inset 3px 0 0 var(--brand-bright);
        }}
        .tab-btn.active .tab-btn__icon {{
            background: var(--brand-primary);
            color: var(--brand-contrast);
        }}
        .sidebar__footer {{
            margin-top: auto;
            display: flex;
            flex-direction: column;
            gap: 0.75rem;
            padding-top: 1rem;
            border-top: 1px solid var(--border-subtle);
        }}
        .bot-status-card {{
            background: var(--bg-elevated);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 0.75rem 0.85rem;
            display: flex;
            flex-direction: column;
            gap: 0.45rem;
            font-size: 0.78rem;
        }}
        .bot-status-card__row {{
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 0.4rem;
        }}
        .bot-link {{
            color: var(--link-text);
            font-weight: 700;
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.76rem;
        }}
        .bot-link:hover {{ text-decoration: underline; }}
        .pulse-dot {{
            width: 8px;
            height: 8px;
            border-radius: 9999px;
            background: #22C55E;
            display: inline-block;
            flex-shrink: 0;
            box-shadow: 0 0 0 3px rgba(34, 197, 94, 0.22);
        }}
        .mode-tag {{ font-weight: 700; font-size: 0.74rem; }}
        .mode-tag--ok {{ color: var(--ok-text); }}
        .mode-tag--info {{ color: var(--info-text); }}

        /* Main Shell & Single Slim Topbar */
        .shell-main {{
            display: flex;
            flex-direction: column;
            min-width: 0;
        }}
        .topbar {{
            position: sticky;
            top: 0;
            z-index: 40;
            background: var(--bg-topbar);
            backdrop-filter: blur(14px);
            -webkit-backdrop-filter: blur(14px);
            border-bottom: 1px solid var(--border-subtle);
            padding: 0.85rem 1.75rem;
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 1rem;
            flex-wrap: wrap;
        }}
        .topbar__left {{
            min-width: 0;
        }}
        .topbar__title {{
            margin: 0;
            font-size: 1.15rem;
            font-weight: 800;
            color: var(--text-primary);
            letter-spacing: -0.02em;
        }}
        .topbar__sub {{
            margin: 0.12rem 0 0;
            font-size: 0.78rem;
            color: var(--text-muted);
        }}
        .topbar__actions {{
            display: flex;
            align-items: center;
            gap: 0.6rem;
            flex-wrap: wrap;
        }}
        .admin-pill {{
            display: inline-flex;
            align-items: center;
            gap: 0.45rem;
            min-height: 36px;
            padding: 0.35rem 0.75rem;
            background: var(--bg-elevated);
            border: 1px solid var(--border-subtle);
            border-radius: 9999px;
            font-size: 0.78rem;
            color: var(--text-muted);
        }}
        .admin-pill strong {{ color: var(--text-primary); }}
        .theme-btn {{
            display: inline-flex;
            align-items: center;
            gap: 0.38rem;
            min-height: 36px;
            padding: 0.38rem 0.85rem;
            border-radius: 9999px;
            background: var(--bg-elevated);
            border: 1px solid var(--border-strong);
            color: var(--text-primary);
            font-size: 0.78rem;
            font-weight: 700;
            cursor: pointer;
            transition: border-color 0.15s ease, background-color 0.15s ease;
        }}
        .theme-btn:hover {{
            border-color: var(--brand-primary);
            background: var(--bg-inset);
        }}

        /* Main Workspace & Footer */
        .admin-main {{
            width: 100%;
            max-width: 1380px;
            margin: 0 auto;
            padding: 1.5rem 1.75rem 2.5rem;
            flex: 1;
            display: flex;
            flex-direction: column;
            gap: 1.25rem;
        }}
        .admin-footer {{
            width: 100%;
            max-width: 1380px;
            margin: 0 auto;
            padding: 1.15rem 1.75rem 1.75rem;
            text-align: center;
            font-size: 0.78rem;
            color: var(--text-muted);
            border-top: 1px solid var(--border-subtle);
        }}
        .tab-pane {{
            display: flex;
            flex-direction: column;
            gap: 1.35rem;
        }}
        .tab-pane.is-hidden {{ display: none !important; }}

        /* Notice Banners */
        .notice-banner {{
            display: flex;
            align-items: flex-start;
            gap: 0.6rem;
            padding: 0.72rem 1rem;
            border-radius: var(--radius-md);
            font-size: 0.84rem;
            font-weight: 500;
            line-height: 1.5;
            border: 1px solid var(--border-subtle);
        }}
        .notice-banner--ok {{
            background: var(--bg-surface);
            border-color: var(--border-subtle);
            border-left: 4px solid var(--ok-text);
            color: var(--text-secondary);
        }}
        .notice-banner--warn {{
            background: var(--warn-bg);
            border-color: var(--warn-border);
            border-left: 4px solid var(--warn-text);
            color: var(--warn-text);
        }}
        .notice-banner--info {{
            background: var(--info-bg);
            border-color: var(--info-border);
            border-left: 4px solid var(--info-text);
            color: var(--info-text);
        }}

        /* Cards */
        .card {{
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-lg);
            padding: 1.4rem 1.5rem;
            box-shadow: var(--shadow-card);
            display: flex;
            flex-direction: column;
            gap: 1.15rem;
        }}
        .card--table {{
            padding: 0;
            gap: 0;
            overflow: hidden;
        }}
        .card--orange {{ border-top: 3px solid var(--brand-bright); }}
        .card--green {{ border-top: 3px solid var(--ok-text); }}
        .card--purple {{ border-top: 3px solid var(--purple-text); }}
        .card--social {{
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-left: 4px solid var(--brand-bright);
            flex-direction: row;
            flex-wrap: wrap;
            align-items: center;
            justify-content: space-between;
            gap: 1rem;
        }}
        .social-left {{
            display: flex;
            align-items: center;
            gap: 0.95rem;
            flex: 1 1 380px;
        }}
        .social-icon {{
            width: 46px;
            height: 46px;
            border-radius: 0.85rem;
            background: var(--brand-soft-bg);
            border: 1px solid var(--brand-soft-border);
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 1.35rem;
            flex-shrink: 0;
        }}
        .social-meta {{
            display: flex;
            flex-direction: column;
            align-items: flex-end;
            gap: 0.35rem;
            text-align: right;
            min-width: 225px;
        }}
        .meta-time {{
            color: var(--text-primary);
            font-family: 'JetBrains Mono', monospace;
            font-weight: 600;
        }}
        .card-head {{
            display: flex;
            flex-wrap: wrap;
            align-items: flex-start;
            justify-content: space-between;
            gap: 0.75rem;
            padding-bottom: 0.95rem;
            border-bottom: 1px solid var(--border-subtle);
        }}
        .card-head--table {{
            padding: 1.15rem 1.5rem;
            margin: 0;
            background: var(--bg-elevated);
        }}
        .card-head-actions {{
            display: flex;
            align-items: center;
            gap: 0.55rem;
            flex-wrap: wrap;
        }}
        .card-title {{
            margin: 0;
            font-size: 1.02rem;
            font-weight: 700;
            color: var(--text-primary);
            display: flex;
            flex-wrap: wrap;
            align-items: center;
            gap: 0.5rem;
        }}
        .card-desc {{
            margin: 0.32rem 0 0;
            font-size: 0.82rem;
            color: var(--text-muted);
            line-height: 1.55;
        }}

        /* KPI Grid */
        .kpi-grid {{
            display: grid;
            grid-template-columns: repeat(4, minmax(0, 1fr));
            gap: 1rem;
        }}
        .kpi-card {{
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-lg);
            padding: 1.2rem 1.25rem;
            display: flex;
            flex-direction: column;
            justify-content: space-between;
            gap: 0.9rem;
            box-shadow: var(--shadow-card);
            transition: border-color 0.15s ease;
        }}
        .kpi-card:hover {{ border-color: var(--border-strong); }}
        .kpi-card--orange {{ border-top: 3px solid var(--brand-bright); }}
        .kpi-card--green {{ border-top: 3px solid var(--ok-text); }}
        .kpi-card__top {{
            display: flex;
            align-items: center;
            justify-content: space-between;
            font-size: 0.73rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            color: var(--text-muted);
        }}
        .kpi-card__icon {{
            width: 2rem;
            height: 2rem;
            border-radius: 0.55rem;
            display: inline-flex;
            align-items: center;
            justify-content: center;
            font-size: 0.95rem;
            background: var(--bg-inset);
            border: 1px solid var(--border-subtle);
        }}
        .kpi-card__val {{
            margin-top: 0.45rem;
            font-size: 1.95rem;
            font-weight: 800;
            line-height: 1.15;
            letter-spacing: -0.02em;
            font-variant-numeric: tabular-nums;
            color: var(--text-primary);
        }}
        .kpi-card__val--amber {{ color: var(--brand-primary); }}
        .kpi-card__val--orange {{ color: var(--brand-primary); }}
        .kpi-card__val--green {{ color: var(--ok-text); font-size: 1rem; font-weight: 700; margin-top: 0.5rem; }}
        .kpi-card__meta {{
            background: var(--bg-inset);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 0.58rem 0.75rem;
            font-size: 0.76rem;
            color: var(--text-secondary);
            display: flex;
            flex-direction: column;
            gap: 0.32rem;
        }}
        .kpi-card__pill {{
            background: var(--bg-inset);
            border: 1px solid var(--border-subtle);
            color: var(--text-secondary);
            border-radius: var(--radius-sm);
            padding: 0.48rem 0.72rem;
            font-size: 0.76rem;
            font-weight: 600;
        }}
        .refresh-chip {{
            background: var(--info-bg);
            border: 1px solid var(--info-border);
            color: var(--info-text);
            border-radius: var(--radius-sm);
            padding: 0.48rem 0.72rem;
            font-size: 0.76rem;
            font-weight: 700;
            text-align: center;
        }}
        .kpi-row {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            gap: 0.5rem;
            flex-wrap: wrap;
        }}
        .kpi-row strong {{ font-family: 'JetBrains Mono', monospace; color: var(--text-primary); }}

        /* Responsive Layout Grids */
        .split-at {{
            display: grid;
            grid-template-columns: minmax(340px, 1.08fr) minmax(0, 1.42fr);
            gap: 1.5rem;
            align-items: start;
        }}
        .grid-2 {{
            display: grid;
            grid-template-columns: repeat(2, minmax(0, 1fr));
            gap: 1.25rem;
        }}
        .grid-3 {{
            display: grid;
            grid-template-columns: repeat(3, minmax(0, 1fr));
            gap: 1rem;
        }}
        .form-grid-2 {{
            display: grid;
            grid-template-columns: repeat(2, minmax(0, 1fr));
            gap: 0.85rem;
        }}

        /* ACCESSTRADE Platform Status List */
        .plat-panel {{
            background: var(--bg-elevated);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 1rem;
            display: flex;
            flex-direction: column;
            gap: 0.85rem;
        }}
        .plat-panel__title {{
            font-size: 0.73rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.06em;
            color: var(--text-muted);
        }}
        .plat-list {{
            display: flex;
            flex-direction: column;
            gap: 0.55rem;
        }}
        .plat-row {{
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 0.75rem;
            padding: 0.65rem 0.85rem;
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
        }}
        .plat-row__left {{
            display: flex;
            flex-direction: column;
            gap: 0.18rem;
            min-width: 0;
        }}
        .plat-row__name {{
            font-size: 0.85rem;
            font-weight: 600;
            color: var(--text-primary);
        }}
        .plat-row__sub {{
            font-size: 0.74rem;
            color: var(--text-muted);
            font-family: 'JetBrains Mono', monospace;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
        }}
        .inset-note {{
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-sm);
            padding: 0.78rem 0.9rem;
            font-size: 0.78rem;
            color: var(--text-muted);
            line-height: 1.55;
            display: flex;
            flex-direction: column;
            gap: 0.3rem;
        }}
        .inset-note strong, .inset-note b {{ color: var(--text-primary); }}
        .inset-note--ok {{
            border-color: var(--ok-border);
            background: var(--ok-bg);
            color: var(--ok-text);
        }}
        .err-text {{ color: var(--err-text); font-size: 0.82rem; font-weight: 600; }}
        .muted-text {{ color: var(--text-muted); font-size: 0.78rem; margin-top: 0.3rem; }}
        .ok-strong {{ color: var(--ok-text); }}
        .primary-strong {{ color: var(--text-primary); }}
        .label-accent {{ color: var(--brand-soft-text); font-weight: 700; }}
        .label-info {{ color: var(--info-text); font-weight: 700; }}
        .warn-hint {{ color: var(--warn-text); }}
        .comm-val {{ color: var(--ok-text); font-weight: 700; }}
        .font-mono {{ font-family: 'JetBrains Mono', monospace; }}
        .inline-form {{ margin: 0; }}

        /* Badges & Pills */
        .badge {{
            display: inline-flex;
            align-items: center;
            gap: 0.3rem;
            padding: 0.28rem 0.65rem;
            border-radius: 9999px;
            font-size: 0.74rem;
            font-weight: 600;
            border: 1px solid transparent;
            white-space: nowrap;
            line-height: 1.25;
            flex-shrink: 0;
        }}
        .badge--ok {{ background: var(--ok-bg); border-color: var(--ok-border); color: var(--ok-text); }}
        .badge--err {{ background: var(--err-bg); border-color: var(--err-border); color: var(--err-text); }}
        .badge--warn {{ background: var(--warn-bg); border-color: var(--warn-border); color: var(--warn-text); }}
        .badge--info {{ background: var(--info-bg); border-color: var(--info-border); color: var(--info-text); }}
        .badge--muted {{
            background: var(--bg-inset);
            border-color: var(--border-strong);
            color: var(--text-secondary);
        }}
        .badge--orange {{
            background: var(--brand-soft-bg);
            border-color: var(--brand-soft-border);
            color: var(--brand-soft-text);
        }}
        .badge--purple {{ background: var(--purple-bg); border-color: var(--purple-border); color: var(--purple-text); }}
        .badge--pink {{ background: var(--pink-bg); border-color: var(--pink-border); color: var(--pink-text); }}
        .badge--cyan {{
            background: var(--info-bg);
            border-color: var(--info-border);
            color: var(--info-text);
        }}
        .code-pill {{
            display: inline-block;
            padding: 0.22rem 0.55rem;
            border-radius: 0.45rem;
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.76rem;
            font-weight: 600;
            background: var(--code-bg);
            color: var(--code-text);
            border: 1px solid var(--code-border);
        }}

        /* Forms & Controls */
        .form-stack {{ display: flex; flex-direction: column; gap: 1rem; }}
        .subpanel {{
            background: var(--bg-elevated);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 1.15rem;
            display: flex;
            flex-direction: column;
            gap: 0.85rem;
        }}
        .subpanel__title {{
            font-size: 0.76rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            color: var(--text-secondary);
        }}
        .subpanel__title--purple {{ color: var(--purple-text); }}
        .divider-top {{ border-top: 1px solid var(--border-subtle); padding-top: 0.95rem; }}
        .field-label {{
            display: block;
            font-size: 0.78rem;
            font-weight: 600;
            color: var(--text-secondary);
            margin-bottom: 0.35rem;
        }}
        input[type="text"],
        input[type="password"],
        input[type="url"],
        input[type="number"],
        select,
        textarea,
        .field-input,
        .code-textarea {{
            width: 100%;
            min-height: 42px;
            background: var(--bg-input);
            border: 1px solid var(--border-strong);
            border-radius: var(--radius-md);
            color: var(--text-primary);
            padding: 0.58rem 0.85rem;
            font-size: 0.84rem;
            transition: border-color 0.15s ease, box-shadow 0.15s ease;
        }}
        textarea.code-textarea {{ min-height: 110px; }}
        .field-input--mono, .code-textarea {{
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.8rem;
        }}
        input::placeholder, textarea::placeholder {{ color: var(--text-placeholder); }}
        input:focus, select:focus, textarea:focus {{
            outline: none;
            border-color: var(--brand-primary);
            box-shadow: 0 0 0 3px var(--brand-ring);
        }}
        .check-row {{
            display: inline-flex;
            align-items: center;
            gap: 0.55rem;
            font-size: 0.82rem;
            color: var(--text-secondary);
            cursor: pointer;
        }}
        .check-row--mt {{ margin-top: 0.45rem; }}
        .check-row input[type="checkbox"] {{
            width: 1.05rem;
            height: 1.05rem;
            accent-color: var(--brand-primary);
            cursor: pointer;
        }}
        .filter-bar {{
            display: flex;
            flex-wrap: wrap;
            align-items: center;
            gap: 0.65rem;
            background: var(--bg-elevated);
            border: 1px solid var(--border-subtle);
            padding: 0.78rem 0.95rem;
            border-radius: var(--radius-md);
        }}
        .filter-bar select {{
            width: auto;
            min-width: 195px;
        }}
        .inline-form-grow {{
            display: flex;
            flex: 1 1 280px;
            align-items: flex-end;
            gap: 0.65rem;
            flex-wrap: wrap;
        }}
        .grow-field {{
            flex: 1 1 220px;
            min-width: 180px;
            width: auto !important;
        }}

        /* Buttons */
        .btn {{
            display: inline-flex;
            align-items: center;
            justify-content: center;
            gap: 0.42rem;
            min-height: 40px;
            padding: 0.55rem 1.05rem;
            border-radius: var(--radius-md);
            font-size: 0.82rem;
            font-weight: 700;
            border: 1px solid transparent;
            cursor: pointer;
            text-decoration: none;
            white-space: nowrap;
            transition: background-color 0.15s ease, border-color 0.15s ease, color 0.15s ease;
        }}
        .btn--primary {{
            background: var(--brand-primary);
            color: var(--brand-contrast);
            border-color: var(--brand-primary);
        }}
        .btn--primary:hover {{
            background: var(--brand-hover);
            border-color: var(--brand-hover);
        }}
        .btn--secondary {{
            background: var(--bg-surface);
            color: var(--text-primary);
            border-color: var(--border-strong);
        }}
        .btn--secondary:hover {{
            background: var(--bg-inset);
            border-color: var(--brand-primary);
        }}
        .btn--success {{
            background: var(--ok-text);
            color: #FFFFFF;
            border-color: var(--ok-text);
        }}
        .btn--success:hover {{ opacity: 0.92; }}
        .btn--purple {{
            background: var(--purple-text);
            color: #FFFFFF;
            border-color: var(--purple-text);
        }}
        .btn--purple:hover {{ opacity: 0.92; }}
        html[data-theme="dark"] .btn--success,
        html[data-theme="dark"] .btn--purple {{ color: #090D16; }}
        .btn--danger {{
            background: var(--err-bg);
            color: var(--err-text);
            border-color: var(--err-border);
        }}
        .btn--danger:hover {{ opacity: 0.9; }}
        .btn--orange-soft {{
            background: var(--brand-soft-bg);
            color: var(--brand-soft-text);
            border-color: var(--brand-soft-border);
        }}
        .btn--orange-soft:hover {{ border-color: var(--brand-primary); }}
        .btn--sm {{ min-height: 34px; padding: 0.35rem 0.78rem; font-size: 0.76rem; border-radius: var(--radius-sm); }}
        .btn--block {{ width: 100%; }}
        .row-actions {{ display: flex; align-items: center; gap: 0.45rem; flex-wrap: nowrap; }}

        /* Tables */
        .table-wrap {{
            overflow-x: auto;
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            background: var(--bg-surface);
        }}
        .card--table .table-wrap {{
            border: none;
            border-radius: 0;
        }}
        .data-table {{
            width: 100%;
            border-collapse: collapse;
            text-align: left;
            font-size: 0.82rem;
        }}
        .data-table thead {{
            background: var(--bg-table-head);
            border-bottom: 1px solid var(--border-subtle);
        }}
        .data-table th {{
            padding: 0.82rem 1rem;
            font-size: 0.72rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            color: var(--text-muted);
            white-space: nowrap;
        }}
        .data-table td {{
            padding: 0.82rem 1rem;
            border-bottom: 1px solid var(--border-subtle);
            vertical-align: middle;
        }}
        .data-table tbody tr:hover {{ background: var(--bg-table-hover); }}
        .data-table tbody tr:last-child td {{ border-bottom: none; }}
        .cell-strong {{ font-weight: 600; color: var(--text-primary); }}
        .cell-sub {{ font-size: 0.76rem; color: var(--text-muted); margin-top: 0.22rem; }}
        .cell-muted {{ color: var(--text-muted); font-size: 0.78rem; }}
        .table-link {{ color: var(--link-text); font-weight: 600; }}
        .table-link:hover {{ text-decoration: underline; }}
        .table-link--amber {{ color: var(--brand-primary); font-weight: 700; }}
        .truncate-url {{
            max-width: 260px;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
        }}
        .empty-cell {{
            padding: 2.25rem 1rem !important;
            text-align: center;
            color: var(--text-muted);
            font-size: 0.84rem;
        }}

        /* Cloudflare Details */
        .cf-details {{
            background: var(--bg-elevated);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 0.78rem 1rem;
        }}
        .cf-summary {{
            cursor: pointer;
            font-size: 0.82rem;
            font-weight: 700;
            color: var(--brand-soft-text);
        }}
        .cf-body {{
            margin-top: 0.78rem;
            font-size: 0.8rem;
            color: var(--text-secondary);
            line-height: 1.55;
            display: flex;
            flex-direction: column;
            gap: 0.5rem;
        }}
        .cf-steps {{
            margin: 0.25rem 0 0 1.25rem;
            padding: 0;
            display: flex;
            flex-direction: column;
            gap: 0.35rem;
        }}

        /* SVG Icons, Signal Meter & Charts */
        .ico {{ width: 18px; height: 18px; display: inline-block; vertical-align: middle; flex-shrink: 0; }}
        .ico--sm {{ width: 16px; height: 16px; }}
        .ico--xs {{ width: 14px; height: 14px; }}
        .kpi-card__icon--blue {{ background: var(--info-bg); color: var(--info-text); border-color: var(--info-border); }}
        .kpi-card__icon--amber {{ background: var(--warn-bg); color: var(--warn-text); border-color: var(--warn-border); }}
        .kpi-card__icon--orange {{ background: var(--brand-soft-bg); color: var(--brand-soft-text); border-color: var(--brand-soft-border); }}
        .kpi-card__icon--green {{ background: var(--ok-bg); color: var(--ok-text); border-color: var(--ok-border); }}
        .social-icon {{ color: var(--brand-primary); }}
        .signal-meter {{
            display: inline-flex; align-items: center; gap: 0.4rem;
            padding: 0.28rem 0.65rem; border-radius: 9999px;
            background: var(--bg-elevated); border: 1px solid var(--border-subtle);
            font-size: 0.74rem; font-weight: 700; color: var(--text-secondary);
        }}

        .charts-grid {{
            display: grid;
            grid-template-columns: repeat(2, minmax(0, 1fr));
            gap: 1.15rem;
        }}
        .chart-card {{ gap: 1rem; transition: border-color 0.16s ease, box-shadow 0.16s ease; }}
        .chart-card:hover {{ border-color: var(--border-strong); }}
        .chart-head {{
            display: flex; align-items: flex-start; justify-content: space-between;
            gap: 0.75rem; flex-wrap: wrap;
            padding-bottom: 0.85rem; border-bottom: 1px solid var(--border-subtle);
        }}
        .chart-head__title {{ display: flex; align-items: center; gap: 0.72rem; }}
        .chart-head__title h3 {{ margin: 0; font-size: 0.96rem; font-weight: 700; color: var(--text-primary); }}
        .chart-head__title p {{ margin: 0.15rem 0 0; font-size: 0.76rem; color: var(--text-muted); }}
        .chart-ico {{
            width: 38px; height: 38px; border-radius: 0.68rem;
            display: inline-flex; align-items: center; justify-content: center; flex-shrink: 0;
            border: 1px solid var(--border-subtle);
        }}
        .chart-ico--brand {{ background: var(--brand-soft-bg); color: var(--brand-soft-text); border-color: var(--brand-soft-border); }}
        .chart-ico--purple {{ background: var(--purple-bg); color: var(--purple-text); border-color: var(--purple-border); }}

        .chart-bars {{ display: flex; flex-direction: column; gap: 0.85rem; }}
        .chart-bar-row {{
            display: grid; grid-template-columns: 108px minmax(0, 1fr) 82px;
            align-items: center; gap: 0.75rem;
        }}
        .chart-bar-label {{
            display: inline-flex; align-items: center; gap: 0.45rem;
            font-size: 0.82rem; font-weight: 600; color: var(--text-secondary);
            white-space: nowrap;
        }}
        .plat-dot, .legend-dot {{ width: 9px; height: 9px; border-radius: 9999px; flex-shrink: 0; }}
        .bar-track {{
            height: 10px; border-radius: 9999px; background: var(--bg-inset);
            border: 1px solid var(--border-subtle); overflow: hidden;
        }}
        .bar-fill {{ height: 100%; border-radius: 9999px; transition: width 0.35s ease; }}
        .chart-bar-val {{
            text-align: right; font-size: 0.8rem; font-family: 'JetBrains Mono', monospace;
            color: var(--text-primary); white-space: nowrap;
        }}
        .chart-bar-val span {{ color: var(--text-muted); font-size: 0.73rem; margin-left: 0.25rem; }}

        .donut-layout {{
            display: flex; align-items: center; justify-content: space-between;
            gap: 1.25rem; flex-wrap: wrap;
        }}
        .donut-wrap {{
            position: relative; width: 124px; height: 124px; flex-shrink: 0;
            display: inline-flex; align-items: center; justify-content: center;
        }}
        .chart-donut {{ display: block; }}
        .donut-center {{
            position: absolute; inset: 0; display: flex; flex-direction: column;
            align-items: center; justify-content: center; pointer-events: none;
        }}
        .donut-center strong {{
            font-size: 1.35rem; font-weight: 800; line-height: 1.1;
            font-variant-numeric: tabular-nums; color: var(--text-primary);
        }}
        .donut-center span {{ font-size: 0.7rem; font-weight: 600; color: var(--text-muted); }}
        .donut-legend {{ flex: 1 1 200px; display: flex; flex-direction: column; gap: 0.68rem; }}
        .legend-item {{
            display: flex; align-items: center; gap: 0.5rem;
            padding: 0.45rem 0.7rem; border-radius: var(--radius-sm);
            background: var(--bg-elevated); border: 1px solid var(--border-subtle);
            font-size: 0.8rem;
        }}
        .legend-name {{ flex: 1; color: var(--text-secondary); font-weight: 600; }}
        .legend-item strong {{ font-family: 'JetBrains Mono', monospace; color: var(--text-primary); }}
        .legend-pct {{ font-family: 'JetBrains Mono', monospace; font-size: 0.74rem; color: var(--text-muted); }}

        /* Scrollbar & Responsive Collapse */
        ::-webkit-scrollbar {{ width: 7px; height: 7px; }}
        ::-webkit-scrollbar-track {{ background: var(--bg-base); }}
        ::-webkit-scrollbar-thumb {{ background: var(--border-strong); border-radius: 9999px; }}
        ::-webkit-scrollbar-thumb:hover {{ background: var(--brand-primary); }}

        @media (max-width: 1180px) {{
            .kpi-grid {{ grid-template-columns: repeat(2, minmax(0, 1fr)); }}
            .charts-grid, .split-at, .grid-2, .grid-3 {{ grid-template-columns: minmax(0, 1fr); }}
        }}
        @media (max-width: 960px) {{
            .app-shell {{
                grid-template-columns: minmax(0, 1fr);
            }}
            .sidebar {{
                position: static;
                height: auto;
                border-right: none;
                border-bottom: 1px solid var(--border-subtle);
                padding: 0.9rem 1rem;
                gap: 0.8rem;
                overflow-y: visible;
            }}
            .brand {{
                padding-bottom: 0.75rem;
            }}
            .sidebar__section-label {{ display: none; }}
            .tab-nav {{
                flex-direction: row;
                overflow-x: auto;
                padding-bottom: 0.2rem;
            }}
            .tab-btn {{
                width: auto;
                flex: 0 0 auto;
                padding: 0.5rem 0.85rem;
            }}
            .tab-btn.active {{
                box-shadow: none;
            }}
            .sidebar__footer {{
                display: none;
            }}
            .topbar {{
                padding: 0.75rem 1rem;
            }}
        }}
        @media (max-width: 640px) {{
            .kpi-grid, .form-grid-2 {{ grid-template-columns: minmax(0, 1fr); }}
            .chart-bar-row {{ grid-template-columns: 92px minmax(0, 1fr) 72px; gap: 0.5rem; }}
            .admin-main {{ padding: 1rem; }}
            .hide-sm {{ display: none !important; }}
            .social-meta {{ align-items: flex-start; text-align: left; min-width: 0; }}
        }}
        @media (prefers-reduced-motion: no-preference) {{
            .kpi-card:hover, .chart-card:hover {{ transform: translateY(-2px); }}
        }}
        @media (prefers-reduced-motion: reduce) {{
            *, *::before, *::after {{ transition: none !important; animation: none !important; }}
        }}
    </style>
</head>
<body>
    {}
    <div class="app-shell">
        <!-- Left Sidebar -->
        <aside class="sidebar">
            <div class="brand">
                <div class="brand__logo-wrap">
                    <img src="/assets/bot-logo.webp" alt="Logo Thế Giới Siêu Sale" class="brand__logo">
                    <span class="brand__dot" aria-hidden="true"></span>
                </div>
                <div>
                    <div class="brand__title-row">
                        <h1 class="brand__title">THẾ GIỚI SIÊU SALE</h1>
                        <span class="brand__tag">Console</span>
                    </div>
                    <p class="brand__sub">Trung Tâm Điều Hành &amp; Săn Sale Đa Sàn</p>
                </div>
            </div>

            <div>
                <div class="sidebar__section-label">Điều Hướng Hệ Thống</div>
                <nav class="tab-nav" aria-label="Danh mục quản trị">
                    <button type="button" onclick="switchTab('overview')" id="tab-btn-overview" class="tab-btn active" aria-current="page">
                        <span class="tab-btn__icon" aria-hidden="true"><svg class="ico"><use href="#ico-overview"/></svg></span>
                        <span>Tổng Quan</span>
                    </button>
                    <button type="button" onclick="switchTab('platforms')" id="tab-btn-platforms" class="tab-btn">
                        <span class="tab-btn__icon" aria-hidden="true"><svg class="ico"><use href="#ico-platforms"/></svg></span>
                        <span>Kết Nối Sàn &amp; API</span>
                    </button>
                    <button type="button" onclick="switchTab('vouchers')" id="tab-btn-vouchers" class="tab-btn">
                        <span class="tab-btn__icon" aria-hidden="true"><svg class="ico"><use href="#ico-vouchers"/></svg></span>
                        <span>Kho Voucher Thị Trường</span>
                    </button>
                    <button type="button" onclick="switchTab('orders')" id="tab-btn-orders" class="tab-btn">
                        <span class="tab-btn__icon" aria-hidden="true"><svg class="ico"><use href="#ico-orders"/></svg></span>
                        <span>Báo Đơn &amp; Doanh Thu</span>
                    </button>
                </nav>
            </div>

            <div class="sidebar__footer">
                <div class="bot-status-card">
                    <div class="bot-status-card__row">
                        <span class="cell-muted">Bot Telegram</span>
                        <span style="display:inline-flex;align-items:center;gap:0.35rem;">
                            <span class="pulse-dot" aria-hidden="true"></span>
                            <a href="https://t.me/{}" target="_blank" rel="noopener noreferrer" class="bot-link">@{}</a>
                        </span>
                    </div>
                    <div class="bot-status-card__row">
                        <span class="cell-muted">Chế độ chạy</span>
                        {}
                    </div>
                </div>
                <a href="/" target="_blank" rel="noopener noreferrer" class="btn btn--secondary btn--sm btn--block">
                    <svg class="ico ico--sm" aria-hidden="true"><use href="#ico-globe"/></svg>
                    <span>Xem Trang Đích Bot 3D ↗</span>
                </a>
            </div>
        </aside>

        <!-- Main Column -->
        <div class="shell-main">
            <header class="topbar">
                <div class="topbar__left">
                    <h2 id="top-page-title" class="topbar__title">Tổng Quan Hệ Thống</h2>
                    <p id="top-page-subtitle" class="topbar__sub">Theo dõi lưu lượng Bot Telegram, Kho Mã Thị Trường &amp; Social Harvester thời gian thực</p>
                </div>
                <div class="topbar__actions">
                    <div class="admin-pill hide-sm">
                        <span>Admin: <strong>{}</strong></span>
                    </div>
                    {}
                    <button type="button" id="theme-toggle-btn" class="theme-btn" onclick="toggleTheme()" aria-label="Chuyển đổi giao diện Sáng/Tối">
                        <svg class="ico ico--xs" aria-hidden="true"><use id="theme-ico-use" href="#ico-moon"/></svg>
                        <span id="theme-btn-text">Tối</span>
                    </button>
                </div>
            </header>

            <main class="admin-main">
                {}
                {}

                <!-- TAB 1: TỔNG QUAN (OVERVIEW) -->
                <div id="tab-pane-overview" class="tab-pane">
                    <section class="kpi-grid" aria-label="Chỉ số tổng quan">
                        <div class="kpi-card">
                            <div>
                                <div class="kpi-card__top">
                                    <span>Người Dùng Telegram</span>
                                    <span class="kpi-card__icon kpi-card__icon--blue" aria-hidden="true"><svg class="ico ico--sm"><use href="#ico-users"/></svg></span>
                                </div>
                                <div class="kpi-card__val">{}</div>
                            </div>
                            <div class="kpi-card__pill">Khách thực tế đã tương tác bot</div>
                        </div>

                        <div class="kpi-card">
                            <div>
                                <div class="kpi-card__top">
                                    <span>Tổng Lượt Dán Link</span>
                                    <span class="kpi-card__icon kpi-card__icon--amber" aria-hidden="true"><svg class="ico ico--sm"><use href="#ico-clicks"/></svg></span>
                                </div>
                                <div class="kpi-card__val kpi-card__val--amber">{}</div>
                            </div>
                            <div class="kpi-card__meta">
                                <div class="kpi-row"><span>Shopee: <strong>{}</strong></span><span>Lazada: <strong>{}</strong></span></div>
                                <div class="kpi-row"><span>TikTok: <strong>{}</strong></span><span>Tiki: <strong>{}</strong></span></div>
                            </div>
                        </div>

                        <div class="kpi-card kpi-card--orange">
                            <div>
                                <div class="kpi-card__top">
                                    <span>Kho Mã Thị Trường</span>
                                    <span class="kpi-card__icon kpi-card__icon--orange" aria-hidden="true"><svg class="ico ico--sm"><use href="#ico-flame"/></svg></span>
                                </div>
                                <div class="kpi-card__val kpi-card__val--orange">{}</div>
                            </div>
                            <div class="kpi-card__meta">
                                <div class="kpi-row"><span>Giảm ≥20%: <strong>{}</strong></span><span>Tiền/Khác: <strong>{}</strong></span></div>
                                <div class="kpi-row"><span>Shopee: <strong>{}</strong></span><span>Tiki: <strong>{}</strong></span><span>Độc quyền: <strong>{}</strong></span></div>
                            </div>
                        </div>

                        <div class="kpi-card kpi-card--green">
                            <div>
                                <div class="kpi-card__top">
                                    <span>Tự Động Quét Nền</span>
                                    <span class="kpi-card__icon kpi-card__icon--green" aria-hidden="true"><svg class="ico ico--sm"><use href="#ico-clock"/></svg></span>
                                </div>
                                <div class="kpi-card__val--green">Chu kỳ 60 giây / lần</div>
                                <div class="muted-text">Lần cuối: <strong class="meta-time">{}</strong></div>
                            </div>
                            <div class="refresh-chip">
                                Tự làm mới Dashboard mỗi 30 giây
                            </div>
                        </div>
                    </section>

                    {}

                    <!-- Widget Shopee Social Harvester -->
                    <section class="card card--social">
                        <div class="social-left">
                            <div class="social-icon" aria-hidden="true"><svg class="ico" style="width:24px;height:24px;"><use href="#ico-bot"/></svg></div>
                            <div>
                                <div class="card-title">
                                    <span>Shopee Social Harvester — Tự Động Nhặt Mã &amp; Chữ Ký FB / IG Reels</span>
                                    {}
                                </div>
                                <p class="card-desc">
                                    Hệ thống chạy tự động hoàn toàn ngay khi mở app &amp; tự quét lại mỗi 5 phút từ Telegram/Web deal công khai để nhặt <b>Mã Instagram / Facebook KOL</b> và chữ ký số (<code>channel_type=fb/ig</code>, tự nâng cấp <code>sm_sig_id</code> nếu phát hiện) — không cần thao tác tay!
                                </p>
                            </div>
                        </div>
                        {}
                    </section>

                    <!-- Lịch sử dán link gần nhất -->
                    <section class="card card--table">
                        <div class="card-head card-head--table">
                            <div>
                                <h2 class="card-title"><svg class="ico" aria-hidden="true"><use href="#ico-history"/></svg><span>Lịch Sử Khách Dán Link Săn Sale Gần Đây (50 lượt mới nhất)</span></h2>
                                <p class="card-desc">Theo dõi thời gian thực các liên kết khách gửi vào bot Telegram</p>
                            </div>
                        </div>
                        <div class="table-wrap">
                            <table class="data-table">
                                <thead>
                                    <tr>
                                        <th>Telegram ID</th>
                                        <th>Sàn</th>
                                        <th>Link Khách Gửi</th>
                                        <th>Link Đầu Ra</th>
                                        <th>Thời Gian</th>
                                    </tr>
                                </thead>
                                <tbody id="clicks-tbody">
                                    {}
                                </tbody>
                            </table>
                        </div>
                    </section>
                </div>

                <!-- TAB 2: KẾT NỐI SÀN & API (PLATFORMS) -->
                <div id="tab-pane-platforms" class="tab-pane is-hidden">
                    <section class="card">
                        <div class="card-head">
                            <div>
                                <h2 class="card-title"><svg class="ico" aria-hidden="true"><use href="#ico-rocket"/></svg><span>Kết Nối ACCESSTRADE API (Tự Động Đa Sàn)</span></h2>
                                <p class="card-desc">
                                    Bot tự động gọi API <code>v1/product_link/create</code> &amp; TikTok v2 Universal Deeplink, đồng thời kiểm tra HTTP 302/200 chống lỗi link 404/500.
                                </p>
                            </div>
                            <div class="card-head-actions">
                                {}
                                <form method="POST" action="/admin/settings/accesstrade/sync" class="inline-form">
                                    <button type="submit" class="btn btn--secondary btn--sm">🔄 Đồng Bộ Lại Chiến Dịch</button>
                                </form>
                            </div>
                        </div>

                        <div class="split-at">
                            <div class="plat-panel">
                                <div class="plat-panel__title">Trạng Thái Duyệt &amp; Chuẩn Hóa Link</div>
                                <div>{}</div>
                                <div class="inset-note">
                                    <div><b>🛡️ Cơ chế bảo vệ Link Đầu Ra 100% Sống:</b></div>
                                    <div>• <b>TikTok Shop:</b> Link v2 <code>tiktok.com/t/...</code> mở thẳng sản phẩm trên App.</div>
                                    <div>• <b>Tiki &amp; Alibaba:</b> Tự dùng gói <b>TIKI CREATOR</b> &amp; <b>ALIBABA</b> đã duyệt + kiểm tra gateway <code>HTTP 200</code> trước khi gửi.</div>
                                    <div>• <b>Shopee / Lazada:</b> Hỗ trợ song song Shopee Chính Chủ &amp; Deeplink dự phòng.</div>
                                </div>
                            </div>

                            <form method="POST" action="/admin/settings/accesstrade" class="form-stack">
                                <div>
                                    <label class="field-label" for="at-api-token">ACCESSTRADE API Access Key (Token)</label>
                                    <input id="at-api-token" type="password" name="api_token" placeholder="Dán API Access Key mới (hoặc để trống để giữ Token hiện tại)..." class="field-input field-input--mono">
                                    <p class="muted-text">💡 Đã tích hợp sẵn API Token từ cấu hình hệ thống — tự động giữ kết nối khi khởi động lại.</p>
                                </div>

                                <div class="form-grid-2">
                                    <div>
                                        <label class="field-label" for="camp-shopee">Campaign ID Shopee (Tùy chọn)</label>
                                        <input id="camp-shopee" type="text" name="campaign_shopee" value="{}" placeholder="Tự động nhận diện nếu để trống" class="field-input field-input--mono">
                                    </div>
                                    <div>
                                        <label class="field-label" for="camp-lazada">Campaign ID Lazada (Tùy chọn)</label>
                                        <input id="camp-lazada" type="text" name="campaign_lazada" value="{}" placeholder="Tự động nhận diện nếu để trống" class="field-input field-input--mono">
                                    </div>
                                    <div>
                                        <label class="field-label" for="camp-tiki">Campaign ID Tiki (Tùy chọn)</label>
                                        <input id="camp-tiki" type="text" name="campaign_tiki" value="{}" placeholder="Tự động nhận diện nếu để trống" class="field-input field-input--mono">
                                    </div>
                                    <div>
                                        <label class="field-label" for="camp-tiktok">Campaign ID TikTok Shop (Tùy chọn)</label>
                                        <input id="camp-tiktok" type="text" name="campaign_tiktok" value="{}" placeholder="Tự động nhận diện nếu để trống" class="field-input field-input--mono">
                                    </div>
                                </div>

                                <div>
                                    <button type="submit" class="btn btn--primary">Lưu &amp; Kích Hoạt ACCESSTRADE API</button>
                                </div>
                            </form>
                        </div>
                    </section>

                    <!-- Card Shopee Chính Chủ & Bọc Link -->
                    {}

                    <!-- Cấu hình Link Tiếp Thị Dự Phòng (Deeplink Template) -->
                    <section class="card">
                        <div>
                            <h2 class="card-title"><svg class="ico" aria-hidden="true"><use href="#ico-platforms"/></svg><span>Cấu Hình Deeplink Dự Phòng (Nếu Chưa Dùng API Token)</span></h2>
                            <p class="card-desc">
                                Nếu chiến dịch trên API chưa duyệt, hệ thống sẽ kiểm tra mẫu link dự phòng này. Nếu để trống, bot tự động trả về link sản phẩm sạch để khách mở App mua hàng không bị lỗi 404.
                            </p>
                        </div>
                        <form method="POST" action="/admin/settings/affiliate" class="form-stack">
                            <div>
                                <label class="field-label" for="aff-template">Mẫu Deeplink Template</label>
                                <input id="aff-template" type="text" name="template" value="{}" placeholder="Ví dụ: https://go.isclix.com/deep_link/PUB_ID/CAMPAIGN_ID?url={{CLEAN_URL}}&amp;utm_source=facebook&amp;sub1={{USER_ID}}" class="field-input field-input--mono">
                            </div>
                            <div>
                                <button type="submit" class="btn btn--secondary">Lưu Deeplink Dự Phòng</button>
                            </div>
                        </form>
                    </section>
                </div>

                <!-- TAB 3: KHO VOUCHER THỊ TRƯỜNG & ĐỘC QUYỀN (VOUCHERS) -->
                <div id="tab-pane-vouchers" class="tab-pane is-hidden">
                    <section class="card">
                        <div class="card-head">
                            <div>
                                <h2 class="card-title">
                                    <svg class="ico" aria-hidden="true"><use href="#ico-flame"/></svg>
                                    <span>Kho Mã Giảm Giá Toàn Thị Trường (Cập Nhật Mỗi 60 Giây)</span>
                                    <span class="badge badge--orange">{} mã đang sẵn ({} mã &gt;=20%)</span>
                                </h2>
                                <p class="card-desc">
                                    Quét từ hệ thống ACCESSTRADE (mã %, giảm tiền K/VNĐ, mã hot, coupon Tiki/Shopee/Lazada/TikTok). Lần quét gần nhất: <strong class="ok-strong">{}</strong>.
                                </p>
                            </div>
                            <form method="POST" action="/admin/vouchers/sync" class="inline-form">
                                <button type="submit" class="btn btn--orange-soft">Quét Sâu Toàn Bộ Ngay</button>
                            </form>
                        </div>

                        <form method="GET" action="/admin" class="filter-bar">
                            <input type="text" name="q" value="{}" aria-label="Tìm kiếm mã giảm giá" placeholder="Tìm tên shop, mã code, mức giảm (VD: 35%, LANGXDKKP, Tiki)..." class="field-input grow-field">
                            <select name="plat" aria-label="Lọc theo sàn hoặc mức giảm" class="field-input">
                                <option value="all" {}>🌟 Tất Cả Sàn &amp; Loại</option>
                                <option value="pct20" {}>🔥 Giảm Sâu 20% - 35%+</option>
                                <option value="other" {}>💸 Giảm Tiền K/VNĐ &amp; Khác</option>
                                <option value="Shopee" {}>🟠 Chỉ Shopee</option>
                                <option value="Tiki" {}>🔹 Chỉ Tiki</option>
                            </select>
                            <button type="submit" class="btn btn--secondary btn--sm">Lọc Mã</button>
                            <span class="cell-muted">Hiển thị <strong class="primary-strong">{}</strong> / <strong>{}</strong> mã khớp</span>
                        </form>

                        <div class="table-wrap">
                            <table class="data-table">
                                <thead>
                                    <tr>
                                        <th>Mức Giảm</th>
                                        <th>Sàn</th>
                                        <th>Mã Code</th>
                                        <th>Gian Hàng &amp; Điều Kiện Áp Mã</th>
                                        <th>HSD</th>
                                        <th>Link Áp Mã</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {}
                                </tbody>
                            </table>
                        </div>
                    </section>

                    <section class="card card--purple">
                        <div class="card-head">
                            <div>
                                <h2 class="card-title">
                                    <svg class="ico" aria-hidden="true"><use href="#ico-crown"/></svg>
                                    <span>Kho Mã Độc Quyền Hợp Tác (Cấp Riêng AM / MCN / Nhãn Hàng)</span>
                                    <span class="badge badge--purple">{} mã đã lưu</span>
                                </h2>
                                <p class="card-desc">
                                    Dán hàng loạt mã độc quyền nhận được từ Account Manager ACCESSTRADE (MCN / Super Hoa Hồng) hoặc hợp tác trực tiếp với Shop. Các mã này luôn được <b>ưu tiên hàng đầu</b> trên Telegram Bot!
                                </p>
                            </div>
                        </div>

                        <div class="grid-2">
                            <form method="POST" action="/admin/deals/import" class="subpanel">
                                <div class="subpanel__title subpanel__title--purple">⚡ Nhập Nhanh Hàng Loạt (Hỗ trợ dấu | hoặc copy thẳng từ Excel/Google Sheets)</div>
                                <p class="card-desc" style="margin: 0;">
                                    Mỗi dòng 1 mã theo thứ tự: <code>Sàn | Tên/Shop | Mã Code | Mức giảm | Link áp mã | Đơn tối thiểu</code>
                                </p>
                                <textarea name="bulk_lines" rows="5" required aria-label="Danh sách mã độc quyền nhập hàng loạt" placeholder="Shopee | Deal Độc Quyền MCN 30% | ATVIP30 | Giảm 30% tối đa 100k | https://shopee.vn/search?... | 200000" class="code-textarea"></textarea>
                                <div>
                                    <button type="submit" class="btn btn--purple">📥 Nhập Danh Sách Mã Độc Quyền</button>
                                </div>
                            </form>

                            <form method="POST" action="/admin/deals/add" class="subpanel">
                                <div class="subpanel__title">➕ Thêm Lẻ 1 Mã Độc Quyền</div>
                                <div class="form-grid-2">
                                    <select name="platform" aria-label="Chọn sàn" class="field-input">
                                        <option value="Shopee">Shopee</option>
                                        <option value="TikTok Shop">TikTok Shop</option>
                                        <option value="Lazada">Lazada</option>
                                        <option value="Tiki">Tiki</option>
                                        <option value="Alibaba">Alibaba</option>
                                    </select>
                                    <input type="text" name="coupon_code" required placeholder="Mã Code (VD: ATVIP35)" class="field-input field-input--mono">
                                    <input type="text" name="title" required placeholder="Tên Shop / Chương trình" class="field-input">
                                    <input type="text" name="discount_desc" required placeholder="Mức giảm (VD: Giảm 35% tối đa 120k)" class="field-input">
                                    <input type="url" name="deal_url" required placeholder="Link áp mã (https://...)" class="field-input field-input--mono">
                                    <input type="number" name="min_spend" min="0" step="1000" value="0" placeholder="Đơn tối thiểu (VNĐ)" class="field-input field-input--mono">
                                </div>
                                <label class="check-row">
                                    <input type="checkbox" name="is_exclusive" value="1" checked>
                                    <span>Đánh dấu là 👑 Mã Độc Quyền Hợp Tác (Ưu tiên hiển thị đầu tiên)</span>
                                </label>
                                <div>
                                    <button type="submit" class="btn btn--secondary">💾 Lưu Mã Độc Quyền</button>
                                </div>
                            </form>
                        </div>

                        <div class="table-wrap">
                            <table class="data-table">
                                <thead>
                                    <tr>
                                        <th>Loại</th>
                                        <th>Sàn</th>
                                        <th>Tên &amp; Mô Tả</th>
                                        <th>Mã Code</th>
                                        <th>Trạng Thái</th>
                                        <th>Hành Động</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {}
                                </tbody>
                            </table>
                        </div>
                    </section>
                </div>

                <!-- TAB 4: BÁO ĐƠN & DOANH THU (ORDERS) -->
                <div id="tab-pane-orders" class="tab-pane is-hidden">
                    {}
                </div>
            </main>

            <footer class="admin-footer">
                Thế Giới Siêu Sale Affiliate System · Bản Quyền 2026 · Vận hành bảo mật trên Rust + LibSQL
            </footer>
        </div>
    </div>

    <script>
        const TAB_META = {{
            overview: {{
                title: 'Tổng Quan Hệ Thống',
                sub: 'Theo dõi lưu lượng Bot Telegram, Kho Mã Thị Trường & Social Harvester thời gian thực'
            }},
            platforms: {{
                title: 'Kết Nối Sàn & API',
                sub: 'Cấu hình ACCESSTRADE đa sàn, Shopee Chính Chủ & Cloudflare Tunnel tự động'
            }},
            vouchers: {{
                title: 'Kho Voucher Thị Trường',
                sub: 'Tra cứu mã giảm giá quét tự động và quản lý Mã Độc Quyền Hợp Tác'
            }},
            orders: {{
                title: 'Báo Đơn & Doanh Thu',
                sub: 'Cài đặt thông báo Telegram Admin và đối soát hoa hồng đơn hàng'
            }}
        }};

        function syncThemeButton() {{
            const btn = document.getElementById('theme-toggle-btn');
            if (!btn) return;
            const isDark = document.documentElement.getAttribute('data-theme') === 'dark';
            const useEl = document.getElementById('theme-ico-use');
            const txtEl = document.getElementById('theme-btn-text');
            if (useEl) useEl.setAttribute('href', isDark ? '#ico-sun' : '#ico-moon');
            if (txtEl) txtEl.textContent = isDark ? 'Sáng' : 'Tối';
            btn.setAttribute('aria-pressed', isDark ? 'true' : 'false');
        }}

        function toggleTheme() {{
            const isDark = document.documentElement.getAttribute('data-theme') === 'dark';
            const next = isDark ? 'light' : 'dark';
            if (next === 'dark') {{
                document.documentElement.setAttribute('data-theme', 'dark');
            }} else {{
                document.documentElement.removeAttribute('data-theme');
            }}
            try {{ localStorage.setItem('dealx-theme', next); }} catch (e) {{}}
            syncThemeButton();
        }}

        function switchTab(tabId) {{
            document.querySelectorAll('.tab-pane').forEach(el => el.classList.add('is-hidden'));
            document.querySelectorAll('.tab-btn').forEach(btn => {{
                btn.classList.remove('active');
                btn.removeAttribute('aria-current');
            }});
            const target = document.getElementById('tab-pane-' + tabId);
            const btn = document.getElementById('tab-btn-' + tabId);
            if (target) target.classList.remove('is-hidden');
            if (btn) {{
                btn.classList.add('active');
                btn.setAttribute('aria-current', 'page');
            }}
            const meta = TAB_META[tabId];
            if (meta) {{
                const tEl = document.getElementById('top-page-title');
                const sEl = document.getElementById('top-page-subtitle');
                if (tEl) tEl.textContent = meta.title;
                if (sEl) sEl.textContent = meta.sub;
            }}
            try {{
                history.replaceState(null, '', '#' + tabId);
            }} catch (e) {{}}
        }}

        window.addEventListener('DOMContentLoaded', () => {{
            syncThemeButton();
            const csrfMeta = document.querySelector('meta[name="csrf-token"]');
            const csrfVal = csrfMeta ? csrfMeta.getAttribute('content') || '' : '';
            if (csrfVal) {{
                document.querySelectorAll('form').forEach((formEl) => {{
                    if ((formEl.method || 'GET').toUpperCase() === 'POST' && !formEl.querySelector('input[name="_csrf"]')) {{
                        const hidden = document.createElement('input');
                        hidden.type = 'hidden';
                        hidden.name = '_csrf';
                        hidden.value = csrfVal;
                        formEl.appendChild(hidden);
                    }}
                }});
                if (!window.__dealxCsrfFetchWrapped && typeof window.fetch === 'function') {{
                    window.__dealxCsrfFetchWrapped = true;
                    const origFetch = window.fetch.bind(window);
                    window.fetch = (input, init = {{}}) => {{
                        const method = String((init && init.method) || (input && input.method) || 'GET').toUpperCase();
                        const reqUrl = typeof input === 'string' ? input : (input && input.url) || '';
                        const isSameOrigin = reqUrl.startsWith('/') || reqUrl.startsWith(window.location.origin);
                        if (isSameOrigin && method !== 'GET' && method !== 'HEAD') {{
                            const headers = new Headers((init && init.headers) || {{}});
                            if (!headers.has('X-CSRF-Token')) {{
                                headers.set('X-CSRF-Token', csrfVal);
                            }}
                            return origFetch(input, Object.assign({{}}, init, {{ headers }}));
                        }}
                        return origFetch(input, init);
                    }};
                }}
            }}
            let hash = window.location.hash.replace('#', '');
            if (!['overview', 'platforms', 'vouchers', 'orders'].includes(hash)) {{
                const urlParams = new URLSearchParams(window.location.search);
                if (urlParams.has('q') || urlParams.has('plat')) {{
                    hash = 'vouchers';
                }} else {{
                    hash = 'overview';
                }}
            }}
            switchTab(hash);
        }});

        let refreshingOverview = false;
        setInterval(async () => {{
            if (document.hidden || refreshingOverview) return;
            const overviewPane = document.getElementById('tab-pane-overview');
            if (!overviewPane || overviewPane.classList.contains('is-hidden')) return;
            const activeTag = document.activeElement ? document.activeElement.tagName : '';
            if (['INPUT', 'TEXTAREA', 'SELECT'].includes(activeTag)) return;
            refreshingOverview = true;
            try {{
                const res = await fetch('/admin', {{ cache: 'no-store' }});
                if (!res.ok) return;
                const html = await res.text();
                const doc = new DOMParser().parseFromString(html, 'text/html');
                const savedScrollY = window.scrollY;
                ['.kpi-grid', '.charts-grid', '.card--social', '#clicks-tbody'].forEach((sel) => {{
                    const cur = document.querySelector(sel);
                    const next = doc.querySelector(sel);
                    if (cur && next && cur.innerHTML !== next.innerHTML) {{
                        cur.innerHTML = next.innerHTML;
                    }}
                }});
                if (Math.abs(window.scrollY - savedScrollY) > 4) {{
                    window.scrollTo(0, savedScrollY);
                }}
            }} catch (e) {{}} finally {{
                refreshingOverview = false;
            }}
        }}, 30000);
    </script>
    <script src="/assets/home.js" defer></script>
</body>
</html>"##,
        ADMIN_SVG_SPRITE,
        escape_html(&state.config.bot_username),
        escape_html(&state.config.bot_username),
        bot_mode_badge,
        escape_html(&state.config.admin_username),
        auth_action_html,
        notice_banner,
        persistence_banner,
        stats.total_users,
        stats.total_clicks,
        stats.shopee_clicks,
        stats.lazada_clicks,
        stats.tiktok_clicks,
        stats.tiki_clicks,
        total_market_vouchers,
        high_pct_vouchers,
        other_vouchers_count,
        shopee_vouchers,
        tiki_vouchers,
        saved_deals.len(),
        escape_html(&last_sync_display),
        charts_html,
        social_harvester_status_html,
        social_auto_panel_html,
        clicks_rows,
        at_status_badge,
        campaign_status_html,
        escape_html(&camp_shopee),
        escape_html(&camp_lazada),
        escape_html(&camp_tiki),
        escape_html(&camp_tiktok),
        shopee_card_html,
        escape_html(&affiliate_template),
        total_market_vouchers,
        high_pct_vouchers,
        escape_html(&last_sync_display),
        escape_html(&search_q),
        sel_all,
        sel_pct20,
        sel_other,
        sel_shopee,
        sel_tiki,
        shown_vouchers_count,
        matched_vouchers_count,
        voucher_rows,
        saved_deals.len(),
        exclusive_rows,
        order_alerts_card_html
    );

    let mut dashboard_html = inject_csrf_into_post_forms(&dashboard_html, &csrf_token);
    if dashboard_html.contains("</head>") {
        let meta_tag = format!(
            r#"<meta name="csrf-token" content="{}"></head>"#,
            escape_html(&csrf_token)
        );
        dashboard_html = dashboard_html.replacen("</head>", &meta_tag, 1);
    }

    let mut response = Html(dashboard_html).into_response();
    let secure_attr = if is_cloud_runtime { "; Secure" } else { "" };
    let csrf_cookie = format!(
        "dealx_csrf={}; HttpOnly; Path=/; SameSite=Lax{}",
        csrf_token, secure_attr
    );
    if let Ok(cookie_val) = HeaderValue::from_str(&csrf_cookie) {
        response.headers_mut().append(header::SET_COOKIE, cookie_val);
    }
    response
}

fn inject_csrf_into_post_forms(html: &str, csrf_token: &str) -> String {
    let csrf_input = format!(
        r#"<input type="hidden" name="_csrf" value="{}">"#,
        escape_html(csrf_token)
    );
    let mut out = String::with_capacity(html.len() + 512);
    let mut rest = html;
    while let Some(form_start) = rest.find("<form") {
        let Some(tag_end_rel) = rest[form_start..].find('>') else {
            break;
        };
        let tag_end = form_start + tag_end_rel + 1;
        out.push_str(&rest[..tag_end]);
        let form_tag_lower = rest[form_start..tag_end].to_ascii_lowercase();
        if form_tag_lower.contains("method=\"post\"") || form_tag_lower.contains("method='post'") {
            out.push_str(&csrf_input);
        }
        rest = &rest[tag_end..];
    }
    out.push_str(rest);
    out
}

/// Form cập nhật cấu hình ACCESSTRADE API & Campaign ID
#[derive(serde::Deserialize)]
pub struct AccessTradeForm {
    pub api_token: String,
    pub campaign_shopee: String,
    pub campaign_lazada: String,
    pub campaign_tiki: String,
    pub campaign_tiktok: String,
}

pub(super) async fn handle_update_accesstrade(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<AccessTradeForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if let Ok(conn) = state.db.conn() {
        let clean_tok = crate::services::accesstrade::clean_api_token(&form.api_token);
        if !clean_tok.is_empty() {
            let _ = Repository::set_setting(&conn, "accesstrade_api_token", clean_tok).await;
            info!("✅ Quản trị viên đã cập nhật ACCESSTRADE API Token mới (đã làm sạch tiền tố) qua Web Admin.");
        }
        let _ = Repository::set_setting(&conn, "campaign_shopee", form.campaign_shopee.trim()).await;
        let _ = Repository::set_setting(&conn, "campaign_lazada", form.campaign_lazada.trim()).await;
        let _ = Repository::set_setting(&conn, "campaign_tiki", form.campaign_tiki.trim()).await;
        let _ = Repository::set_setting(&conn, "campaign_tiktok", form.campaign_tiktok.trim()).await;
    }

    state.db.invalidate_settings_cache().await;
    state.accesstrade.invalidate_cache().await;
    Redirect::to("/admin").into_response()
}

struct ShopeeCardView<'a> {
    aff_id: &'a str,
    app_id: &'a str,
    app_secret: &'a str,
    social_params: &'a str,
    shortlink_base_url: &'a str,
    shortlink_mobile_only: bool,
    public_home_cloak: bool,
    origin_base_url: &'a str,
    bot_username: &'a str,
    has_cf_tunnel_token: bool,
    quick_tunnel_url: Option<&'a str>,
    tunnel_is_running: bool,
}

fn render_shopee_card(view: ShopeeCardView<'_>) -> String {
    let ShopeeCardView {
        aff_id,
        app_id,
        app_secret,
        social_params,
        shortlink_base_url,
        shortlink_mobile_only,
        public_home_cloak,
        origin_base_url,
        bot_username,
        has_cf_tunnel_token,
        quick_tunnel_url,
        tunnel_is_running,
    } = view;
    let status_badge = if !app_id.trim().is_empty() && !app_secret.trim().is_empty() {
        format!(
            r#"<span class="badge badge--ok">🟢 Đang bật Open API (Secret: {})</span>"#,
            escape_html(&mask_token(app_secret))
        )
    } else if is_valid_shopee_affiliate_id(aff_id) {
        format!(
            r#"<span class="badge badge--ok">🟢 Đang bật Direct Affiliate ID: {}</span>"#,
            escape_html(aff_id.trim())
        )
    } else {
        r#"<span class="badge badge--muted">⚪ Chưa cấu hình — đang dùng luồng ACCESSTRADE</span>"#.to_string()
    };

    let tunnel_badge_html = if has_cf_tunnel_token && tunnel_is_running {
        r#"<span class="badge badge--ok">🌐 Named Tunnel đang chạy</span>"#.to_string()
    } else if let Some(qt) = quick_tunnel_url {
        format!(
            r#"<span class="badge badge--info">⚡ Auto Tunnel: {}</span>"#,
            escape_html(qt)
        )
    } else {
        r#"<span class="badge badge--warn">⚠️ Tunnel chưa bật</span>"#.to_string()
    };

    let tunnel_token_badge = if has_cf_tunnel_token {
        "🟢 Đã lưu Token cố định"
    } else {
        "Chưa gắn — đang tự chạy Quick Tunnel"
    };
    let tunnel_token_placeholder = if has_cf_tunnel_token {
        "Để trống để giữ Token hiện tại (hoặc dán Token mới để đổi)..."
    } else {
        "Dán Cloudflare Tunnel Token (eyJ... hoặc cả lệnh service install) để giữ cố định thegioisieusale.click..."
    };

    let secret_placeholder = if app_secret.trim().is_empty() {
        "Nhập Secret Key nếu đã được Shopee cấp Open API..."
    } else {
        "Để trống để giữ Secret Key hiện tại (đã lưu an toàn)..."
    };
    let social_placeholder = if social_params.trim().is_empty() {
        "Dán link/query phiên IG/FB Reels (có sm_sig_id / encrypted_payload) nếu có..."
    } else {
        "Đã lưu mẫu phiên IG/FB Reels (để trống để giữ nguyên mẫu hiện tại)..."
    };
    let social_badge = if social_params.trim().is_empty() {
        "Chưa gắn mẫu phiên IG/FB"
    } else {
        "🟢 Đã gắn bộ tham số IG/FB Reels"
    };
    let mobile_checked = if shortlink_mobile_only { "checked" } else { "" };
    let home_cloak_checked = if public_home_cloak { "checked" } else { "" };
    let mut worker_code = CLOUDFLARE_WORKER_TEMPLATE.replace(
        "https://origin.thegioisieusale.click",
        origin_base_url.trim_end_matches('/'),
    );
    let bot_clean = bot_username.trim().trim_start_matches('@');
    if !bot_clean.is_empty() {
        worker_code = worker_code.replace(
            r#"const BOT_USERNAME = "vvipass_vip_bot";"#,
            &format!(r#"const BOT_USERNAME = "{}";"#, bot_clean),
        );
    }

    format!(
        r##"<section class="card card--orange">
            <div class="card-head">
                <div>
                    <h2 class="card-title"><span class="plat-dot" style="background:#EA580C;width:12px;height:12px;" aria-hidden="true"></span><span>Kết Nối Shopee Affiliate Chính Chủ &amp; Bọc Link Riêng</span></h2>
                    <p class="card-desc">
                        Ưu tiên link Shopee chính chủ qua Open API hoặc <code>s.shopee.vn/an_redir</code>. Khi chạy trên PC, hệ thống <b>tự động bật Cloudflare Tunnel</b> ngay khi mở server — không cần thao tác tay!
                    </p>
                </div>
                <div class="card-head-actions">{}{}</div>
            </div>
            <form method="POST" action="/admin/settings/shopee" class="form-stack">
                <div class="grid-3">
                    <div>
                        <label class="field-label">Shopee Affiliate ID (Dùng ngay qua an_redir)</label>
                        <input type="text" name="shopee_affiliate_id" value="{}" placeholder="Ví dụ: 17375520550 (5-20 chữ số)" class="field-input field-input--mono">
                    </div>
                    <div>
                        <label class="field-label">Shopee Open API AppID (Tùy chọn)</label>
                        <input type="text" name="shopee_app_id" value="{}" placeholder="Nhập AppID từ affiliate.shopee.vn..." class="field-input field-input--mono">
                    </div>
                    <div>
                        <label class="field-label">Shopee Open API Secret Key (Tùy chọn)</label>
                        <input type="password" name="shopee_app_secret" placeholder="{}" class="field-input field-input--mono">
                    </div>
                </div>
                <div class="grid-2 divider-top">
                    <div class="form-stack">
                        <div>
                            <label class="field-label">Bộ Tham Số Phiên IG/FB Reels Passthrough (<span class="label-accent">{}</span>)</label>
                            <input type="password" name="shopee_social_params" placeholder="{}" class="field-input field-input--mono">
                            <label class="check-row check-row--mt">
                                <input type="checkbox" name="clear_social_params" value="1">
                                <span>Xóa bộ tham số phiên IG/FB đang lưu</span>
                            </label>
                        </div>
                    </div>
                    <div class="form-stack">
                        <div>
                            <label class="field-label">Tên Miền Bọc Link Riêng / Tunnel Đang Dùng</label>
                            <input type="text" name="shortlink_base_url" value="{}" placeholder="Mặc định: tự động Quick Tunnel hoặc https://thegioisieusale.click (nhập off để tắt)" class="field-input field-input--mono">
                        </div>
                        <div>
                            <label class="field-label">Cloudflare Named Tunnel Token (<span class="label-info">{}</span>)</label>
                            <input type="password" name="cloudflare_tunnel_token" placeholder="{}" class="field-input field-input--mono">
                            <label class="check-row" style="margin-top:0.45rem;">
                                <input type="checkbox" name="clear_tunnel_token" value="1">
                                <span>Xóa Tunnel Token đã lưu (quay về Auto Quick Tunnel)</span>
                            </label>
                        </div>
                        <label class="check-row">
                            <input type="checkbox" name="shortlink_mobile_only" value="1" {}>
                            <span>🛡️ Chặn máy tính soi link bọc (Chỉ cho điện thoại mở giống godeal.online)</span>
                        </label>
                        <label class="check-row">
                            <input type="checkbox" name="public_home_cloak" value="1" {}>
                            <span>🙈 Ẩn luôn Trang chủ <code>/</code> thành <code>404 Not Found</code> (Chỉ bật nếu không chờ duyệt web Shopee/AT)</span>
                        </label>
                    </div>
                </div>
                <div>
                    <button type="submit" class="btn btn--primary">
                        💾 Lưu Cấu Hình Shopee &amp; Tự Động Áp Dụng Tunnel
                    </button>
                </div>
            </form>
            <details class="cf-details">
                <summary class="cf-summary">
                    ☁️ Hướng dẫn gắn tên miền cố định (thegioisieusale.click) cho Cloudflare Tunnel (Tùy chọn — không gắn vẫn tự chạy Quick Tunnel)
                </summary>
                <div class="cf-body">
                    <ol class="cf-steps">
                        <li>Vào <b>one.dash.cloudflare.com</b> (Zero Trust → Networks → Tunnels) tạo 1 Tunnel mới, trỏ Public Hostname <code>origin.thegioisieusale.click</code> về <code>HTTP</code> <code>127.0.0.1:10000</code>, rồi dán Tunnel Token vào ô <b>Cloudflare Named Tunnel Token</b> ở trên và bấm <b>Lưu</b> (hệ thống tự kích hoạt ngay).</li>
                        <li>Vào <b>dash.cloudflare.com</b> → <b>Workers &amp; Pages</b> → chọn Worker của <code>thegioisieusale.click</code> → <b>Edit code</b>, dán toàn bộ code bên dưới vào rồi bấm <b>Deploy</b>.</li>
                        <li>Mọi đường dẫn bot gửi sẽ cố định dạng <code>https://thegioisieusale.click/8_ky_tu</code>; trang chủ <code>/</code> phục vụ Trang Đích 3D tại Cloudflare Edge và mọi đường dẫn nội bộ (<code>/admin</code>, <code>/convert</code>, <code>/health</code>...) đều trả <code>404 Not Found</code> khi truy cập từ Internet!</li>
                    </ol>
                    <textarea id="cf-worker-code" readonly rows="10" class="field-input field-input--mono code-textarea">{}</textarea>
                    <div>
                        <button type="button" class="btn btn--secondary js-copy-link" data-copy-target="cf-worker-code">
                            📋 Sao chép Code Cloudflare Worker
                        </button>
                    </div>
                </div>
            </details>
        </section>"##,
        status_badge,
        tunnel_badge_html,
        escape_html(aff_id),
        escape_html(app_id),
        secret_placeholder,
        social_badge,
        social_placeholder,
        escape_html(shortlink_base_url),
        tunnel_token_badge,
        tunnel_token_placeholder,
        mobile_checked,
        home_cloak_checked,
        escape_html(&worker_code)
    )
}

#[derive(serde::Deserialize)]
pub struct ShopeeSettingsForm {
    pub shopee_affiliate_id: String,
    pub shopee_app_id: String,
    pub shopee_app_secret: String,
    #[serde(default)]
    pub shopee_social_params: String,
    #[serde(default)]
    pub clear_social_params: Option<String>,
    #[serde(default)]
    pub shortlink_base_url: String,
    #[serde(default)]
    pub cloudflare_tunnel_token: String,
    #[serde(default)]
    pub clear_tunnel_token: Option<String>,
    #[serde(default)]
    pub shortlink_mobile_only: Option<String>,
    #[serde(default)]
    pub public_home_cloak: Option<String>,
}

pub(super) async fn handle_update_shopee_settings(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<ShopeeSettingsForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    let aff_trimmed = form.shopee_affiliate_id.trim();
    if !aff_trimmed.is_empty() && !is_valid_shopee_affiliate_id(aff_trimmed) {
        return redirect_with_notice("Shopee Affiliate ID phải là dãy gồm 5 đến 20 chữ số (hoặc để trống để tắt).");
    }

    let short_base_trimmed = form.shortlink_base_url.trim().trim_end_matches('/');
    if !short_base_trimmed.is_empty()
        && !crate::config::is_shortlink_disabled_value(short_base_trimmed)
        && !is_valid_http_url(short_base_trimmed)
    {
        return redirect_with_notice("Tên miền bọc link riêng phải bắt đầu bằng https:// hoặc http:// (hoặc nhập off để tắt).");
    }

    let social_trimmed = form.shopee_social_params.trim();
    let validated_social = if form.clear_social_params.as_deref() == Some("1") {
        Some(String::new())
    } else if !social_trimmed.is_empty() {
        let http_probe = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        match crate::services::social_harvester::resolve_social_params_from_input(&http_probe, social_trimmed).await {
            Some(clean) => Some(clean),
            None => {
                return redirect_with_notice(
                    "Mẫu phiên IG/FB Reels chưa hợp lệ (phải chứa sm_sig_id, encrypted_payload hoặc link có chữ ký FB/IG).",
                );
            }
        }
    } else {
        None
    };

    let tunnel_tok_trimmed = form.cloudflare_tunnel_token.trim();
    let validated_tunnel_tok = if form.clear_tunnel_token.as_deref() == Some("1") {
        Some(String::new())
    } else if !tunnel_tok_trimmed.is_empty() {
        match crate::services::tunnel::extract_tunnel_token(tunnel_tok_trimmed) {
            Some(tok) => Some(tok),
            None => {
                return redirect_with_notice(
                    "Cloudflare Tunnel Token chưa hợp lệ (phải chứa chuỗi token bắt đầu bằng eyJ...).",
                );
            }
        }
    } else {
        None
    };

    let Ok(conn) = state.db.conn() else {
        return redirect_with_notice("Lỗi kết nối cơ sở dữ liệu.");
    };

    let _ = Repository::set_setting(&conn, "shopee_affiliate_id", aff_trimmed).await;
    let _ = Repository::set_setting(&conn, "shopee_app_id", form.shopee_app_id.trim()).await;
    let secret_trimmed = form.shopee_app_secret.trim();
    if !secret_trimmed.is_empty() {
        let _ = Repository::set_setting(&conn, "shopee_app_secret", secret_trimmed).await;
    }
    if let Some(social_val) = validated_social {
        let _ = Repository::set_setting(&conn, "shopee_social_params", &social_val).await;
    }
    if let Some(tok_val) = validated_tunnel_tok {
        let _ = Repository::set_setting(&conn, "cloudflare_tunnel_token", &tok_val).await;
    }
    let _ = Repository::set_setting(&conn, "shortlink_base_url", short_base_trimmed).await;
    let mobile_flag = if form.shortlink_mobile_only.as_deref() == Some("1") {
        "1"
    } else {
        "0"
    };
    let _ = Repository::set_setting(&conn, "shortlink_mobile_only", mobile_flag).await;
    let home_cloak_flag = if form.public_home_cloak.as_deref() == Some("1") {
        "1"
    } else {
        "0"
    };
    let _ = Repository::set_setting(&conn, "public_home_cloak", home_cloak_flag).await;

    state.db.invalidate_settings_cache().await;
    crate::services::tunnel::ensure_tunnel_running(&state.db, &state.config).await;

    info!("✅ Quản trị viên đã cập nhật cấu hình Shopee Chính Chủ & Bọc Link trên Web Admin.");
    redirect_with_notice("Đã lưu cấu hình Shopee Chính Chủ & đồng bộ Cloudflare Tunnel ngay lập tức!")
}

pub(super) async fn handle_sync_accesstrade(headers: HeaderMap, State(state): State<AppState>) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if let Ok(conn) = state.db.conn()
        && let Some(token) = resolve_accesstrade_token(&conn, &state.config).await {
            let _ = state.accesstrade.get_approved_campaigns(&token, true).await;
        }

    Redirect::to("/admin").into_response()
}

fn render_order_alerts_card(admin_telegram_id: &str, orders: &[TrackedOrder]) -> String {
    let status_badge = if admin_telegram_id.trim().is_empty() {
        r#"<span class="badge badge--warn">🟡 Chưa gắn Telegram ID nhận báo đơn</span>"#.to_string()
    } else {
        format!(
            r#"<span class="badge badge--ok">🟢 Đang báo đơn tới ID: {}</span>"#,
            escape_html(admin_telegram_id.trim())
        )
    };

    let mut order_rows = String::new();
    for ord in orders {
        let status_cls = match ord.status.as_str() {
            "APPROVED" => "badge badge--ok",
            "REJECTED" => "badge badge--err",
            _ => "badge badge--warn",
        };
        order_rows.push_str(&format!(
            r#"<tr>
                <td class="cell-strong">{}</td>
                <td class="font-mono cell-muted">{}</td>
                <td><span class="{}">{}</span></td>
                <td class="cell-strong">{}</td>
                <td class="font-mono comm-val">{}</td>
                <td class="font-mono cell-muted">{}</td>
            </tr>"#,
            escape_html(&ord.platform),
            escape_html(&ord.order_id),
            status_cls,
            escape_html(&ord.status),
            escape_html(&format_vnd(ord.order_amount)),
            escape_html(&format_vnd(ord.commission)),
            escape_html(&format_timestamp_vn(&ord.order_time))
        ));
    }
    if order_rows.is_empty() {
        order_rows = r#"<tr><td colspan="6" class="empty-cell">Chưa có đơn hàng phát sinh được ghi nhận trong 7 ngày gần nhất. Khi có đơn thật từ Shopee / TikTok Shop / ACCESSTRADE, hệ thống sẽ tự lưu và báo ngay về Telegram!</td></tr>"#.to_string();
    }

    format!(
        r##"<section class="card card--green">
            <div class="card-head">
                <div>
                    <h2 class="card-title"><svg class="ico" aria-hidden="true"><use href="#ico-orders"/></svg><span>Báo Đơn Tự Động Về Telegram &amp; Đối Soát Hoa Hồng (Mỗi 10 Phút)</span></h2>
                    <p class="card-desc">
                        Gõ lệnh <code>/stats</code> trong bot Telegram để lấy ID số của bạn rồi dán vào ô bên dưới. Mỗi khi có đơn mới trên ACCESSTRADE / Shopee Open API, bot sẽ nhắn tin báo ngay!
                    </p>
                </div>
                <div>{}</div>
            </div>
            <div class="filter-bar">
                <form method="POST" action="/admin/settings/notifications" class="inline-form-grow">
                    <div class="grow-field">
                        <label class="field-label">Telegram ID Admin Nhận Thông Báo Đơn</label>
                        <input type="text" name="admin_telegram_id" value="{}" placeholder="Ví dụ: 5123456789 (hoặc để trống để tắt)" class="field-input font-mono">
                    </div>
                    <button type="submit" class="btn btn--success">
                        💾 Lưu Telegram ID Báo Đơn
                    </button>
                </form>
                <form method="POST" action="/admin/orders/sync">
                    <button type="submit" class="btn btn--secondary">
                        🔄 Quét Đơn Mới Ngay
                    </button>
                </form>
            </div>
            <div class="table-wrap">
                <table class="data-table">
                    <thead>
                        <tr>
                            <th>Sàn</th>
                            <th>Mã Đơn Hàng</th>
                            <th>Trạng Thái</th>
                            <th>Giá Trị Đơn</th>
                            <th>Hoa Hồng</th>
                            <th>Thời Gian Đặt</th>
                        </tr>
                    </thead>
                    <tbody>
                        {}
                    </tbody>
                </table>
            </div>
        </section>"##,
        status_badge,
        escape_html(admin_telegram_id.trim()),
        order_rows
    )
}

#[derive(serde::Deserialize)]
pub struct NotificationsForm {
    pub admin_telegram_id: String,
}

pub(super) async fn handle_update_notifications(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<NotificationsForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    let trimmed = form.admin_telegram_id.trim();
    if !trimmed.is_empty() && trimmed.parse::<i64>().is_err() {
        return redirect_with_notice("Telegram ID phải là dãy số nguyên (lấy bằng cách gõ /stats trong Bot).");
    }

    let Ok(conn) = state.db.conn() else {
        return redirect_with_notice("Lỗi kết nối cơ sở dữ liệu.");
    };

    let _ = Repository::set_setting(&conn, "admin_telegram_id", trimmed).await;
    state.db.invalidate_settings_cache().await;
    if trimmed.is_empty() {
        redirect_with_notice("Đã tắt thông báo đơn hàng qua Telegram.")
    } else {
        redirect_with_notice(&format!("Đã lưu Telegram ID {} để nhận báo đơn tự động!", trimmed))
    }
}

pub(super) async fn handle_sync_orders(headers: HeaderMap, State(state): State<AppState>) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    let summary = order_tracker::sync_orders_once(&state.db, &state.config, &state.accesstrade).await;
    if summary.errors.is_empty() {
        redirect_with_notice(&format!(
            "Đối soát xong: Quét {} đơn trong 7 ngày qua, phát hiện {} đơn mới!",
            summary.fetched, summary.newly_inserted
        ))
    } else {
        redirect_with_notice(&format!(
            "Quét {} đơn (mới {}). Lưu ý: {}",
            summary.fetched,
            summary.newly_inserted,
            summary.errors.join("; ")
        ))
    }
}

fn redirect_with_notice(msg: &str) -> Response {
    let encoded: String = url::form_urlencoded::byte_serialize(msg.as_bytes()).collect();
    Redirect::to(&format!("/admin?notice={}", encoded)).into_response()
}

pub(super) async fn handle_sync_vouchers(headers: HeaderMap, State(state): State<AppState>) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    let Ok(conn) = state.db.conn() else {
        return redirect_with_notice("Lỗi kết nối cơ sở dữ liệu.");
    };

    let Some(token) = resolve_accesstrade_token(&conn, &state.config).await else {
        return redirect_with_notice("Chưa cấu hình ACCESSTRADE API Token.");
    };

    match state.accesstrade.fetch_high_discount_vouchers(&token, 0, true).await {
        Ok(list) => redirect_with_notice(&format!(
            "Đã đồng bộ thành công tổng cộng {} mã giảm giá toàn thị trường từ ACCESSTRADE (không bỏ sót mã nào)!",
            list.len()
        )),
        Err(err) => redirect_with_notice(&format!("Lỗi đồng bộ kho mã: {}", err)),
    }
}

#[derive(serde::Deserialize)]
pub struct AddDealForm {
    pub platform: String,
    pub title: String,
    pub coupon_code: String,
    pub discount_desc: String,
    pub deal_url: String,
    pub min_spend: Option<f64>,
    pub is_exclusive: Option<String>,
}

fn is_valid_http_url(u: &str) -> bool {
    let trimmed = u.trim();
    trimmed.starts_with("https://") || trimmed.starts_with("http://")
}

pub(super) async fn handle_add_deal(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<AddDealForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if !is_valid_http_url(&form.deal_url) {
        return redirect_with_notice("Link áp mã phải bắt đầu bằng https:// hoặc http://");
    }

    let Ok(conn) = state.db.conn() else {
        return redirect_with_notice("Lỗi kết nối cơ sở dữ liệu.");
    };

    let code_clean = form.coupon_code.trim();
    let item = NewDealInput {
        platform: form.platform.trim().to_string(),
        title: form.title.trim().to_string(),
        coupon_code: if code_clean.is_empty() {
            None
        } else {
            Some(code_clean.to_string())
        },
        discount_desc: form.discount_desc.trim().to_string(),
        min_spend: form.min_spend.unwrap_or(0.0).max(0.0),
        deal_url: form.deal_url.trim().to_string(),
        expire_at: None,
        is_exclusive: form.is_exclusive.is_some(),
    };

    match Repository::add_deal(&conn, &item).await {
        Ok(inserted) => {
            state.db.invalidate_deals_cache().await;
            info!(
                "👑 Admin đã {} mã '{}' ({})",
                if inserted { "thêm mới" } else { "cập nhật" },
                sanitize_crlf(&item.title),
                sanitize_crlf(&item.platform)
            );
            redirect_with_notice(if inserted {
                "Đã lưu Mã Độc Quyền mới vào kho Bot!"
            } else {
                "Đã cập nhật lại Mã Độc Quyền trùng code trong kho!"
            })
        }
        Err(err) => redirect_with_notice(&format!("Lỗi lưu mã: {}", err)),
    }
}

#[derive(serde::Deserialize)]
pub struct BulkImportDealsForm {
    pub bulk_lines: String,
}

pub(super) async fn handle_import_deals(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<BulkImportDealsForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    let (items, skipped) = parse_bulk_exclusive_deals(&form.bulk_lines);
    if items.is_empty() {
        return redirect_with_notice(
            "Không có dòng nào đúng định dạng (Sàn | Tên | Mã Code | Mức giảm | Link https://... | Đơn tối thiểu).",
        );
    }

    let Ok(conn) = state.db.conn() else {
        return redirect_with_notice("Lỗi kết nối cơ sở dữ liệu.");
    };

    match Repository::add_deals_batch(&conn, &items).await {
        Ok((inserted, updated)) => {
            state.db.invalidate_deals_cache().await;
            info!(
                "📥 Admin nhập hàng loạt mã độc quyền: +{} mới, ~{} cập nhật, bỏ qua {} dòng sai định dạng.",
                inserted, updated, skipped
            );
            redirect_with_notice(&format!(
                "Nhập xong: +{} mã mới, cập nhật {} mã (bỏ qua {} dòng sai định dạng).",
                inserted, updated, skipped
            ))
        }
        Err(err) => redirect_with_notice(&format!("Lỗi nhập hàng loạt: {}", err)),
    }
}

#[derive(serde::Deserialize)]
pub struct DealIdForm {
    pub deal_id: i64,
}

pub(super) async fn handle_toggle_deal(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<DealIdForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if let Ok(conn) = state.db.conn() {
        let _ = Repository::toggle_deal(&conn, form.deal_id).await;
        state.db.invalidate_deals_cache().await;
    }

    Redirect::to("/admin").into_response()
}

pub(super) async fn handle_delete_deal(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<DealIdForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if let Ok(conn) = state.db.conn() {
        let _ = Repository::delete_deal(&conn, form.deal_id).await;
        state.db.invalidate_deals_cache().await;
    }

    Redirect::to("/admin").into_response()
}

/// Phân tích văn bản nhập hàng loạt (hỗ trợ dấu `|` hoặc Tab từ Excel/Google Sheets)
pub fn parse_bulk_exclusive_deals(raw_text: &str) -> (Vec<NewDealInput>, usize) {
    let mut items = Vec::new();
    let mut skipped = 0usize;

    for raw_line in raw_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let sep = if line.contains('\t') { '\t' } else { '|' };
        let cols: Vec<&str> = line.split(sep).map(str::trim).collect();
        if cols.len() < 5 {
            skipped += 1;
            continue;
        }

        let platform = cols[0];
        let title = cols[1];
        let code = cols[2];
        let desc = cols[3];
        let url = cols[4];
        let min_spend = cols
            .get(5)
            .and_then(|s| s.replace([',', '.'], "").parse::<f64>().ok())
            .unwrap_or(0.0);

        if platform.is_empty() || title.is_empty() || desc.is_empty() || !is_valid_http_url(url) {
            skipped += 1;
            continue;
        }

        items.push(NewDealInput {
            platform: platform.to_string(),
            title: title.to_string(),
            coupon_code: if code.is_empty() {
                None
            } else {
                Some(code.to_string())
            },
            discount_desc: desc.to_string(),
            min_spend: min_spend.max(0.0),
            deal_url: url.to_string(),
            expire_at: None,
            is_exclusive: true,
        });
    }

    (items, skipped)
}

/// Form cập nhật cấu hình link Affiliate dự phòng
#[derive(serde::Deserialize)]
pub struct AffiliateSettingsForm {
    pub template: String,
}

/// Xử lý cập nhật mẫu link Affiliate
pub(super) async fn handle_update_affiliate_settings(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<AffiliateSettingsForm>,
) -> Response {
    if !is_authenticated(&state, &headers).await {
        return Redirect::to("/admin/login").into_response();
    }

    if let Ok(conn) = state.db.conn() {
        let _ = Repository::set_setting(&conn, "affiliate_template", form.template.trim()).await;
        state.db.invalidate_settings_cache().await;
        info!("✅ Đã cập nhật mẫu link Affiliate dự phòng trên Web Admin.");
    }

    Redirect::to("/admin").into_response()
}

