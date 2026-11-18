use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};

const SHA256_H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
    0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Tính mã băm SHA-256 dạng hex chữ thường chuẩn FIPS 180-4 (không cần crate ngoài)
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = SHA256_H0;
    let bit_len = (data.len() as u64).wrapping_mul(8);

    let mut padded = Vec::with_capacity(((data.len() + 9 + 63) / 64) * 64);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            let idx = i * 4;
            w[i] = u32::from_be_bytes([chunk[idx], chunk[idx + 1], chunk[idx + 2], chunk[idx + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = String::with_capacity(64);
    for word in h {
        out.push_str(&format!("{:08x}", word));
    }
    out
}

/// Kiểm tra Shopee Affiliate ID có hợp lệ (chuỗi chữ số từ 5 đến 20 ký tự)
pub fn is_valid_shopee_affiliate_id(id: &str) -> bool {
    let t = id.trim();
    (5..=20).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_digit())
}

/// Sinh link Shopee Affiliate trực tiếp qua định dạng an_redir chuẩn tại Việt Nam
pub fn build_direct_an_redir_link(clean_url: &str, affiliate_id: &str, sub_id: &str) -> String {
    let aff_trimmed = affiliate_id.trim();
    let encoded_origin = percent_encode_rfc3986(clean_url.trim());
    let encoded_sub = percent_encode_rfc3986(sub_id.trim());
    format!(
        "https://s.shopee.vn/an_redir?origin_link={}&affiliate_id={}&sub_id={}",
        encoded_origin, aff_trimmed, encoded_sub
    )
}

#[derive(Serialize)]
struct ShopeeGraphQlReq<'a> {
    query: &'a str,
    variables: ShopeeShortLinkVars<'a>,
}

#[derive(Serialize)]
struct ShopeeShortLinkVars<'a> {
    input: ShopeeShortLinkInput<'a>,
}

#[derive(Serialize)]
struct ShopeeShortLinkInput<'a> {
    #[serde(rename = "originUrl")]
    origin_url: &'a str,
    #[serde(rename = "subIds")]
    sub_ids: &'a [&'a str],
}

#[derive(Deserialize, Debug)]
struct ShopeeGraphQlResp {
    data: Option<ShopeeGraphQlData>,
    errors: Option<Vec<ShopeeGraphQlError>>,
}

#[derive(Deserialize, Debug)]
struct ShopeeGraphQlData {
    #[serde(rename = "generateShortLink")]
    generate_short_link: Option<ShopeeShortLinkResult>,
}

