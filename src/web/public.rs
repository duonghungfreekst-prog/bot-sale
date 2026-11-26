use super::*;

pub(super) const CLOUDFLARE_WORKER_TEMPLATE: &str = include_str!("../../cloudflare_worker.js");

#[derive(serde::Deserialize, Default)]
pub struct PublicHomeQuery {
    pub q: Option<String>,
    pub plat: Option<String>,
}

enum PublicDealCard<'a> {
    Db(&'a crate::db::models::DealItem),
    Api(&'a VoucherOffer),
}

#[derive(serde::Deserialize, Default)]
pub struct PublicConvertForm {
    pub url: String,
}

/// Trang chủ công khai tổng hợp mã giảm giá & gợi ý sản phẩm (phục vụ khách hàng & xét duyệt đối tác)
pub(super) async fn handle_root(
    Query(query): Query<PublicHomeQuery>,
    State(state): State<AppState>,
) -> Response {
    if is_public_home_cloaked(&state).await {
        return not_found_cloak_response();
    }
    render_public_home(&state, query, "", "").await
}

/// Đổi link sản phẩm trực tiếp trên Trang chủ Web `/` (không cần Telegram)
pub(super) async fn handle_public_convert(
    headers: HeaderMap,
    State(state): State<AppState>,
    Form(form): Form<PublicConvertForm>,
) -> Response {
    if is_public_home_cloaked(&state).await {
        return not_found_cloak_response();
    }
    let input_raw = form.url.trim().to_string();
    let client_ip = extract_client_ip(&headers);

    // Chống spam endpoint /convert theo IP (tối đa 20 lượt / 60 giây)
    {
        let now_ts = Utc::now().timestamp();
        let mut limiter = state.convert_limiter.write().await;
        prune_rate_limiter_map(&mut limiter, now_ts, 60, 4000);
        let entry = limiter.entry(client_ip).or_insert((0, now_ts));
        if now_ts - entry.1 >= 60 {
            *entry = (1, now_ts);
        } else if entry.0 >= 20 {
            let err_html = r##"<div class="convert-alert convert-alert--error" role="alert"><svg class="ico" aria-hidden="true"><use href="#ico-clock"/></svg><span>Bạn đang gửi quá nhanh. Vui lòng chờ khoảng 1 phút rồi thử đổi link tiếp nhé!</span></div>"##;
            return render_public_home(&state, PublicHomeQuery::default(), &input_raw, err_html)
                .await;
        } else {
            entry.0 += 1;
        }
    }

    if state.parser.extract_first_url(&input_raw).is_none() {
        let err_html = r##"<div class="convert-alert convert-alert--error" role="alert"><svg class="ico" aria-hidden="true"><use href="#ico-warn"/></svg><span>Vui lòng dán đúng đường link sản phẩm bắt đầu bằng <code>https://</code> từ Shopee, Tiki, Lazada, TikTok Shop hoặc Alibaba.</span></div>"##;
        return render_public_home(&state, PublicHomeQuery::default(), &input_raw, err_html).await;
    }

    let Ok(conn) = state.db.conn() else {
        let err_html = r##"<div class="convert-alert convert-alert--error" role="alert"><svg class="ico" aria-hidden="true"><use href="#ico-warn"/></svg><span>Hệ thống cơ sở dữ liệu tạm bận, vui lòng thử lại sau ít giây.</span></div>"##;
        return render_public_home(&state, PublicHomeQuery::default(), &input_raw, err_html).await;
    };

    // Đảm bảo tài khoản khách Web (user_id = 0) tồn tại để thỏa khóa ngoại bảng clicks
    let _ = Repository::get_or_create_user(&conn, 0, Some("web_guest"), "Khách Web Thế Giới Siêu Sale", None)
        .await;
    let owned_aff = load_owned_aff_settings_cached(&state.db, &state.config).await;
    let _ = state
        .accesstrade
        .get_or_load_approved_campaigns(owned_aff.accesstrade_token.as_deref())
        .await;

    match state
        .parser
        .process_link(&input_raw, 0, None, owned_aff.as_borrowed())
        .await
    {
        Ok(parsed) => {
            let click_record = ClickRecord {
                id: parsed.click_id.clone(),
                user_id: 0,
                platform: parsed.platform.as_str().to_string(),
                original_url: parsed.original_url.clone(),
                clean_url: parsed.clean_url.clone(),
                affiliate_url: parsed.affiliate_url.clone(),
                sub_id: parsed.sub_id.clone(),
                created_at: Utc::now().to_rfc3339(),
            };
            let _ = Repository::record_click(&conn, &click_record).await;

            let wrapped_main = state
                .db
                .wrap_url_cached(
                    owned_aff.shortlink_base.as_deref(),
                    &parsed.affiliate_url,
                    &parsed.product_title,
                    parsed.platform.as_str(),
                )
                .await;

            let mut meta_parts = Vec::new();
            if let Some(ref shop) = parsed.product_shop {
                meta_parts.push(format!("Shop: <strong>{}</strong>", escape_html(shop)));
            }
            if let Some(price) = parsed.product_price_vnd.filter(|&p| p > 0.0) {
                meta_parts.push(format!("Giá tham khảo: <strong>{}</strong>", escape_html(&format_vnd(price))));
            }
            let meta_line = if meta_parts.is_empty() {
                String::new()
            } else {
                format!(r#"<p class="convert-result__meta">{}</p>"#, meta_parts.join(" · "))
            };

            let mut matched_btns = String::new();
            let mut seen_btn_urls: std::collections::HashSet<String> = std::collections::HashSet::new();
            if let Ok(deals) = state
                .db
                .get_active_deals_cached(Some(parsed.platform.as_str()))
                .await
            {
                for d in crate::bot::handlers::select_top_relevant_deals(&deals, &parsed, 2) {
                    let enriched_deal_url = crate::bot::handlers::enrich_deal_url_for_platform(
                        &d.deal_url,
                        &d.platform,
                        owned_aff.shopee_affiliate_id.as_deref(),
                        owned_aff.shopee_social_params.as_deref(),
                        &parsed.sub_id,
                    );
                    if !seen_btn_urls.insert(enriched_deal_url.clone()) {
                        continue;
                    }
                    let wrapped_ex = state
                        .db
                        .wrap_url_cached(
                            owned_aff.shortlink_base.as_deref(),
                            &enriched_deal_url,
                            &d.title,
                            &d.platform,
                        )
                        .await;
                    let short_label: String = if d.discount_desc.chars().count() > 38 {
                        format!("{}...", d.discount_desc.chars().take(36).collect::<String>())
                    } else {
                        d.discount_desc.clone()
                    };
                    let code_label = d
                        .coupon_code
                        .as_deref()
                        .map(str::trim)
                        .filter(|c| !crate::services::social_harvester::is_synthetic_tracking_code(c))
                        .map(|c| format!(" · {}", c))
                        .unwrap_or_default();
                    matched_btns.push_str(&format!(
                        r##"<a class="btn btn--secondary" href="{}" target="_blank" rel="noopener noreferrer"><svg class="ico" aria-hidden="true"><use href="#ico-crown"/></svg><span>{}{}</span><svg class="ico" aria-hidden="true"><use href="#ico-external"/></svg></a>"##,
                        escape_html(&wrapped_ex),
                        escape_html(&short_label),
                        escape_html(&code_label)
                    ));
                }
            }

            let approved_cached = state.accesstrade.peek_approved_campaigns().await;
            let matched_market = state
                .accesstrade
                .search_matching_vouchers_for_deal(&VoucherMatchQuery {
                    platform: parsed.platform.as_str(),
                    product_title: &parsed.product_title,
                    clean_url: &parsed.clean_url,
                    shop_name: parsed.product_shop.as_deref(),
                    brand_name: parsed.product_brand.as_deref(),
                    price_vnd: parsed.product_price_vnd,
                    max_items: 2,
                })
                .await;
            for mv in matched_market {
                let raw_v_url = state
                    .accesstrade
                    .verified_offer_url(&mv, approved_cached.as_ref())
                    .await;
                let enriched_v_url = crate::bot::handlers::enrich_deal_url_for_platform(
                    &raw_v_url,
                    &mv.platform,
                    owned_aff.shopee_affiliate_id.as_deref(),
                    owned_aff.shopee_social_params.as_deref(),
                    &parsed.sub_id,
                );
                let wrapped_v = state
                    .db
                    .wrap_url_cached(
                        owned_aff.shortlink_base.as_deref(),
                        &enriched_v_url,
                        &mv.shop_name,
                        &mv.platform,
                    )
                    .await;
                matched_btns.push_str(&format!(
                    r##"<a class="btn btn--secondary" href="{}" target="_blank" rel="noopener noreferrer"><svg class="ico" aria-hidden="true"><use href="#ico-ticket"/></svg><span>Mã {} ({})</span><svg class="ico" aria-hidden="true"><use href="#ico-external"/></svg></a>"##,
                    escape_html(&wrapped_v),
                    escape_html(&mv.coupon_code),
                    escape_html(&mv.discount_badge())
                ));
            }

            let aff_badge_text = if owned_aff.shortlink_base.is_some() {
                "Đã gắn link bọc ưu đãi"
            } else {
                "Đã gắn link ưu đãi trực tiếp"
            };
            let plat_dot_color = platform_brand_color(parsed.platform.as_str());

            let result_html = format!(
                r##"<div class="convert-result" aria-live="polite">
                    <div class="convert-result__head">
                        <span class="badge badge--platform"><span class="plat-dot" style="background:{};" aria-hidden="true"></span><span>{}</span></span>
                        <span class="badge badge--accent">{}</span>
                    </div>
                    <h2 class="convert-result__title">{}</h2>
                    {}
                    <div class="convert-result__url"><code>{}</code></div>
                    <div class="convert-actions">
                        <a class="btn btn--primary" href="{}" target="_blank" rel="noopener noreferrer"><svg class="ico" aria-hidden="true"><use href="#ico-rocket"/></svg><span>Mở Ứng Dụng &amp; Áp Mã Ngay</span><svg class="ico" aria-hidden="true"><use href="#ico-external"/></svg></a>
                        <button type="button" class="btn btn--secondary js-copy-link" data-copy-url="{}"><svg class="ico" aria-hidden="true"><use href="#ico-copy"/></svg><span class="copy-lbl">Sao chép link ưu đãi</span></button>
                        {}
                    </div>
                </div>"##,
                plat_dot_color,
                escape_html(parsed.platform.as_str()),
                aff_badge_text,
                escape_html(&parsed.product_title),
                meta_line,
                escape_html(&wrapped_main),
                escape_html(&wrapped_main),
                escape_html(&wrapped_main),
                matched_btns
            );

            render_public_home(&state, PublicHomeQuery::default(), &input_raw, &result_html).await
        }
        Err(err) => {
            let err_html = format!(
                r##"<div class="convert-alert convert-alert--error" role="alert"><svg class="ico" aria-hidden="true"><use href="#ico-warn"/></svg><span>Không thể phân tích link này: {}</span></div>"##,
                escape_html(&err.to_string())
            );
            render_public_home(&state, PublicHomeQuery::default(), &input_raw, &err_html).await
        }
    }
}

/// JS ngoài cho Trang Chủ & nút sao chép trong Web Admin (tuân thủ nghiêm ngặt CSP `default-src 'self'`)
pub(super) async fn handle_home_js() -> Response {
    let js = r##"function syncPublicThemeBtn() {
    var themeBtn = document.getElementById("public-theme-btn");
    if (!themeBtn) return;
    var isDark = document.documentElement.getAttribute("data-theme") === "dark";
    var icoUse = document.getElementById("public-theme-ico-use");
    var lbl = document.getElementById("public-theme-text");
    if (icoUse && lbl) {
        icoUse.setAttribute("href", isDark ? "#ico-sun" : "#ico-moon");
        lbl.textContent = isDark ? "Giao diện Sáng" : "Giao diện Tối";
    } else {
        themeBtn.textContent = isDark ? "Giao diện Sáng" : "Giao diện Tối";
    }
    themeBtn.setAttribute("aria-pressed", isDark ? "true" : "false");
}
document.addEventListener("DOMContentLoaded", syncPublicThemeBtn);
document.addEventListener("click", function (e) {
    var tBtn = e.target.closest("#public-theme-btn");
    if (tBtn) {
        var isDark = document.documentElement.getAttribute("data-theme") === "dark";
        var next = isDark ? "light" : "dark";
        if (next === "dark") {
            document.documentElement.setAttribute("data-theme", "dark");
        } else {
            document.documentElement.removeAttribute("data-theme");
        }
        try { localStorage.setItem("dealx-theme", next); } catch (_) {}
        syncPublicThemeBtn();
        return;
    }
    var btn = e.target.closest(".js-copy-link");
    if (!btn) return;
    var url = btn.getAttribute("data-copy-url");
    var fromTarget = false;
    if (!url) {
        var targetId = btn.getAttribute("data-copy-target");
        if (targetId) {
            var el = document.getElementById(targetId);
            if (el) {
                url = typeof el.value === "string" ? el.value : el.textContent;
                fromTarget = true;
            }
        }
    }
    if (!url) return;
    if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(url).then(function () {
            var lbl = btn.querySelector(".copy-lbl");
            if (lbl) {
                var old = lbl.textContent;
                lbl.textContent = btn.classList.contains("copy-code-btn") ? "Đã chép" : (fromTarget ? "Đã sao chép code" : "Đã sao chép link");
                btn.classList.add("is-copied");
                setTimeout(function () {
                    lbl.textContent = old;
                    btn.classList.remove("is-copied");
                }, 2000);
            } else {
                var oldText = btn.textContent;
                btn.textContent = fromTarget ? "✅ Đã sao chép code" : "✅ Đã sao chép link";
                setTimeout(function () { btn.textContent = oldText; }, 2000);
            }
        });
    }
});"##;
    let mut resp = (StatusCode::OK, js).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300"),
    );
    resp
}

