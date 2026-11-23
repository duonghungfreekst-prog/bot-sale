// Cloudflare Worker Proxy & Trang Đích Bot Telegram "Thế Giới Siêu Sale"
// Hướng dẫn nhanh:
//   1. Đăng nhập Cloudflare -> Workers & Pages -> chọn Worker (VD: thegioisieusale) -> Edit code.
//   2. Xóa code mẫu, dán toàn bộ file này vào -> bấm Deploy.
//   3. Gắn tên miền (VD: thegioisieusale.click) ở tab Domains của Worker.
// Lưu ý: Trang gốc `/` hiển thị Trang Đích Bot Telegram; các đường dẫn hệ thống (/admin, /convert, /health...) luôn trả 404.

const ORIGIN_URL = "https://bot-sale.onrender.com";
const BOT_USERNAME = "vvipass_vip_bot";
const BOT_DISPLAY_NAME = "Thế Giới Siêu Sale";

function renderBotLandingHtml() {
  return `<!DOCTYPE html>
<html lang="vi">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>${BOT_DISPLAY_NAME} — Bot Săn Mã Giảm Giá Tự Động Trên Telegram</title>
  <meta name="description" content="Dán link Shopee, Lazada, Tiki, TikTok Shop hoặc Alibaba vào bot Telegram @${BOT_USERNAME} để nhận link kèm mã giảm giá phù hợp ngay lập tức.">
  <meta name="theme-color" content="#0b0f18">
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Be+Vietnam+Pro:wght@400;500;600;700&amp;family=Lexend:wght@600;700;800&amp;display=swap">
  <style>
    :root {
      --color-bg: #0b0f18;
      --color-surface: #131a27;
      --color-surface-2: #1a2335;
      --color-border: #283349;
      --color-text: #f8fafc;
      --color-text-muted: #cbd5e1;
      --color-accent: #f97316;
      --color-tg: #0369a1;
      --color-tg-hover: #075985;
      --color-success: #34d399;
      --space-1: 4px;
      --space-2: 8px;
      --space-3: 12px;
      --space-4: 16px;
      --space-5: 24px;
      --space-6: 32px;
      --radius-sm: 10px;
      --radius-md: 14px;
      --radius-lg: 20px;
      --font-sans: "Be Vietnam Pro", system-ui, -apple-system, sans-serif;
      --font-display: "Lexend", "Be Vietnam Pro", system-ui, sans-serif;
    }
    * { box-sizing: border-box; }
    html, body { margin: 0; padding: 0; }
    body {
      min-height: 100vh;
      background:
        radial-gradient(circle at 85% 10%, rgba(3, 105, 161, 0.22), transparent 45%),
        radial-gradient(circle at 10% 15%, rgba(249, 115, 22, 0.14), transparent 40%),
        var(--color-bg);
      color: var(--color-text);
      font-family: var(--font-sans);
      font-size: 16px;
      line-height: 1.6;
      display: flex;
      flex-direction: column;
    }
    a:focus-visible {
      outline: 2px solid #fb923c;
      outline-offset: 3px;
    }
    .wrap {
      width: 100%;
      max-width: 980px;
      margin: 0 auto;
      padding: var(--space-5) var(--space-4);
      flex: 1;
      display: flex;
      flex-direction: column;
      justify-content: center;
      gap: var(--space-5);
    }
    .topbar {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-3);
    }
    .brand {
      display: inline-flex;
      align-items: center;
      gap: var(--space-3);
      text-decoration: none;
      color: var(--color-text);
      font-family: var(--font-display);
      font-weight: 700;
      font-size: 18px;
    }
    .brand-mark {
      width: 40px;
      height: 40px;
      border-radius: var(--radius-sm);
      background: linear-gradient(135deg, #f97316, #ea580c);
      color: #0b0f18;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      font-weight: 800;
      font-size: 15px;
      letter-spacing: 0.02em;
    }
    .status-pill {
      display: inline-flex;
      align-items: center;
      gap: var(--space-2);
      padding: 6px 12px;
      border-radius: 999px;
      background: rgba(52, 211, 153, 0.12);
      border: 1px solid rgba(52, 211, 153, 0.35);
      color: #a7f3d0;
      font-size: 13px;
      font-weight: 600;
    }
    .dot {
      width: 8px;
      height: 8px;
      border-radius: 50%;
      background: var(--color-success);
    }
    .grid {
      display: grid;
      grid-template-columns: 1fr;
      gap: var(--space-5);
      align-items: center;
    }
    .eyebrow {
      display: inline-block;
      font-size: 13px;
      font-weight: 600;
      color: #fdba74;
      background: rgba(249, 115, 22, 0.14);
      border: 1px solid rgba(249, 115, 22, 0.35);
      padding: 4px 12px;
      border-radius: 999px;
      margin-bottom: var(--space-3);
    }
    h1 {
      margin: 0 0 var(--space-3);
      font-family: var(--font-display);
      font-size: clamp(28px, 4.6vw, 40px);
      line-height: 1.18;
      letter-spacing: -0.02em;
    }
    .bot-handle {
      color: #7dd3fc;
      font-weight: 600;
      font-size: 15px;
      margin: 0 0 var(--space-3);
    }
    .lead {
      margin: 0 0 var(--space-4);
      color: var(--color-text-muted);
      max-width: 52ch;
    }
    .btn-tg {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: var(--space-3);
      width: 100%;
      min-height: 52px;
      padding: 14px 24px;
      border-radius: var(--radius-md);
      background: var(--color-tg);
      color: #ffffff;
      text-decoration: none;
      font-family: var(--font-display);
      font-size: 16px;
      font-weight: 700;
      border: 1px solid rgba(125, 211, 252, 0.35);
      box-shadow: 0 12px 28px rgba(3, 105, 161, 0.35);
      transition: background-color 180ms ease, transform 180ms ease;
    }
    .btn-tg:hover {
      background: var(--color-tg-hover);
      transform: translateY(-1px);
    }
    .btn-tg:active {
      transform: translateY(0);
    }
    .platforms {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
      margin-top: var(--space-4);
    }
    .chip {
      font-size: 13px;
      font-weight: 500;
      color: var(--color-text-muted);
      background: var(--color-surface);
      border: 1px solid var(--color-border);
      padding: 5px 11px;
      border-radius: 999px;
    }
    .steps {
      list-style: none;
      padding: 0;
      margin: var(--space-4) 0 0;
      display: grid;
      gap: var(--space-2);
    }
    .steps li {
      display: flex;
      align-items: flex-start;
      gap: var(--space-3);
      font-size: 14px;
      color: var(--color-text-muted);
    }
    .step-num {
      flex: 0 0 24px;
      height: 24px;
      border-radius: 50%;
      background: var(--color-surface-2);
      border: 1px solid var(--color-border);
      color: #fdba74;
      font-size: 12px;
      font-weight: 700;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      margin-top: 2px;
    }
    .chat-card {
      background: var(--color-surface);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-lg);
      overflow: hidden;
      box-shadow: 0 18px 40px rgba(0, 0, 0, 0.4);
    }
    .chat-head {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      padding: var(--space-3) var(--space-4);
      background: var(--color-surface-2);
      border-bottom: 1px solid var(--color-border);
    }
    .chat-avatar {
      width: 36px;
      height: 36px;
      border-radius: 50%;
      background: linear-gradient(135deg, #f97316, #ea580c);
      color: #0b0f18;
      font-family: var(--font-display);
      font-weight: 800;
      font-size: 13px;
      display: flex;
      align-items: center;
      justify-content: center;
    }
    .chat-name {
      margin: 0;
      font-size: 14px;
      font-weight: 700;
    }
    .chat-sub {
      margin: 0;
      font-size: 12px;
      color: #7dd3fc;
    }
    .chat-body {
      padding: var(--space-4);
      display: grid;
      gap: var(--space-3);
    }
    .msg-user {
      justify-self: end;
      max-width: 88%;
      background: #075985;
      color: #ffffff;
      padding: 10px 14px;
      border-radius: 14px 14px 4px 14px;
      font-size: 13px;
      word-break: break-all;
    }
    .msg-bot {
      justify-self: start;
      width: 100%;
      background: var(--color-surface-2);
      border: 1px solid var(--color-border);
      padding: 14px;
      border-radius: 14px 14px 14px 4px;
      display: grid;
      gap: var(--space-2);
    }
    .msg-tag {
      font-size: 12px;
      font-weight: 700;
      color: #fdba74;
    }
    .msg-title {
      font-size: 14px;
      font-weight: 600;
      color: var(--color-text);
    }
    .msg-voucher {
      font-size: 13px;
      color: #a7f3d0;
      background: rgba(52, 211, 153, 0.1);
      border: 1px solid rgba(52, 211, 153, 0.28);
      border-radius: 8px;
      padding: 6px 10px;
    }
    .msg-cta {
      display: block;
      text-align: center;
      padding: 10px 12px;
      border-radius: 10px;
      background: var(--color-accent);
      color: #0b0f18;
      font-size: 13px;
      font-weight: 700;
    }
    footer {
      border-top: 1px solid rgba(40, 51, 73, 0.65);
      text-align: center;
      padding: var(--space-4);
      font-size: 13px;
      color: #94a3b8;
    }
    @media (min-width: 640px) {
      .btn-tg { width: auto; }
    }
    @media (min-width: 768px) {
      .wrap { padding: var(--space-6) var(--space-5); }
      .grid {
        grid-template-columns: 1.15fr 0.85fr;
        gap: var(--space-6);
      }
    }
    @media (prefers-reduced-motion: reduce) {
      .btn-tg { transition: none; transform: none !important; }
    }
  </style>
</head>
<body>
  <div class="wrap">
    <header class="topbar">
      <a class="brand" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">
        <span class="brand-mark" aria-hidden="true">TG</span>
        <span>${BOT_DISPLAY_NAME}</span>
      </a>
      <span class="status-pill">
        <span class="dot" aria-hidden="true"></span>
        <span>Bot Telegram hoạt động 24/7</span>
      </span>
    </header>

    <main class="grid">
      <section>
        <span class="eyebrow">Trợ lý săn mã &amp; chuyển link trực tiếp</span>
        <h1>${BOT_DISPLAY_NAME}</h1>
        <p class="bot-handle">@${BOT_USERNAME} trên Telegram</p>
        <p class="lead">
          Chỉ cần gửi link sản phẩm vào bot Telegram, hệ thống tự động tìm mã giảm giá khớp đúng shop/ngành hàng và tạo link mở thẳng ứng dụng mua sắm.
        </p>

        <a class="btn-tg" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <path d="M21.9 4.2c.3-.9-.5-1.7-1.4-1.3L2.8 9.8c-.9.4-.9 1.7.1 2l4.5 1.5 1.7 5.3c.3.8 1.3 1 1.9.4l2.6-2.5 4.9 3.6c.7.5 1.7.1 1.9-.7l3.5-15.2zM9.7 13.7l-.5 3.1-1.1-3.6 9.9-6.2-8.3 6.7z"/>
          </svg>
          <span>Mở Bot Telegram Ngay</span>
        </a>

        <div class="platforms" aria-label="Các sàn thương mại điện tử hỗ trợ">
          <span class="chip">Shopee</span>
          <span class="chip">Lazada</span>
          <span class="chip">Tiki</span>
          <span class="chip">TikTok Shop</span>
          <span class="chip">Alibaba</span>
        </div>

        <ol class="steps">
          <li>
            <span class="step-num" aria-hidden="true">1</span>
            <span>Sao chép liên kết sản phẩm từ app Shopee, Lazada, Tiki hoặc TikTok Shop.</span>
          </li>
          <li>
            <span class="step-num" aria-hidden="true">2</span>
            <span>Dán vào khung chat của bot <strong>@${BOT_USERNAME}</strong> (hỗ trợ tối đa 5 link cùng lúc).</span>
          </li>
          <li>
            <span class="step-num" aria-hidden="true">3</span>
            <span>Bấm nút mở ứng dụng &amp; lưu mã giảm giá đi kèm trước khi thanh toán.</span>
          </li>
        </ol>
      </section>

      <aside class="chat-card" aria-label="Minh họa cách bot hoạt động">
        <div class="chat-head">
          <div class="chat-avatar" aria-hidden="true">TG</div>
          <div>
            <p class="chat-name">${BOT_DISPLAY_NAME}</p>
            <p class="chat-sub">@${BOT_USERNAME} · bot</p>
          </div>
        </div>
        <div class="chat-body" aria-hidden="true">
          <div class="msg-user">https://shopee.vn/san-pham-mau</div>
          <div class="msg-bot">
            <span class="msg-tag">✨ Đã khớp ưu đãi · Shopee</span>
            <span class="msg-title">Tai Nghe Bluetooth Chống Ồn Pro</span>
            <span class="msg-voucher">🎟️ Mã SHOPGIAM20 · Giảm 20% tối đa 50K</span>
            <span class="msg-cta">🛍️ Mở Shopee &amp; Áp Mã Ngay ↗</span>
          </div>
        </div>
      </aside>
    </main>
  </div>

  <footer>
    ${BOT_DISPLAY_NAME} © 2026 · Kênh hỗ trợ &amp; săn mã chính thức trên Telegram (@${BOT_USERNAME})
  </footer>
</body>
</html>`;
}