#[derive(Deserialize, Debug)]
struct ShopeeShortLinkResult {
    #[serde(rename = "shortLink")]
    short_link: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ShopeeGraphQlError {
    message: Option<String>,
}

/// Tạo link rút gọn Shopee Affiliate qua Open API GraphQL (`open-api.affiliate.shopee.vn/graphql`)
pub async fn generate_open_api_short_link(
    http: &Client,
    app_id: &str,
    app_secret: &str,
    origin_url: &str,
    sub_ids: &[&str],
) -> Result<String, String> {
    let app_id_clean = app_id.trim();
    let secret_clean = app_secret.trim();
    if app_id_clean.is_empty() || secret_clean.is_empty() {
        return Err("Thiếu App ID hoặc Secret Key của Shopee Open API".to_string());
    }

    let req_body = ShopeeGraphQlReq {
        query: "mutation($input: ShortLinkInput!) { generateShortLink(input: $input) { shortLink } }",
        variables: ShopeeShortLinkVars {
            input: ShopeeShortLinkInput {
                origin_url: origin_url.trim(),
                sub_ids,
            },
        },
    };
    let payload_json = serde_json::to_string(&req_body)
        .map_err(|e| format!("Lỗi đóng gói JSON Shopee Open API: {}", e))?;

    let timestamp = Utc::now().timestamp();
    let sign_payload = format!("{}{}{}{}", app_id_clean, timestamp, payload_json, secret_clean);
    let signature = sha256_hex(sign_payload.as_bytes());
    let auth_header = format!(
        "SHA256 Credential={}, Timestamp={}, Signature={}",
        app_id_clean, timestamp, signature
    );

    let resp = http
        .post("https://open-api.affiliate.shopee.vn/graphql")
        .header("Authorization", auth_header)
        .header("Content-Type", "application/json")
        .body(payload_json)
        .send()
        .await
        .map_err(|e| format!("Lỗi kết nối Shopee Open API: {}", e))?;

    let status = resp.status();
    let body_text = resp
        .text()
        .await
        .map_err(|e| format!("Lỗi đọc phản hồi Shopee Open API: {}", e))?;

    if !status.is_success() {
        return Err(format!("Shopee Open API trả về HTTP {}", status));
    }

    let parsed: ShopeeGraphQlResp = serde_json::from_str(&body_text)
        .map_err(|e| format!("Lỗi giải mã JSON Shopee Open API: {}", e))?;

    if let Some(errors) = parsed.errors {
        if let Some(first) = errors.first() {
            let msg = first
                .message
                .as_deref()
                .filter(|m| !m.trim().is_empty())
                .unwrap_or("Shopee Open API từ chối yêu cầu");
            return Err(msg.to_string());
        }
    }

    if let Some(short_link) = parsed
        .data
        .and_then(|d| d.generate_short_link)
        .and_then(|r| r.short_link)
        .map(|s| s.trim().to_string())
        .filter(|s| s.starts_with("https://"))
    {
        return Ok(short_link);
    }

    Err("Shopee Open API không trả về shortLink hợp lệ".to_string())
}

const SOCIAL_PASSTHROUGH_KEYS: [&str; 9] = [
    "fb_content_id",
    "encrypted_payload",
    "fbclid",
    "cts",
    "channel_type",
    "content_source",
    "content_type",
    "sm_sig_id",
    "gads_t_sig",
];

/// Trích xuất bộ tham số phiên mạng xã hội hợp lệ từ link mẫu hoặc chuỗi query (như cơ chế passthrough IG/FB Reels)
pub fn extract_social_passthrough_params(sample_input: &str) -> Option<String> {
    let trimmed = sample_input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let query_part = match trimmed.find('?') {
        Some(idx) => &trimmed[idx + 1..],
        None => trimmed.trim_start_matches('&'),
    };
    let query_clean = query_part.split('#').next().unwrap_or(query_part);

    let mut kept = Vec::new();
    let mut has_sig_or_payload = false;

    for pair in query_clean.split('&') {
        let Some((raw_key, raw_val)) = pair.split_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        let val = raw_val.trim();
        if !SOCIAL_PASSTHROUGH_KEYS.contains(&key) || val.is_empty() {
            continue;
        }
        let is_safe_val = val
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'%'));
        if !is_safe_val {
            continue;
        }
        if key == "sm_sig_id" || key == "encrypted_payload" {
            has_sig_or_payload = true;
        }
        kept.push(format!("{}={}", key, val));
    }

    if has_sig_or_payload && !kept.is_empty() {
        Some(kept.join("&"))
    } else {
        None
    }
}

/// Nối bộ tham số phiên mở khóa mã IG/FB Reels vào đuôi link Shopee Affiliate (s.shopee.vn)
pub fn append_social_params_to_shopee_link(
    shopee_url: &str,
    raw_social_sample: Option<&str>,
) -> String {
    let Some(extra) = raw_social_sample.and_then(extract_social_passthrough_params) else {
        return shopee_url.to_string();
    };
    if !shopee_url.contains("shopee.vn") || shopee_url.contains("sm_sig_id=") {
        return shopee_url.to_string();
    }
    let sep = if shopee_url.contains('?') { '&' } else { '?' };
    format!("{}{}{}", shopee_url, sep, extra)
}

#[derive(Serialize)]
struct ShopeeReportReq {
    query: String,
}

