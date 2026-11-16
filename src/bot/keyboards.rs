use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardMarkup};
use url::Url;

/// Bàn phím Reply Keyboard thường trực ở dưới khung chat (Tập trung săn sale)
pub fn persistent_reply_keyboard() -> KeyboardMarkup {
    KeyboardMarkup::new(vec![
        vec![
            KeyboardButton::new("🎟️ Kho Mã 20% - 35%+"),
        ],
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
            InlineKeyboardButton::callback("🎟️ Kho Mã 20% - 35%+ & Độc Quyền", "deals_page:all:0"),
        ],
        vec![
            InlineKeyboardButton::callback("📖 Hướng Dẫn Áp Mã Thành Công 100%", "menu_guide"),
        ],
        vec![
            InlineKeyboardButton::url("🚀 Chia Sẻ DealX Cho Bạn Bè", share_url),
        ],
    ])
}

/// Bàn phím lọc & phân trang cho Kho Mã Giảm Giá (/deals)
pub fn deals_pagination_keyboard(filter: &str, page: usize, total_pages: usize) -> InlineKeyboardMarkup {
    let filter_row = vec![
        InlineKeyboardButton::callback(
            if filter == "all" { "• 🌟 Tất Cả •" } else { "🌟 Tất Cả" },
            "deals_page:all:0",
        ),
        InlineKeyboardButton::callback(
            if filter == "exclusive" { "• 👑 Độc Quyền •" } else { "👑 Độc Quyền" },
            "deals_page:exclusive:0",
        ),
        InlineKeyboardButton::callback(
            if filter == "pct20" { "• 🔥 20%-35%+ •" } else { "🔥 20%-35%+" },
            "deals_page:pct20:0",
        ),
    ];

    let mut rows = vec![filter_row];

    if total_pages > 1 {
        let mut nav_row = Vec::new();
        if page > 0 {
            nav_row.push(InlineKeyboardButton::callback(
                "⬅️ Trang trước",
                format!("deals_page:{}:{}", filter, page - 1),
            ));
        }
        nav_row.push(InlineKeyboardButton::callback(
            format!("📄 {}/{}", page + 1, total_pages),
            format!("deals_page:{}:{}", filter, page),
        ));
        if page + 1 < total_pages {
            nav_row.push(InlineKeyboardButton::callback(
                "Trang sau ➡️",
                format!("deals_page:{}:{}", filter, page + 1),
            ));
        }
        rows.push(nav_row);
    }

    rows.push(vec![InlineKeyboardButton::callback(
        "🔄 Cập Nhật Kho Mã Mới Nhất",
        format!("deals_refresh:{}:{}", filter, page),
    )]);

    InlineKeyboardMarkup::new(rows)
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

    rows.push(vec![InlineKeyboardButton::callback(
        "🔥 Xem Kho Mã 20% - 35%+ & Độc Quyền",
        "deals_page:all:0",
    )]);

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