export default {
  async fetch(request) {
    const url = new URL(request.url);
    const path = url.pathname;
    const isAllowedMethod = request.method === "GET" || request.method === "HEAD";

    // Trang gốc `/` phục vụ trực tiếp Trang Đích Bot Telegram từ Cloudflare Edge (không lộ Render/Admin)
    if (isAllowedMethod && path === "/") {
      return new Response(request.method === "HEAD" ? null : renderBotLandingHtml(), {
        status: 200,
        headers: {
          "content-type": "text/html; charset=utf-8",
          "cache-control": "public, max-age=300",
          "x-content-type-options": "nosniff",
          "x-frame-options": "DENY",
          "referrer-policy": "no-referrer",
          "content-security-policy":
            "default-src 'none'; style-src 'unsafe-inline' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data:",
        },
      });
    }

    // Chỉ cho phép GET/HEAD vào mã shortlink 6-12 ký tự (/{code}, /go/{code}) hoặc /assets/bridge.js
    const isReserved = ["/health", "/convert", "/status", "/metrics", "/robots"].includes(path.toLowerCase());
    const isShortCode = !isReserved && /^\/(?:go\/)?[A-Za-z0-9]{6,12}$/.test(path);
    const isBridgeAsset = path === "/assets/bridge.js";

    // Mọi đường dẫn khác (/admin, /admin/login, /convert, /health, /robots.txt...) -> 404 Not Found
    if (!isAllowedMethod || (!isShortCode && !isBridgeAsset)) {
      return new Response("Not Found", {
        status: 404,
        headers: {
          "content-type": "text/plain; charset=utf-8",
          "x-robots-tag": "noindex, nofollow",
          "cache-control": "no-store",
        },
      });
    }

    const forwardHeaders = new Headers();
    const passKeys = [
      "user-agent",
      "accept",
      "accept-language",
      "sec-ch-ua",
      "sec-ch-ua-mobile",
      "sec-ch-ua-platform",
    ];
    for (const key of passKeys) {
      const val = request.headers.get(key);
      if (val) {
        forwardHeaders.set(key, val);
      }
    }
    // Báo hiệu backend bật chế độ Shortlink Cloak ở cả tầng Origin
    forwardHeaders.set("x-dealx-cloak", "1");

    try {
      const originUrl = ORIGIN_URL.replace(/\/+$/, "") + path + url.search;
      const originResp = await fetch(originUrl, {
        method: request.method,
        headers: forwardHeaders,
        redirect: "manual",
      });

      const respHeaders = new Headers(originResp.headers);
      respHeaders.delete("x-render-origin-server");
      respHeaders.delete("rndr-id");
      respHeaders.delete("server");
      respHeaders.delete("x-powered-by");

      return new Response(originResp.body, {
        status: originResp.status,
        statusText: originResp.statusText,
        headers: respHeaders,
      });
    } catch (_err) {
      return new Response("Bad Gateway", {
        status: 502,
        headers: { "content-type": "text/plain; charset=utf-8" },
      });
    }
  },
};