/// Lấy danh sách đơn hàng phát sinh từ Shopee Affiliate Open API (`conversionReport`)
pub async fn fetch_open_api_recent_orders(
    http: &Client,
    app_id: &str,
    app_secret: &str,
    days_back: i64,
) -> Result<Vec<crate::db::models::TrackedOrder>, String> {
    let app_id_clean = app_id.trim();
    let secret_clean = app_secret.trim();
    if app_id_clean.is_empty() || secret_clean.is_empty() {
        return Err("Thiếu App ID hoặc Secret Key của Shopee Open API".to_string());
    }

    let now_ts = Utc::now().timestamp();
    let start_ts = now_ts - days_back.clamp(1, 30) * 86400;
    let gql_query = format!(
        "{{ conversionReport(purchaseTimeStart: {}, purchaseTimeEnd: {}, limit: 50) {{ nodes {{ conversionId conversionStatus totalCommission grossCommission purchaseTime utmContent subIds orders {{ orderId orderStatus items {{ actualAmount itemCommission }} }} }} }} }}",
        start_ts, now_ts
    );

    let payload_json = serde_json::to_string(&ShopeeReportReq { query: gql_query })
        .map_err(|e| format!("Lỗi đóng gói JSON Shopee conversionReport: {}", e))?;

    let sign_payload = format!("{}{}{}{}", app_id_clean, now_ts, payload_json, secret_clean);
    let signature = sha256_hex(sign_payload.as_bytes());
    let auth_header = format!(
        "SHA256 Credential={}, Timestamp={}, Signature={}",
        app_id_clean, now_ts, signature
    );

    let resp = http
        .post("https://open-api.affiliate.shopee.vn/graphql")
        .header("Authorization", auth_header)
        .header("Content-Type", "application/json")
        .body(payload_json)
        .send()
        .await
        .map_err(|e| format!("Lỗi kết nối Shopee Open API conversionReport: {}", e))?;

    let status = resp.status();
    let body_text = resp
        .text()
        .await
        .map_err(|e| format!("Lỗi đọc phản hồi Shopee Open API: {}", e))?;

    if !status.is_success() {
        return Err(format!("Shopee Open API conversionReport trả về HTTP {}", status));
    }

    parse_shopee_conversion_report_json(&body_text)
}

pub fn parse_shopee_conversion_report_json(
    body: &str,
) -> Result<Vec<crate::db::models::TrackedOrder>, String> {
    let root: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| format!("Lỗi parse JSON Shopee conversionReport: {}", e))?;

    if let Some(err_msg) = root
        .get("errors")
        .and_then(|e| e.as_array())
        .and_then(|a| a.first())
        .and_then(|x| x.get("message"))
        .and_then(|m| m.as_str())
    {
        return Err(err_msg.to_string());
    }

    let Some(nodes) = root
        .get("data")
        .and_then(|d| d.get("conversionReport"))
        .and_then(|r| r.get("nodes"))
        .and_then(|n| n.as_array())
    else {
        return Ok(Vec::new());
    };

    let now_rfc = Utc::now().to_rfc3339();
    let mut out = Vec::new();
    for node in nodes {
        let conv_id = node
            .get("conversionId")
            .and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_i64().map(|n| n.to_string())))
            .unwrap_or_default();
        let nested_order_id = node
            .get("orders")
            .and_then(|o| o.as_array())
            .and_then(|a| a.first())
            .and_then(|o| o.get("orderId"))
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();

        let order_id = if !nested_order_id.is_empty() {
            nested_order_id
        } else {
            conv_id
        };
        if order_id.is_empty() {
            continue;
        }

        let status = node
            .get("conversionStatus")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "PENDING".to_string());

        let commission = parse_val_f64(node.get("totalCommission"))
            .or_else(|| parse_val_f64(node.get("grossCommission")))
            .unwrap_or(0.0);

        let mut order_amount = 0.0;
        if let Some(orders_arr) = node.get("orders").and_then(|o| o.as_array()) {
            for ord in orders_arr {
                if let Some(items) = ord.get("items").and_then(|i| i.as_array()) {
                    for it in items {
                        order_amount += parse_val_f64(it.get("actualAmount")).unwrap_or(0.0);
                    }
                }
            }
        }

        let sub_id = node
            .get("subIds")
            .and_then(|s| s.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .filter(|s| !s.is_empty())
            .or_else(|| {
                node.get("utmContent")
                    .and_then(|u| u.as_str())
                    .map(|s| s.trim().to_string())
            })
            .unwrap_or_default();

        let order_time = node
            .get("purchaseTime")
            .and_then(|v| v.as_i64().map(|ts| ts.to_string()).or_else(|| v.as_str().map(str::to_string)))
            .unwrap_or_default();

        out.push(crate::db::models::TrackedOrder {
            order_key: format!("SHOPEE:{}", order_id),
            platform: "Shopee".to_string(),
            order_id,
            status,
            order_amount,
            commission,
            sub_id,
            order_time,
            notified_at: now_rfc.clone(),
        });
    }

    Ok(out)
}

