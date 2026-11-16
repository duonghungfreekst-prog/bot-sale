use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};
use url::Url;

/// Bàn phím Reply Keyboard thường trực ở dưới khung chat (Tập trung săn sale)
pub fn persistent_reply_keyboard() -> KeyboardMarkup {
    KeyboardMarkup::new(vec![
        vec![
            KeyboardButton::new("🎟️ Mã Giảm Giá Hôm Nay"),
            KeyboardButton::new("📖 Hướng Dẫn Săn Sale"),
        ],
        vec![
            KeyboardButton::new("🟠 Săn Sale Shopee"),
            KeyboardButton::new("🔵 Săn Sale Lazada"),
        ],
        vec![
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
            InlineKeyboardButton::callback("🎟️ Xem Mã Giảm Giá Hôm Nay", "menu_deals"),
        ],
        vec![
            InlineKeyboardButton::url("🚀 Chia Sẻ Cho Bạn Bè", share_url),
            InlineKeyboardButton::callback("📖 Hướng Dẫn Áp Mã", "menu_guide"),
        ],
    ])
}

/// Bàn phím hành động khi người dùng gửi link sản phẩm
pub fn deal_action_keyboard(affiliate_url: &str, platform: &str) -> InlineKeyboardMarkup {
    let deals_callback = match platform.to_lowercase().as_str() {
        "shopee" => "deals_shopee",
        "lazada" => "deals_lazada",
        "tiki" => "deals_tiki",
        _ => "menu_deals",
    };

    let target_url = Url::parse(affiliate_url)
        .unwrap_or_else(|_| Url::parse("https://shopee.vn").unwrap());

    InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::url(
            format!("🛍️ Mở {} & Áp Mã Sale Ngay 🛍️", platform),
            target_url,
        )],
        vec![
            InlineKeyboardButton::callback("🎟️ Xem Thêm Mã Sàn Này", deals_callback),
        ],
    ])
}

/// Bàn phím chọn sàn để lọc mã giảm giá
pub fn deals_filter_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("🟠 Shopee", "deals_shopee"),
            InlineKeyboardButton::callback("🔵 Lazada", "deals_lazada"),
            InlineKeyboardButton::callback("🔵 Tiki", "deals_tiki"),
        ],
        vec![
            InlineKeyboardButton::callback("🌟 Xem Tất Cả Mã", "deals_all"),
            InlineKeyboardButton::callback("🔙 Về Menu Chính", "menu_main"),
        ],
    ])
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