const BOT_LOGO_WEBP: &[u8] = include_bytes!("../../assets/bot-logo.webp");
const BOT_CART_3D_WEBP: &[u8] = include_bytes!("../../assets/bot-cart-3d.webp");

fn serve_static_asset(body: &'static [u8], content_type: &'static str, max_age: u32) -> Response {
    let mut resp = (StatusCode::OK, body).into_response();
    resp.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(cache_val) = HeaderValue::from_str(&format!("public, max-age={}", max_age)) {
        resp.headers_mut().insert(header::CACHE_CONTROL, cache_val);
    }
    resp
}

/// Logo chính thức của Bot Thế Giới Siêu Sale (WebP tối ưu dung lượng)
pub(super) async fn handle_bot_logo_webp() -> Response {
    serve_static_asset(BOT_LOGO_WEBP, "image/webp", 86400)
}

/// Ảnh minh họa 3D Bot đẩy xe hàng chứa kho mã giảm giá (WebP)
pub(super) async fn handle_bot_cart_3d_webp() -> Response {
    serve_static_asset(BOT_CART_3D_WEBP, "image/webp", 86400)
}

/// Script ngoài tạo hiệu ứng xoay 3D & ánh sáng phản chiếu theo chuột cho Trang Đích Bot (CSP `script-src 'self'`)
pub(super) async fn handle_landing_js() -> Response {
    let js = r#"(function () {
    if (window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    var stage = document.getElementById("stage3d");
    if (!stage) return;
    stage.addEventListener("pointermove", function (e) {
        if (e.pointerType === "touch") return;
        var r = stage.getBoundingClientRect();
        if (!r.width || !r.height) return;
        var px = (e.clientX - r.left) / r.width;
        var py = (e.clientY - r.top) / r.height;
        stage.classList.add("is-interacting");
        stage.style.setProperty("--rx", ((0.5 - py) * 12).toFixed(2) + "deg");
        stage.style.setProperty("--ry", ((px - 0.5) * 14).toFixed(2) + "deg");
        stage.style.setProperty("--gx", (px * 100).toFixed(1) + "%");
        stage.style.setProperty("--gy", (py * 100).toFixed(1) + "%");
    });
    stage.addEventListener("pointerleave", function () {
        stage.classList.remove("is-interacting");
        stage.style.setProperty("--rx", "0deg");
        stage.style.setProperty("--ry", "0deg");
        stage.style.setProperty("--gx", "50%");
        stage.style.setProperty("--gy", "45%");
    });
})();"#;
    serve_static_asset(js.as_bytes(), "application/javascript; charset=utf-8", 3600)
}

