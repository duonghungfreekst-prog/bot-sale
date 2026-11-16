use rand::RngExt;

/// Định dạng số tiền thành chuỗi tiền tệ Việt Nam Đồng (VND)
/// Ví dụ: 150000.0 -> "150,000 ₫"
pub fn format_vnd(amount: f64) -> String {
    let int_part = amount.round() as i64;
    let is_negative = int_part < 0;
    let abs_val = int_part.abs();
    let s = abs_val.to_string();
    
    let mut result = String::new();
    let chars: Vec<char> = s.chars().rev().collect();
    
    for (i, ch) in chars.iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(*ch);
    }
    
    let formatted: String = result.chars().rev().collect();
    if is_negative {
        format!("-{} ₫", formatted)
    } else {
        format!("{} ₫", formatted)
    }
}

/// Chống CRLF / Log Injection (theo quy định PHẦN 3)
#[allow(dead_code)]
pub fn sanitize_crlf(input: &str) -> String {
    input
        .chars()
        .filter(|&c| c != '\r' && c != '\n')
        .collect::<String>()
        .trim()
        .to_string()
}

/// Sinh mã giới thiệu ngẫu nhiên an toàn (7 ký tự in hoa và số)
/// Ví dụ: "REF8X2M"
pub fn generate_referral_code() -> String {
    const CHARSET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
    let mut rng = rand::rng();
    let code: String = (0..6)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();
    format!("R{}", code)
}

/// Ẩn bớt số tài khoản ngân hàng để bảo vệ dữ liệu người dùng
/// Ví dụ: "123456789" -> "123***789"
#[allow(dead_code)]
pub fn mask_account(acc: &str) -> String {
    let len = acc.chars().count();
    if len <= 4 {
        return acc.to_string();
    }
    let chars: Vec<char> = acc.chars().collect();
    let prefix: String = chars.iter().take(3).collect();
    let suffix: String = chars.iter().skip(len - 3).collect();
    format!("{}***{}", prefix, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_vnd() {
        assert_eq!(format_vnd(0.0), "0 ₫");
        assert_eq!(format_vnd(50000.0), "50,000 ₫");
        assert_eq!(format_vnd(1234567.0), "1,234,567 ₫");
    }

    #[test]
    fn test_sanitize_crlf() {
        assert_eq!(sanitize_crlf("hello\r\nworld"), "helloworld");
        assert_eq!(sanitize_crlf(" admin  "), "admin");
    }

    #[test]
    fn test_generate_referral_code() {
        let code = generate_referral_code();
        assert_eq!(code.len(), 7);
        assert!(code.starts_with('R'));
    }
}