fn parse_val_f64(val: Option<&serde_json::Value>) -> Option<f64> {
    match val? {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn percent_encode_rfc3986(data: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_hex_nist_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_build_direct_an_redir_link_and_validation() {
        assert!(is_valid_shopee_affiliate_id("17375520550"));
        assert!(is_valid_shopee_affiliate_id(" 17375520550 "));
        assert!(!is_valid_shopee_affiliate_id(""));
        assert!(!is_valid_shopee_affiliate_id("1234"));
        assert!(!is_valid_shopee_affiliate_id("17375abc550"));

        let link = build_direct_an_redir_link(
            "https://shopee.vn/product/123/456",
            "17375520550",
            "u1_r0_abcd1234",
        );
        assert_eq!(
            link,
            "https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fproduct%2F123%2F456&affiliate_id=17375520550&sub_id=u1_r0_abcd1234"
        );
    }

    #[test]
    fn test_extract_and_append_social_passthrough_params() {
        let sample_full = "https://s.shopee.vn/2gCo1LuaEU?fb_content_id=Q9abc&encrypted_payload=cM5f_D-Cz&fbclid=76po&cts=1794981997&channel_type=ig&content_source=ig&content_type=REELS&sm_sig_id=QfU_6-xl&utm_source=ignore_me";
        let extracted = extract_social_passthrough_params(sample_full).unwrap();
        assert!(extracted.contains("channel_type=ig"));
        assert!(extracted.contains("sm_sig_id=QfU_6-xl"));
        assert!(!extracted.contains("utm_source="));

        // Kiểm tra nối lên link ngắn s.shopee.vn (chưa có '?')
        let on_short = append_social_params_to_shopee_link(
            "https://s.shopee.vn/2gCo1LuaEU",
            Some(sample_full),
        );
        assert!(on_short.starts_with("https://s.shopee.vn/2gCo1LuaEU?fb_content_id="));

        // Kiểm tra nối lên link an_redir (đã có '?')
        let an_redir = "https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fproduct%2F1%2F2&affiliate_id=17375520550&sub_id=u1";
        let on_redir = append_social_params_to_shopee_link(an_redir, Some(&extracted));
        assert!(on_redir.starts_with(&format!("{}&fb_content_id=", an_redir)));

        // Nếu không chứa sm_sig_id hay encrypted_payload thì từ chối
        assert!(extract_social_passthrough_params("channel_type=ig&fbclid=123").is_none());
    }

    #[test]
    fn test_parse_shopee_conversion_report_json() {
        let json = r#"{
            "data": {
                "conversionReport": {
                    "nodes": [
                        {
                            "conversionId": "CONV777",
                            "conversionStatus": "PENDING",
                            "totalCommission": "14250",
                            "purchaseTime": 1794980000,
                            "subIds": ["u99_r0_1234"],
                            "orders": [
                                {
                                    "orderId": "261118XYZ999",
                                    "orderStatus": "UNPAID",
                                    "items": [
                                        { "actualAmount": "285000", "itemCommission": "14250" }
                                    ]
                                }
                            ]
                        }
                    ]
                }
            }
        }"#;
        let list = parse_shopee_conversion_report_json(json).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].order_key, "SHOPEE:261118XYZ999");
        assert_eq!(list[0].platform, "Shopee");
        assert_eq!(list[0].order_amount, 285000.0);
        assert_eq!(list[0].commission, 14250.0);
        assert_eq!(list[0].sub_id, "u99_r0_1234");
    }
}
