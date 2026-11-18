use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};
use url::Url;

/// Bàn phím Reply Keyboard thường trực ở dưới khung chat (Tập trung dán link sản phẩm)
pub fn persistent_reply_keyboard() -> KeyboardMarkup {
    KeyboardMarkup::new(vec![vec![
        KeyboardButton::new("📖 Hướng Dẫn Săn Sale"),
        KeyboardButton::new("🚀 Chia Sẻ Cho Bạn Bè"),
    ]])
    .resize_keyboard()
    .persistent()
}

/// Bàn phím Inline cho Menu chính
pub fn main_menu_keyboard(bot_username: &str) -> InlineKeyboardMarkup {
    let share_text = format!(
        "Dán link sản phẩm Shopee/Lazada/Tiki/TikTok vào bot này để lấy link mua kèm mã giảm giá phù hợp nè: https://t.me/{}",
        bot_username
    );
    let share_url_str = format!(
        "https://t.me/share/url?url={}&text={}",
        urlencoding::encode(&format!("https://t.me/{}", bot_username)),
        urlencoding::encode(&share_text)
    );

    let share_url = Url::parse(&share_url_str).unwrap_or_else(|_| Url::parse("https://t.me").unwrap());

    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("📖 Hướng Dẫn Áp Mã Thành Công 100%", "menu_guide"),
        ],
        vec![
            InlineKeyboardButton::url("🚀 Chia Sẻ DealX Cho Bạn Bè", share_url),
        ],
    ])
}

/// Bàn phím hành động khi người dùng gửi link sản phẩm (100% nút đã được bọc link an toàn)
pub fn deal_action_keyboard(
    wrapped_product_url: &str,
    platform: &str,
    voucher_links: &[(String, String)],
    portal_links: &[(String, String)],
) -> InlineKeyboardMarkup {
    let target_url = Url::parse(wrapped_product_url)
        .unwrap_or_else(|_| Url::parse("https://shopee.vn").unwrap());

    let mut rows = vec![vec![InlineKeyboardButton::url(
        format!("🛍️ Mở {} & Mua Ngay 🛍️", platform),
        target_url,
    )]];

    for (label, raw_voucher_url) in voucher_links.iter().take(2) {
        if let Ok(parsed_voucher_url) = Url::parse(raw_voucher_url) {
            rows.push(vec![InlineKeyboardButton::url(
                label.clone(),
                parsed_voucher_url,
            )]);
        }
    }

    let mut portal_row = Vec::new();
    for (label, raw_portal_url) in portal_links {
        if let Ok(parsed_portal_url) = Url::parse(raw_portal_url) {
            portal_row.push(InlineKeyboardButton::url(label.clone(), parsed_portal_url));
        }
    }
    if !portal_row.is_empty() {
        rows.push(portal_row);
    }

    InlineKeyboardMarkup::new(rows)
}

mod urlencoding {
    pub fn encode(data: &str) -> String {
        let mut encoded = String::with_capacity(data.len() * 3);
        for byte in data.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    encoded.push(byte as char);
                }
                _ => {
                    encoded.push_str(&format!("%{:02X}", byte));
                }
            }
        }
        encoded
    }
}