fn platform_brand_color(platform: &str) -> &'static str {
    let p = platform.trim().to_ascii_lowercase();
    if p.contains("shopee") {
        "#EA580C"
    } else if p.contains("lazada") {
        "#DB2777"
    } else if p.contains("tiki") {
        "#0284C7"
    } else if p.contains("tiktok") {
        "#E11D48"
    } else if p.contains("alibaba") {
        "#D97706"
    } else {
        "#64748B"
    }
}

const PUBLIC_SVG_SPRITE: &str = r#"<svg aria-hidden="true" style="position:absolute;width:0;height:0;overflow:hidden">
    <defs>
        <symbol id="ico-sun" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/></symbol>
        <symbol id="ico-moon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/></symbol>
        <symbol id="ico-shield" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="m9 12 2 2 4-4"/></symbol>
        <symbol id="ico-ticket" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 9a3 3 0 0 1 0 6v2a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-2a3 3 0 0 1 0-6V7a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2Z"/><path d="M13 5v2"/><path d="M13 17v2"/><path d="M13 11v2"/></symbol>
        <symbol id="ico-clock" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><polyline points="12 7 12 12 15.5 14"/></symbol>
        <symbol id="ico-zap" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></symbol>
        <symbol id="ico-check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></symbol>
        <symbol id="ico-copy" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></symbol>
        <symbol id="ico-external" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></symbol>
        <symbol id="ico-crown" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m2 4 3 12h14l3-12-6 7-4-7-4 7-6-7zm3 16h14"/></symbol>
        <symbol id="ico-rocket" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z"/><path d="m12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z"/></symbol>
        <symbol id="ico-warn" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></symbol>
    </defs>
</svg>"#;

struct PublicDealCardData<'a> {
    platform: &'a str,
    badge: &'a str,
    title: &'a str,
    desc: &'a str,
    code_txt: &'a str,
    note_txt: &'a str,
    wrapped_url: &'a str,
    cta_label: &'a str,
}

fn render_public_deal_card(card: PublicDealCardData<'_>) -> String {
    let dot_color = platform_brand_color(card.platform);
    let trimmed_code = card.code_txt.trim();
    let has_manual_code = !crate::services::social_harvester::is_synthetic_tracking_code(trimmed_code)
        && !trimmed_code.eq_ignore_ascii_case("Tự động áp dụng");
    let coupon_row_html = if has_manual_code {
        format!(
            r##"<span class="coupon-pill">Mã: <code>{}</code></span>
                <button type="button" class="copy-code-btn js-copy-link" data-copy-url="{}" aria-label="Sao chép mã {}"><svg class="ico" style="width:13px;height:13px;" aria-hidden="true"><use href="#ico-copy"/></svg><span class="copy-lbl">Chép mã</span></button>"##,
            escape_html(trimmed_code),
            escape_html(trimmed_code),
            escape_html(trimmed_code)
        )
    } else {
        r##"<span class="coupon-pill coupon-pill--link"><svg class="ico" style="width:13px;height:13px;" aria-hidden="true"><use href="#ico-zap"/></svg><span>Áp tự động qua link (Không cần gõ mã)</span></span>"##.to_string()
    };
    let final_cta_label = if has_manual_code {
        card.cta_label.to_string()
    } else {
        format!("Lưu & Áp ưu đãi {}", card.platform)
    };
    format!(
        r##"<article class="deal-card" style="--plat-accent:{};">
            <div class="deal-card__top">
                <span class="badge badge--platform"><span class="plat-dot" style="background:{};" aria-hidden="true"></span><span>{}</span></span>
                <span class="badge badge--accent">{}</span>
            </div>
            <h3 class="deal-card__title">{}</h3>
            <p class="deal-card__desc">{}</p>
            <div class="deal-card__meta">
                <div class="coupon-row">
                    {}
                </div>
                <span class="deal-card__note">{}</span>
            </div>
            <a class="btn btn--primary deal-card__cta" href="{}" target="_blank" rel="noopener noreferrer">
                <span>{}</span>
                <svg class="ico" style="width:15px;height:15px;" aria-hidden="true"><use href="#ico-external"/></svg>
            </a>
        </article>"##,
        dot_color,
        dot_color,
        escape_html(card.platform),
        escape_html(card.badge),
        escape_html(card.title),
        escape_html(card.desc),
        coupon_row_html,
        escape_html(card.note_txt),
        escape_html(card.wrapped_url),
        escape_html(&final_cta_label)
    )
}

