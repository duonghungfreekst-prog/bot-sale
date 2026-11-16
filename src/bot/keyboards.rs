use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};
use url::Url;

/// Bàn phím Reply Keyboard thường trực ở dưới khung chat
pub fn persistent_reply_keyboard() -> KeyboardMarkup {
    KeyboardMarkup::new(vec![
        vec![
            KeyboardButton::new("🚀 Bắt Đầu"),
            KeyboardButton::new("💰 Ví Hoàn Tiền"),
        ],
        vec![
            KeyboardButton::new("🎟️ Mã Giảm Giá"),
            KeyboardButton::new("🤝 Giới Thiệu Bạn Bè"),
        ],
        vec![
            KeyboardButton::new("📖 Hướng Dẫn"),
            KeyboardButton::new("💳 Rút Tiền"),
        ],
    ])
    .resize_keyboard()
    .persistent()
}

/// Bàn phím Inline cho Menu chính
pub fn main_menu_keyboard(bot_username: &str, ref_code: &str) -> InlineKeyboardMarkup {
    let share_text = format!(
        "Tớ vừa tìm được Bot Săn Sale & Hoàn Tiền siêu xịn! Mua Shopee/Lazada/Tiki được hoàn tiền mặt như Shopback nè: https://t.me/{}?start={}",
        bot_username, ref_code
    );
    let share_url_str = format!(
        "https://t.me/share/url?url={}&text={}",
        urlencoding::encode(&format!("https://t.me/{}?start={}", bot_username, ref_code)),
        urlencoding::encode(&share_text)
    );

    let share_url = Url::parse(&share_url_str).unwrap_or_else(|_| Url::parse("https://t.me").unwrap());

    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("💰 Xem Ví Hoàn Tiền", "menu_wallet"),
            InlineKeyboardButton::callback("🎟️ Mã Giảm Giá", "menu_deals"),
        ],
        vec![
            InlineKeyboardButton::url("🚀 Mời Bạn Nhận Hoa Hồng", share_url),
            InlineKeyboardButton::callback("📜 Lịch Sử Đơn", "menu_history"),
        ],
        vec![
            InlineKeyboardButton::callback("📖 Hướng Dẫn Săn Sale", "menu_guide"),
            InlineKeyboardButton::callback("💳 Hướng Dẫn Rút Tiền", "menu_withdraw_guide"),
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
            format!("🛍️ Mở {} & Nhận Hoàn Tiền", platform),
            target_url,
        )],
        vec![
            InlineKeyboardButton::callback("🎟️ Mã Giảm Giá Sàn Này", deals_callback),
            InlineKeyboardButton::callback("💰 Kiểm Tra Số Dư Ví", "menu_wallet"),
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
            InlineKeyboardButton::callback("🌟 Xem Tất Cả Deals", "deals_all"),
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
