use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};
use url::Url;

/// Bàn phím Reply Keyboard thường trực ở dưới khung chat (Tập trung săn sale)
pub fn persistent_reply_keyboard() -> KeyboardMarkup {
    KeyboardMarkup::new(vec![
        vec![
            KeyboardButton::new("📖 Hướng Dẫn Săn Sale"),
            KeyboardButton::new("🚀 Chia Sẻ Cho Bạn Bè"),
        ],
    ])
    .resize_keyboard()
    .persistent()
}

/// Bàn phím Inline cho Menu chính
pub fn main_menu_keyboard(bot_username: &str) -> InlineKeyboardMarkup {
    let share_text = format!(
        "Dán link sản phẩm Shopee/Lazada vào bot này là tự động được gắn mã giảm giá 20% - 50k nè: https://t.me/{}",
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

/// Bàn phím hành động khi người dùng gửi link sản phẩm
pub fn deal_action_keyboard(affiliate_url: &str, platform: &str) -> InlineKeyboardMarkup {
    let target_url = Url::parse(affiliate_url)
        .unwrap_or_else(|_| Url::parse("https://shopee.vn").unwrap());

    let mut rows = vec![vec![InlineKeyboardButton::url(
        format!("🛍️ Mở {} & Mua Giá Giảm Ngay 🛍️", platform),
        target_url,
    )]];

    match platform.to_lowercase().as_str() {
        "shopee" => {
            if let (Ok(voucher_url), Ok(video_url)) = (
                Url::parse("https://shopee.vn/m/ma-giam-gia"),
                Url::parse("https://shopee.vn/m/shopee-video"),
            ) {
                rows.push(vec![
                    InlineKeyboardButton::url("🎟️ Lưu Mã Shopee", voucher_url),
                    InlineKeyboardButton::url("🎬 Săn Mã Video", video_url),
                ]);
            }
        }
        "lazada" => {
            if let Ok(voucher_url) = Url::parse("https://www.lazada.vn/voucher") {
                rows.push(vec![InlineKeyboardButton::url(
                    "🎟️ Thu Thập Voucher Lazada",
                    voucher_url,
                )]);
            }
        }
        "tiki" => {
            if let Ok(voucher_url) = Url::parse("https://tiki.vn/khuyen-mai/ma-giam-gia") {
                rows.push(vec![InlineKeyboardButton::url(
                    "🎟️ Lưu Mã Giảm Giá Tiki",
                    voucher_url,
                )]);
            }
        }
        _ => {}
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