pub(super) async fn render_public_home(
    state: &AppState,
    query: PublicHomeQuery,
    convert_input_val: &str,
    convert_result_html: &str,
) -> Response {
    let search_q = query.q.unwrap_or_default().trim().to_string();
    let filter_plat = query
        .plat
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("all")
        .to_string();

    let db_deals = state.db.get_active_deals_cached(None).await.unwrap_or_default();
    let settings_map = state.db.get_settings_map_cached().await.unwrap_or_default();
    let read_setting = |key: &str| {
        settings_map
            .get(key)
            .cloned()
            .filter(|s| !s.trim().is_empty())
    };
    let shortlink_db = read_setting("shortlink_base_url");
    let shopee_aff_id = read_setting("shopee_affiliate_id")
        .or_else(|| state.config.shopee_affiliate_id.clone());
    let shopee_social_params = read_setting("shopee_social_params")
        .or_else(|| state.config.shopee_social_params.clone());
    let tunnel_snap = crate::services::tunnel::snapshot_tunnel_state();
    let shortlink_base = crate::config::resolve_pc_runtime_shortlink_base_url(
        shortlink_db.as_deref(),
        state.config.shortlink_base_url.as_deref(),
        state.config.public_base_url.as_deref(),
        tunnel_snap.quick_tunnel_url.as_deref(),
        tunnel_snap.has_named_token,
    );
    let market_vouchers = state.accesstrade.peek_cached_high_discount_vouchers().await;
    let approved = state.accesstrade.peek_approved_campaigns().await;
    let sync_meta = state.accesstrade.get_voucher_sync_meta().await;

    let clean_db_deals: Vec<&crate::db::models::DealItem> = db_deals
        .iter()
        .filter(|d| {
            !crate::services::social_harvester::is_guide_noise_deal_text(&d.discount_desc)
                && !crate::services::social_harvester::is_guide_noise_deal_text(&d.title)
        })
        .collect();

    let q_lower = search_q.to_lowercase();
    let mut cards: Vec<PublicDealCard<'_>> = Vec::new();
    let mut seen_urls: std::collections::HashSet<String> = std::collections::HashSet::new();

    for &d in &clean_db_deals {
        let plat_ok = filter_plat == "all" || d.platform.eq_ignore_ascii_case(&filter_plat);
        let q_ok = q_lower.is_empty()
            || d.title.to_lowercase().contains(&q_lower)
            || d.discount_desc.to_lowercase().contains(&q_lower)
            || d.coupon_code.as_deref().unwrap_or("").to_lowercase().contains(&q_lower)
            || d.platform.to_lowercase().contains(&q_lower);
        if plat_ok && q_ok {
            let norm = d.deal_url.trim().trim_end_matches('/').to_ascii_lowercase();
            if norm.is_empty() || seen_urls.insert(norm) {
                cards.push(PublicDealCard::Db(d));
            }
        }
    }

    for v in &market_vouchers {
        let plat_ok = filter_plat == "all" || v.platform.eq_ignore_ascii_case(&filter_plat);
        let q_ok = q_lower.is_empty()
            || v.shop_name.to_lowercase().contains(&q_lower)
            || v.title.to_lowercase().contains(&q_lower)
            || v.coupon_code.to_lowercase().contains(&q_lower)
            || v.discount_desc.to_lowercase().contains(&q_lower)
            || v.platform.to_lowercase().contains(&q_lower);
        if plat_ok && q_ok {
            let norm = v
                .best_target_url(approved.as_ref())
                .trim()
                .trim_end_matches('/')
                .to_ascii_lowercase();
            if norm.is_empty() || seen_urls.insert(norm) {
                cards.push(PublicDealCard::Api(v));
            }
        }
    }

    let total_available = clean_db_deals.len() + market_vouchers.len();
    let matched_count = cards.len();
    let last_sync_text = if sync_meta.last_sync_vn.is_empty() {
        "Đang đồng bộ thời gian thực".to_string()
    } else {
        sync_meta.last_sync_vn
    };

    struct PreparedPublicCard {
        platform: String,
        badge: String,
        title: String,
        desc: String,
        code_txt: String,
        note_txt: String,
        enriched_url: String,
        cta_label: &'static str,
    }

    let prepared_cards: Vec<PreparedPublicCard> = cards
        .into_iter()
        .take(24)
        .map(|card| match card {
            PublicDealCard::Db(d) => {
                let code_txt = d
                    .coupon_code
                    .as_deref()
                    .filter(|c| !c.trim().is_empty())
                    .unwrap_or("Tự động áp dụng")
                    .to_string();
                let badge = if d.is_exclusive {
                    "Ưu đãi nổi bật"
                } else {
                    "Mã giảm giá"
                }
                .to_string();
                let note_txt = if d.min_spend > 0.0 {
                    format!("Đơn từ {}", format_vnd(d.min_spend))
                } else {
                    "Không giới hạn đơn tối thiểu".to_string()
                };
                let enriched_url = crate::bot::handlers::enrich_deal_url_for_platform(
                    &d.deal_url,
                    &d.platform,
                    shopee_aff_id.as_deref(),
                    shopee_social_params.as_deref(),
                    "web_public_deal",
                );
                PreparedPublicCard {
                    platform: d.platform.clone(),
                    badge,
                    title: d.title.clone(),
                    desc: d.discount_desc.clone(),
                    code_txt,
                    note_txt,
                    enriched_url,
                    cta_label: "Lấy mã & Xem ưu đãi",
                }
            }
            PublicDealCard::Api(v) => {
                let target_url = v.best_target_url(approved.as_ref());
                let enriched_url = crate::bot::handlers::enrich_deal_url_for_platform(
                    &target_url,
                    &v.platform,
                    shopee_aff_id.as_deref(),
                    shopee_social_params.as_deref(),
                    "web_public_voucher",
                );
                let note_txt = if v.end_time.trim().is_empty() {
                    "Đang hiệu lực".to_string()
                } else {
                    format!("HSD: {}", v.end_time.trim())
                };
                PreparedPublicCard {
                    platform: v.platform.clone(),
                    badge: v.discount_badge(),
                    title: v.shop_name.clone(),
                    desc: v.discount_desc.clone(),
                    code_txt: v.coupon_code.clone(),
                    note_txt,
                    enriched_url,
                    cta_label: "Lấy mã & Mở gian hàng",
                }
            }
        })
        .collect();

    let batch_targets: Vec<(&str, &str, &str)> = prepared_cards
        .iter()
        .map(|c| (c.enriched_url.as_str(), c.title.as_str(), c.platform.as_str()))
        .collect();
    let wrapped_urls = state
        .db
        .wrap_urls_batch_cached(shortlink_base.as_deref(), &batch_targets)
        .await;

    let mut cards_html = String::new();
    for (c, wrapped_url) in prepared_cards.iter().zip(wrapped_urls.iter()) {
        cards_html.push_str(&render_public_deal_card(PublicDealCardData {
            platform: &c.platform,
            badge: &c.badge,
            title: &c.title,
            desc: &c.desc,
            code_txt: &c.code_txt,
            note_txt: &c.note_txt,
            wrapped_url,
            cta_label: c.cta_label,
        }));
    }

    if cards_html.is_empty() {
        let empty_msg = if total_available == 0 {
            "Hệ thống đang đồng bộ danh sách ưu đãi mới nhất từ đối tác. Bạn có thể quay lại sau ít phút hoặc gửi trực tiếp link sản phẩm qua trợ lý Telegram để kiểm tra nhanh."
        } else {
            "Không tìm thấy ưu đãi nào khớp với từ khóa hoặc sàn bạn vừa chọn. Hãy thử xóa bộ lọc để xem toàn bộ ưu đãi đang hoạt động."
        };
        cards_html = format!(
            r#"<div class="empty-state">
                <p>{}</p>
                <a class="btn btn--secondary" href="/">Xem toàn bộ ưu đãi</a>
            </div>"#,
            empty_msg
        );
    }

    let filter_links = [
        ("all", "Tất cả sàn"),
        ("Shopee", "Shopee"),
        ("Tiki", "Tiki"),
        ("Lazada", "Lazada"),
        ("TikTok Shop", "TikTok Shop"),
        ("Alibaba", "Alibaba"),
    ]
    .into_iter()
    .map(|(val, label)| {
        let active_cls = if filter_plat.eq_ignore_ascii_case(val) {
            "filter-pill filter-pill--active"
        } else {
            "filter-pill"
        };
        let href = if search_q.is_empty() {
            format!("/?plat={}", urlencoding_simple(val))
        } else {
            format!(
                "/?q={}&plat={}",
                urlencoding_simple(&search_q),
                urlencoding_simple(val)
            )
        };
        let dot_html = if val == "all" {
            String::new()
        } else {
            format!(
                r#"<span class="plat-dot" style="background:{};" aria-hidden="true"></span>"#,
                platform_brand_color(val)
            )
        };
        format!(r#"<a class="{}" href="{}">{}{}</a>"#, active_cls, href, dot_html, label)
    })
    .collect::<Vec<_>>()
    .join("");

    let html = format!(
        r##"<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Thế Giới Siêu Sale — Tổng Hợp Mã Giảm Giá &amp; Gợi Ý Sản Phẩm Chính Hãng</title>
    <meta name="description" content="Cổng tổng hợp mã giảm giá công khai theo từng ngành hàng từ Shopee, Tiki, Lazada, TikTok Shop giúp người mua tiết kiệm mỗi ngày.">
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Be+Vietnam+Pro:wght@400;500;600;700;800&amp;family=Lexend:wght@600;700;800&amp;family=JetBrains+Mono:wght@500;600&amp;display=swap">
    <script>
        (function () {{
            try {{
                if (localStorage.getItem("dealx-theme") === "dark") {{
                    document.documentElement.setAttribute("data-theme", "dark");
                }}
            }} catch (e) {{}}
        }})();
    </script>
    <style>
        :root {{
            color-scheme: light;
            --color-bg: #F4F6F9;
            --color-header: rgba(255, 255, 255, 0.92);
            --color-surface: #FFFFFF;
            --color-surface-elevated: #F8FAFC;
            --color-inset: #F1F5F9;
            --color-text: #0F172A;
            --color-text-secondary: #334155;
            --color-text-muted: #475569;
            --color-primary: #C2410C;
            --color-primary-hover: #9A3412;
            --color-primary-bright: #EA580C;
            --color-primary-contrast: #FFFFFF;
            --color-primary-soft: #FFF7ED;
            --color-primary-soft-border: #FED7AA;
            --color-border: #E2E8F0;
            --color-border-strong: #CBD5E1;
            --color-success: #15803D;
            --color-success-bg: #DCFCE7;
            --color-success-border: #86EFAC;
            --color-err: #B91C1C;
            --color-err-bg: #FEE2E2;
            --color-err-border: #FCA5A5;
            --radius-sm: 8px;
            --radius-md: 12px;
            --radius-lg: 18px;
            --shadow-sm: 0 1px 2px rgba(15, 23, 42, 0.05);
            --shadow-md: 0 10px 28px -8px rgba(15, 23, 42, 0.1), 0 2px 6px rgba(15, 23, 42, 0.04);
            --shadow-hero: 0 24px 48px -14px rgba(194, 65, 12, 0.18), 0 8px 20px rgba(15, 23, 42, 0.06);
            --font-sans: "Be Vietnam Pro", system-ui, -apple-system, sans-serif;
            --font-display: "Lexend", "Be Vietnam Pro", system-ui, sans-serif;
            --font-mono: "JetBrains Mono", monospace;
        }}
        html[data-theme="dark"] {{
            color-scheme: dark;
            --color-bg: #0B1120;
            --color-header: rgba(15, 23, 42, 0.92);
            --color-surface: #131C31;
            --color-surface-elevated: #18233C;
            --color-inset: #0D1526;
            --color-text: #F8FAFC;
            --color-text-secondary: #E2E8F0;
            --color-text-muted: #94A3B8;
            --color-primary: #F97316;
            --color-primary-hover: #FB923C;
            --color-primary-bright: #FB923C;
            --color-primary-contrast: #090D16;
            --color-primary-soft: rgba(249, 115, 22, 0.16);
            --color-primary-soft-border: rgba(249, 115, 22, 0.38);
            --color-border: rgba(148, 163, 184, 0.18);
            --color-border-strong: rgba(148, 163, 184, 0.32);
            --color-success: #86EFAC;
            --color-success-bg: rgba(34, 197, 94, 0.16);
            --color-success-border: rgba(34, 197, 94, 0.38);
            --color-err: #FCA5A5;
            --color-err-bg: rgba(239, 68, 68, 0.16);
            --color-err-border: rgba(239, 68, 68, 0.38);
            --shadow-sm: 0 2px 6px rgba(0, 0, 0, 0.35);
            --shadow-md: 0 14px 32px -10px rgba(0, 0, 0, 0.5);
            --shadow-hero: 0 24px 48px -12px rgba(0, 0, 0, 0.6);
        }}
        * {{ box-sizing: border-box; }}
        html {{ scroll-behavior: smooth; }}
        body {{
            margin: 0;
            background:
                radial-gradient(circle at 85% 0%, rgba(234, 88, 12, 0.08), transparent 44%),
                radial-gradient(circle at 10% 35%, rgba(14, 165, 233, 0.06), transparent 42%),
                var(--color-bg);
            color: var(--color-text);
            font-family: var(--font-sans);
            font-size: 16px;
            line-height: 1.6;
            -webkit-font-smoothing: antialiased;
        }}
        a, button, input {{ font: inherit; }}
        a:focus-visible, button:focus-visible, input:focus-visible {{
            outline: 2px solid var(--color-primary);
            outline-offset: 2px;
        }}
        .ico {{ width: 18px; height: 18px; display: inline-block; vertical-align: middle; flex-shrink: 0; }}
        .container {{ max-width: 1200px; margin: 0 auto; padding: 0 18px; }}

        /* Header */
        .site-header {{
            position: sticky; top: 0; z-index: 30;
            background: var(--color-header);
            backdrop-filter: blur(12px);
            -webkit-backdrop-filter: blur(12px);
            border-bottom: 1px solid var(--color-border);
        }}
        .site-header__inner {{
            display: flex; align-items: center; justify-content: space-between;
            gap: 12px; min-height: 70px; flex-wrap: wrap; padding: 10px 0;
        }}
        .brand {{
            display: inline-flex; align-items: center; gap: 11px;
            text-decoration: none; color: var(--color-text);
            font-family: var(--font-display); font-weight: 800; font-size: 1.12rem;
            letter-spacing: -0.02em;
        }}
        .brand__logo {{
            width: 40px; height: 40px; border-radius: 9999px; object-fit: cover; display: block;
            border: 2px solid var(--color-primary-bright);
            box-shadow: 0 4px 12px rgba(234, 88, 12, 0.22);
        }}
        .nav {{ display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }}
        .nav__link {{
            color: var(--color-text-muted); text-decoration: none; font-size: 0.92rem; font-weight: 600;
            padding: 8px 10px; min-height: 44px; display: inline-flex; align-items: center;
            border-radius: var(--radius-sm);
            transition: color 160ms ease, background-color 160ms ease;
        }}
        .nav__link:hover {{ color: var(--color-text); background: var(--color-inset); }}
        .theme-toggle {{
            min-height: 42px; padding: 8px 14px; border-radius: 9999px;
            display: inline-flex; align-items: center; gap: 6px;
            background: var(--color-surface); border: 1px solid var(--color-border-strong);
            color: var(--color-text); font-size: 0.84rem; font-weight: 700; cursor: pointer;
            transition: border-color 160ms ease;
        }}
        .theme-toggle:hover {{ border-color: var(--color-primary); }}
        .plat-dot {{ width: 8px; height: 8px; border-radius: 9999px; flex-shrink: 0; display: inline-block; }}
        .platforms-strip {{ display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 18px; }}
        .plat-chip {{
            display: inline-flex; align-items: center; gap: 6px;
            padding: 5px 11px; border-radius: 999px; font-size: 0.8rem; font-weight: 700;
            background: var(--color-surface); border: 1px solid var(--color-border);
            color: var(--color-text-secondary); box-shadow: var(--shadow-sm);
        }}
        .btn {{
            display: inline-flex; align-items: center; justify-content: center; gap: 8px;
            min-height: 44px; padding: 10px 18px; border-radius: var(--radius-md);
            border: 1px solid transparent; font-weight: 700; font-size: 0.92rem;
            text-decoration: none; cursor: pointer;
            transition: background-color 160ms ease, border-color 160ms ease, transform 160ms ease;
        }}
        .btn.is-copied {{
            border-color: var(--color-success); color: var(--color-success); background: var(--color-success-bg);
        }}
        .btn--primary {{
            background: var(--color-primary); color: var(--color-primary-contrast);
            border-color: var(--color-primary);
        }}
        .btn--primary:hover {{ background: var(--color-primary-hover); border-color: var(--color-primary-hover); }}
        .btn--secondary {{
            background: var(--color-surface); color: var(--color-text);
            border-color: var(--color-border-strong);
        }}
        .btn--secondary:hover {{ border-color: var(--color-primary); background: var(--color-inset); }}

        /* Hero 2-Column 3D Stage */
        .hero {{ padding: 34px 0 24px; }}
        .hero-grid {{
            display: grid;
            grid-template-columns: minmax(0, 1.25fr) minmax(280px, 0.85fr);
            gap: 28px;
            align-items: center;
            margin-bottom: 24px;
        }}
        .hero__eyebrow {{
            display: inline-flex; align-items: center; gap: 8px;
            padding: 6px 13px; border-radius: 999px;
            background: var(--color-success-bg); border: 1px solid var(--color-success-border);
            color: var(--color-success); font-size: 0.84rem; font-weight: 700;
        }}
        .hero__title {{
            font-family: var(--font-display);
            font-size: clamp(1.78rem, 3.6vw, 2.65rem);
            line-height: 1.18; letter-spacing: -0.025em;
            margin: 14px 0 12px; color: var(--color-text);
        }}
        .hero__subtitle {{
            color: var(--color-text-muted); max-width: 62ch; margin: 0 0 18px; font-size: 0.98rem;
        }}
        .stats-bar {{
            display: flex; flex-wrap: wrap; gap: 12px; margin-bottom: 20px;
        }}
        .stat-chip {{
            display: inline-flex; align-items: center; gap: 7px;
            padding: 6px 12px; border-radius: var(--radius-sm);
            background: var(--color-surface); border: 1px solid var(--color-border);
            color: var(--color-text-muted); font-size: 0.85rem; box-shadow: var(--shadow-sm);
        }}
        .stat-chip strong {{ color: var(--color-text); font-family: var(--font-mono); }}

        .hero-visual {{
            display: flex; align-items: center; justify-content: center;
            perspective: 1100px;
        }}
        .stage3d {{
            position: relative; width: 100%; max-width: 390px; aspect-ratio: 1 / 1;
            border-radius: 26px; padding: 14px;
            background: linear-gradient(145deg, var(--color-surface) 0%, var(--color-surface-elevated) 100%);
            border: 1px solid var(--color-border);
            box-shadow: var(--shadow-hero);
            transform-style: preserve-3d;
            transform: rotateX(var(--rx, 0deg)) rotateY(var(--ry, 0deg));
            transition: transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
        }}
        .stage3d::before {{
            content: ""; position: absolute; inset: 0; border-radius: 26px; pointer-events: none;
            background: radial-gradient(circle at var(--gx, 50%) var(--gy, 45%), rgba(234, 88, 12, 0.16), transparent 62%);
        }}
        .stage3d__img {{
            width: 100%; height: 100%; object-fit: cover; border-radius: 18px; display: block;
            box-shadow: 0 10px 24px rgba(15, 23, 42, 0.14);
        }}
        .float-tag {{
            position: absolute; display: inline-flex; align-items: center; gap: 7px;
            padding: 8px 12px; border-radius: 999px; font-size: 0.78rem; font-weight: 700;
            background: var(--color-surface); color: var(--color-text);
            border: 1px solid var(--color-border); box-shadow: var(--shadow-md);
            white-space: nowrap;
        }}
        .float-tag--top {{ top: -10px; right: 14px; color: var(--color-primary); border-color: var(--color-primary-soft-border); }}
        .float-tag--bot {{ bottom: -10px; left: 14px; color: var(--color-success); border-color: var(--color-success-border); }}

        /* Convert & Search Panels */
        .convert-panel {{
            background: var(--color-surface);
            border: 1px solid var(--color-border);
            border-top: 3px solid var(--color-primary-bright);
            border-radius: var(--radius-lg); padding: 18px; box-shadow: var(--shadow-md);
            margin-bottom: 16px;
        }}
        .convert-panel__title {{
            font-family: var(--font-display); font-size: 1.02rem; font-weight: 700;
            margin: 0 0 12px; color: var(--color-text);
            display: flex; align-items: center; gap: 8px;
        }}
        .search-panel {{
            background: var(--color-surface); border: 1px solid var(--color-border);
            border-radius: var(--radius-lg); padding: 16px; box-shadow: var(--shadow-sm);
        }}
        .search-form {{ display: flex; flex-wrap: wrap; gap: 10px; align-items: stretch; }}
        .sr-only {{
            position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px;
            overflow: hidden; clip: rect(0, 0, 0, 0); border: 0;
        }}
        .search-input {{
            flex: 1 1 240px; min-height: 44px; padding: 10px 14px;
            border-radius: var(--radius-md); border: 1px solid var(--color-border-strong);
            background: var(--color-surface-elevated); color: var(--color-text);
        }}
        .search-input::placeholder {{ color: var(--color-text-muted); opacity: 0.78; }}
        .filter-row {{ display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; }}
        .filter-pill {{
            min-height: 40px; padding: 6px 14px; border-radius: 999px;
            border: 1px solid var(--color-border); background: var(--color-surface-elevated);
            color: var(--color-text-muted); text-decoration: none; font-size: 0.88rem; font-weight: 600;
            display: inline-flex; align-items: center; gap: 7px;
            transition: border-color 150ms ease, background-color 150ms ease, color 150ms ease;
        }}
        .filter-pill:hover {{ border-color: var(--color-primary); color: var(--color-text); }}
        .filter-pill--active {{
            background: var(--color-primary); color: var(--color-primary-contrast); border-color: var(--color-primary);
        }}

        /* Convert Result Box */
        .convert-result {{
            margin-top: 14px; background: var(--color-surface-elevated); border: 1px solid var(--color-border);
            border-radius: var(--radius-md); padding: 15px;
        }}
        .convert-result__head {{ display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 8px; }}
        .convert-result__title {{ font-family: var(--font-display); font-size: 1rem; margin: 0 0 6px; color: var(--color-text); }}
        .convert-result__meta {{ margin: 0 0 8px; color: var(--color-text-muted); font-size: 0.9rem; }}
        .convert-result__url {{
            background: var(--color-surface); border: 1px dashed var(--color-border-strong);
            padding: 9px 12px; border-radius: var(--radius-sm); word-break: break-all;
            font-size: 0.85rem; font-family: var(--font-mono); margin-bottom: 12px; color: var(--color-primary);
        }}
        .convert-actions {{ display: flex; flex-wrap: wrap; gap: 10px; }}
        .convert-alert {{
            margin-top: 12px; padding: 12px 14px; border-radius: var(--radius-md); font-size: 0.92rem;
            display: flex; align-items: flex-start; gap: 10px;
        }}
        .convert-alert--error {{
            background: var(--color-err-bg); border: 1px solid var(--color-err-border); color: var(--color-err); font-weight: 600;
        }}

        /* Deals Grid & Steps */
        .section {{ padding: 26px 0; }}
        .section__head {{
            display: flex; flex-wrap: wrap; justify-content: space-between; align-items: baseline;
            gap: 12px; margin-bottom: 18px;
        }}
        .section__title {{ font-family: var(--font-display); font-size: 1.4rem; margin: 0; color: var(--color-text); }}
        .deals-grid {{
            display: grid; gap: 16px; grid-template-columns: 1fr;
        }}
        @media (min-width: 720px) {{ .deals-grid {{ grid-template-columns: repeat(2, minmax(0, 1fr)); }} }}
        @media (min-width: 1060px) {{ .deals-grid {{ grid-template-columns: repeat(3, minmax(0, 1fr)); }} }}
        .deal-card {{
            display: flex; flex-direction: column; justify-content: space-between; gap: 12px;
            background: var(--color-surface); border: 1px solid var(--color-border);
            border-top: 3px solid var(--plat-accent, var(--color-primary-bright));
            border-radius: var(--radius-lg); padding: 18px; box-shadow: var(--shadow-sm);
            transition: border-color 160ms ease, box-shadow 160ms ease, transform 160ms ease;
        }}
        .deal-card:hover {{ border-color: var(--color-primary-bright); box-shadow: var(--shadow-md); }}
        .deal-card__top {{ display: flex; justify-content: space-between; align-items: center; gap: 8px; }}
        .badge {{
            font-size: 0.78rem; font-weight: 700; padding: 4px 10px; border-radius: 999px;
        }}
        .badge--platform {{
            display: inline-flex; align-items: center; gap: 6px;
            background: var(--color-inset); color: var(--color-text-secondary); border: 1px solid var(--color-border);
        }}
        .badge--accent {{ background: var(--color-primary-soft); color: var(--color-primary); border: 1px solid var(--color-primary-soft-border); }}
        .deal-card__title {{ font-family: var(--font-display); font-size: 1.04rem; margin: 0; color: var(--color-text); }}
        .deal-card__desc {{ margin: 0; color: var(--color-text-muted); font-size: 0.92rem; }}
        .deal-card__meta {{ display: flex; flex-direction: column; gap: 6px; font-size: 0.85rem; }}
        .coupon-row {{ display: flex; align-items: center; justify-content: space-between; gap: 8px; flex-wrap: wrap; }}
        .copy-code-btn {{
            min-height: 32px; padding: 4px 10px; border-radius: 8px;
            border: 1px solid var(--color-border-strong); background: var(--color-surface-elevated);
            color: var(--color-text); font-size: 0.78rem; font-weight: 700; cursor: pointer;
            display: inline-flex; align-items: center; gap: 5px;
            transition: border-color 150ms ease, background-color 150ms ease, color 150ms ease;
        }}
        .copy-code-btn:hover {{ border-color: var(--color-primary); color: var(--color-primary); }}
        .copy-code-btn.is-copied {{
            border-color: var(--color-success); color: var(--color-success); background: var(--color-success-bg);
        }}
        .coupon-pill code {{
            background: var(--color-inset); border: 1px dashed var(--color-primary);
            padding: 2px 8px; border-radius: 6px; color: var(--color-primary);
            font-family: var(--font-mono); font-weight: 700;
        }}
        .coupon-pill--link {{
            display: inline-flex; align-items: center; gap: 6px;
            padding: 4px 10px; border-radius: 8px;
            background: var(--color-success-bg); border: 1px solid var(--color-success-border);
            color: var(--color-success); font-size: 0.78rem; font-weight: 700;
        }}
        .deal-card__note {{ color: var(--color-text-muted); font-size: 0.84rem; }}
        .deal-card__cta {{ width: 100%; }}
        .steps-grid {{
            display: grid; gap: 16px; grid-template-columns: 1fr;
        }}
        @media (min-width: 768px) {{ .steps-grid {{ grid-template-columns: repeat(3, minmax(0, 1fr)); }} }}
        .step-card {{
            background: var(--color-surface); border: 1px solid var(--color-border);
            border-radius: var(--radius-lg); padding: 20px; box-shadow: var(--shadow-sm);
            display: flex; flex-direction: column; gap: 10px;
        }}
        .step-num {{
            width: 36px; height: 36px; border-radius: 10px;
            display: inline-flex; align-items: center; justify-content: center;
            background: var(--color-primary-soft); border: 1px solid var(--color-primary-soft-border);
            color: var(--color-primary); font-family: var(--font-mono); font-weight: 800; font-size: 0.92rem;
        }}
        .step-card h3 {{ margin: 0; font-family: var(--font-display); font-size: 1.04rem; color: var(--color-text); }}
        .step-card p {{ margin: 0; color: var(--color-text-muted); font-size: 0.92rem; }}
        .empty-state {{
            grid-column: 1 / -1; background: var(--color-surface); border: 1px solid var(--color-border);
            border-radius: var(--radius-lg); padding: 28px; text-align: center; color: var(--color-text-muted);
        }}
        .site-footer {{
            border-top: 1px solid var(--color-border); margin-top: 32px; padding: 28px 0 40px;
            background: var(--color-surface); color: var(--color-text-muted); font-size: 0.88rem;
        }}
        .site-footer__links {{ display: flex; flex-wrap: wrap; gap: 16px; margin-top: 10px; }}
        .site-footer a {{ color: var(--color-primary); font-weight: 700; text-decoration: none; }}
        .site-footer a:hover {{ text-decoration: underline; }}

        @media (max-width: 900px) {{
            .hero-grid {{ grid-template-columns: minmax(0, 1fr); }}
            .stage3d {{ max-width: 320px; }}
        }}
        @media (prefers-reduced-motion: no-preference) {{
            .deal-card:hover {{ transform: translateY(-2px); }}
            .btn--primary:hover {{ transform: translateY(-1px); }}
        }}
        @media (prefers-reduced-motion: reduce) {{
            *, *::before, *::after {{ transition: none !important; scroll-behavior: auto !important; transform: none !important; }}
        }}
    </style>
</head>
<body>
    {}
    <header class="site-header">
        <div class="container site-header__inner">
            <a class="brand" href="/">
                <img src="/assets/bot-logo.webp" alt="Logo Thế Giới Siêu Sale" class="brand__logo">
                <span>Thế Giới Siêu Sale</span>
            </a>
            <nav class="nav" aria-label="Điều hướng chính">
                <a class="nav__link" href="#uu-dai">Ưu đãi nổi bật</a>
                <a class="nav__link" href="#huong-dan">Cách nhận mã</a>
                <a class="nav__link" href="#chinh-sach">Minh bạch đối tác</a>
                <button type="button" id="public-theme-btn" class="theme-toggle" aria-label="Chuyển đổi giao diện Sáng/Tối">
                    <svg class="ico" style="width:15px;height:15px;" aria-hidden="true"><use id="public-theme-ico-use" href="#ico-moon"/></svg>
                    <span id="public-theme-text">Giao diện Tối</span>
                </button>
                <a class="btn btn--primary" href="https://t.me/{}" target="_blank" rel="noopener noreferrer">Mở Trợ Lý @{}</a>
            </nav>
        </div>
    </header>

    <main class="container">
        <section class="hero">
            <div class="hero-grid">
                <div>
                    <span class="hero__eyebrow">
                        <svg class="ico" aria-hidden="true"><use href="#ico-shield"/></svg>
                        <span>Cập nhật tự động từ đối tác chính hãng</span>
                    </span>
                    <h1 class="hero__title">Tổng hợp mã giảm giá &amp; gợi ý sản phẩm tiết kiệm mỗi ngày</h1>
                    <p class="hero__subtitle">
                        Thế Giới Siêu Sale tổng hợp các chương trình ưu đãi, mã giảm giá công khai theo từng gian hàng và ngành hàng từ Shopee, Tiki, Lazada, TikTok Shop và Alibaba giúp người mua dễ dàng so sánh và đặt hàng trực tiếp trên sàn.
                    </p>
                    <div class="stats-bar">
                        <span class="stat-chip">
                            <svg class="ico" aria-hidden="true"><use href="#ico-ticket"/></svg>
                            <span>Đang hiển thị: <strong>{}</strong> / <strong>{}</strong> ưu đãi</span>
                        </span>
                        <span class="stat-chip">
                            <svg class="ico" aria-hidden="true"><use href="#ico-clock"/></svg>
                            <span>Đồng bộ gần nhất: <strong>{}</strong></span>
                        </span>
                    </div>
                    <div class="platforms-strip" aria-label="Các sàn TMĐT hỗ trợ">
                        <span class="plat-chip"><span class="plat-dot" style="background:#EA580C;" aria-hidden="true"></span>Shopee</span>
                        <span class="plat-chip"><span class="plat-dot" style="background:#0284C7;" aria-hidden="true"></span>Tiki</span>
                        <span class="plat-chip"><span class="plat-dot" style="background:#DB2777;" aria-hidden="true"></span>Lazada</span>
                        <span class="plat-chip"><span class="plat-dot" style="background:#E11D48;" aria-hidden="true"></span>TikTok Shop</span>
                        <span class="plat-chip"><span class="plat-dot" style="background:#D97706;" aria-hidden="true"></span>Alibaba</span>
                    </div>
                </div>

                <div class="hero-visual">
                    <div id="stage3d" class="stage3d">
                        <img src="/assets/bot-cart-3d.webp" alt="Trợ lý 3D Thế Giới Siêu Sale săn mã giảm giá" class="stage3d__img">
                        <div class="float-tag float-tag--top" aria-hidden="true">
                            <svg class="ico" aria-hidden="true"><use href="#ico-zap"/></svg>
                            <span>Săn mã giảm đến 50%</span>
                        </div>
                        <div class="float-tag float-tag--bot" aria-hidden="true">
                            <svg class="ico" aria-hidden="true"><use href="#ico-check"/></svg>
                            <span>Kiểm tra link sống 100%</span>
                        </div>
                    </div>
                </div>
            </div>

            <div class="convert-panel">
                <p class="convert-panel__title">
                    <svg class="ico" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></svg>
                    <span>Dán link sản phẩm để lấy link ưu đãi &amp; mã khớp ngay (không cần cài App)</span>
                </p>
                <form class="search-form" method="POST" action="/convert">
                    <label class="sr-only" for="web-convert-url">Dán đường link sản phẩm cần săn mã</label>
                    <input id="web-convert-url" class="search-input" type="url" name="url" required value="{}" placeholder="Dán link sản phẩm Shopee, Tiki, Lazada, TikTok Shop hoặc Alibaba vào đây...">
                    <button class="btn btn--primary" type="submit">
                        <svg class="ico" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>
                        <span>Lấy Link Ưu Đãi</span>
                    </button>
                </form>
                {}
            </div>

            <div class="search-panel">
                <form class="search-form" method="GET" action="/">
                    <label class="sr-only" for="deal-search">Tìm mã giảm giá hoặc tên gian hàng</label>
                    <input id="deal-search" class="search-input" type="text" name="q" value="{}" placeholder="Tìm tên gian hàng, ngành hàng hoặc mã ưu đãi (VD: Tiki Trading, Gia dụng...)">
                    <input type="hidden" name="plat" value="{}">
                    <button class="btn btn--primary" type="submit">
                        <svg class="ico" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
                        <span>Tìm ưu đãi</span>
                    </button>
                </form>
                <div class="filter-row" aria-label="Lọc theo sàn thương mại điện tử">
                    {}
                </div>
            </div>
        </section>

        <section id="uu-dai" class="section">
            <div class="section__head">
                <h2 class="section__title">Danh sách ưu đãi &amp; mã giảm giá mới nhất</h2>
                <span class="deal-card__note">Chuyển tiếp trực tiếp sang trang chính thức của sàn</span>
            </div>
            <div class="deals-grid">
                {}
            </div>
        </section>

        <section id="huong-dan" class="section">
            <div class="section__head">
                <h2 class="section__title">Quy trình sử dụng nhanh chóng &amp; hợp lệ</h2>
            </div>
            <div class="steps-grid">
                <article class="step-card">
                    <span class="step-num" aria-hidden="true">01</span>
                    <h3>Chọn ưu đãi hoặc gửi link sản phẩm</h3>
                    <p>Tìm mã theo ngành hàng ngay trên website hoặc dán liên kết sản phẩm trực tiếp cho trợ lý Telegram @{}.</p>
                </article>
                <article class="step-card">
                    <span class="step-num" aria-hidden="true">02</span>
                    <h3>Lưu mã tại trang gian hàng chính thức</h3>
                    <p>Bấm nút mở gian hàng để truy cập trang khuyến mãi hợp lệ trên Shopee, Tiki, Lazada hoặc TikTok Shop.</p>
                </article>
                <article class="step-card">
                    <span class="step-num" aria-hidden="true">03</span>
                    <h3>Thanh toán trực tiếp trên sàn</h3>
                    <p>Người dùng chủ động chọn mã ở bước thanh toán trên ứng dụng thương mại điện tử để hưởng mức giá tốt nhất.</p>
                </article>
            </div>
        </section>
    </main>

    <footer id="chinh-sach" class="site-footer">
        <div class="container">
            <p><strong>Tuyên bố minh bạch tiếp thị liên kết (Affiliate Disclosure):</strong> Thế Giới Siêu Sale là website tổng hợp thông tin khuyến mãi độc lập, hoạt động theo quy chuẩn đối tác tiếp thị liên kết chính thức. Khi người dùng bấm vào liên kết ưu đãi và hoàn tất mua sắm tự nhiên trên sàn thương mại điện tử, nền tảng có thể nhận hoa hồng giới thiệu mà không làm tăng giá sản phẩm của người mua.</p>
            <div class="site-footer__links">
                <span>Thế Giới Siêu Sale © 2026</span>
                <a href="https://t.me/{}" target="_blank" rel="noopener noreferrer">Kênh hỗ trợ Telegram</a>
            </div>
        </div>
    </footer>
    <script src="/assets/home.js" defer></script>
    <script src="/assets/landing.js" defer></script>
</body>
</html>"##,
        PUBLIC_SVG_SPRITE,
        escape_html(&state.config.bot_username),
        escape_html(&state.config.bot_username),
        matched_count,
        total_available,
        escape_html(&last_sync_text),
        escape_html(convert_input_val),
        convert_result_html,
        escape_html(&search_q),
        escape_html(&filter_plat),
        filter_links,
        cards_html,
        escape_html(&state.config.bot_username),
        escape_html(&state.config.bot_username)
    );

    Html(html).into_response()
}

fn urlencoding_simple(data: &str) -> String {
    url::form_urlencoded::byte_serialize(data.as_bytes()).collect()
}
