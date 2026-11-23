// Cloudflare Worker Proxy & Trang Đích Bot Telegram 3D "Thế Giới Siêu Sale"
// Hướng dẫn nhanh:
//   1. Đăng nhập Cloudflare -> Workers & Pages -> chọn Worker (VD: thegioisieusale) -> Edit code.
//   2. Xóa code mẫu, dán toàn bộ file này vào -> bấm Deploy.
//   3. Gắn tên miền (VD: thegioisieusale.click) ở tab Domains của Worker.
// Lưu ý: Trang gốc `/` hiển thị Trang Đích Bot Telegram 3D; các đường dẫn hệ thống (/admin, /convert, /health...) luôn trả 404.

const ORIGIN_URL = "https://bot-sale.onrender.com";
const ASSET_FALLBACK_BASE = "https://raw.githubusercontent.com/duonghungfreekst-prog/bot-sale/main/assets";
const BOT_USERNAME = "vvipass_vip_bot";
const BOT_DISPLAY_NAME = "Thế Giới Siêu Sale";

const LANDING_JS = `(function () {
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
})();`;

function renderBotLandingHtml() {
  return `<!DOCTYPE html>
<html lang="vi">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>${BOT_DISPLAY_NAME} — Bot Săn Mã Giảm Giá Tự Động Trên Telegram</title>
  <meta name="description" content="Dán link Shopee, Lazada, Tiki, TikTok Shop hoặc Alibaba vào bot Telegram @${BOT_USERNAME} để nhận link kèm mã giảm giá sâu ngay lập tức.">
  <meta name="theme-color" content="#060d18">
  <link rel="icon" type="image/webp" href="/assets/bot-logo.webp">
  <meta property="og:title" content="${BOT_DISPLAY_NAME} — Trợ Lý Săn Mã &amp; Giỏ Hàng Telegram">
  <meta property="og:description" content="Gửi link sản phẩm vào @${BOT_USERNAME} để tìm mã giảm sâu và mở thẳng ứng dụng mua sắm.">
  <meta property="og:image" content="/assets/bot-logo.webp">
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Be+Vietnam+Pro:wght@400;500;600;700&amp;family=Lexend:wght@600;700;800&amp;display=swap">
  <style>
    :root {
      --color-bg: #060d18;
      --color-surface: #0d1626;
      --color-surface-2: #132036;
      --color-border: rgba(56, 189, 248, 0.18);
      --color-text: #f8fafc;
      --color-text-muted: #cbd5e1;
      --color-cyan: #38bdf8;
      --color-orange: #f97316;
      --color-gold: #fbbf24;
      --color-tg: #0369a1;
      --color-tg-hover: #075985;
      --color-success: #34d399;
      --rx: 0deg;
      --ry: 0deg;
      --gx: 50%;
      --gy: 45%;
      --space-2: 8px;
      --space-3: 12px;
      --space-4: 16px;
      --space-5: 24px;
      --space-6: 32px;
      --radius-md: 14px;
      --radius-lg: 24px;
      --font-sans: "Be Vietnam Pro", system-ui, -apple-system, sans-serif;
      --font-display: "Lexend", "Be Vietnam Pro", system-ui, sans-serif;
    }
    * { box-sizing: border-box; }
    html, body { margin: 0; padding: 0; }
    body {
      min-height: 100vh;
      overflow-x: hidden;
      position: relative;
      background-color: var(--color-bg);
      background-image:
        radial-gradient(rgba(56, 189, 248, 0.08) 1px, transparent 1px),
        radial-gradient(circle at 76% 24%, rgba(56, 189, 248, 0.16), transparent 44%),
        radial-gradient(circle at 18% 22%, rgba(249, 115, 22, 0.14), transparent 42%);
      background-size: 30px 30px, 100% 100%, 100% 100%;
      color: var(--color-text);
      font-family: var(--font-sans);
      font-size: 16px;
      line-height: 1.6;
      display: flex;
      flex-direction: column;
    }
    a:focus-visible {
      outline: 2px solid var(--color-cyan);
      outline-offset: 3px;
    }

    .ambient-bg {
      position: fixed;
      inset: 0;
      pointer-events: none;
      overflow: hidden;
      z-index: 0;
    }
    .ambient-glow {
      position: absolute;
      width: 460px;
      height: 460px;
      border-radius: 50%;
      filter: blur(95px);
      opacity: 0.32;
      animation: ambientPulse 9s ease-in-out infinite alternate;
    }
    .ambient-glow--cyan { top: -110px; right: 4%; background: #0284c7; }
    .ambient-glow--orange { bottom: -120px; left: -60px; background: #f97316; animation-delay: -4.5s; }

    .page {
      position: relative;
      z-index: 1;
      width: 100%;
      max-width: 1140px;
      margin: 0 auto;
      padding: var(--space-5) var(--space-4) var(--space-6);
      flex: 1;
      display: flex;
      flex-direction: column;
      gap: var(--space-6);
    }

    /* Topbar với Logo thật của Bot */
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
      gap: 12px;
      text-decoration: none;
      color: var(--color-text);
      font-family: var(--font-display);
      font-weight: 700;
      font-size: 19px;
    }
    .logo-ring {
      position: relative;
      width: 48px;
      height: 48px;
      border-radius: 50%;
      padding: 2.5px;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      flex-shrink: 0;
    }
    .logo-ring::before {
      content: "";
      position: absolute;
      inset: 0;
      border-radius: 50%;
      background: conic-gradient(from 0deg, #38bdf8, #f97316, #fbbf24, #38bdf8);
      animation: spinRing 6s linear infinite;
    }
    .logo-ring img {
      position: relative;
      z-index: 1;
      width: 100%;
      height: 100%;
      border-radius: 50%;
      object-fit: cover;
      background: var(--color-bg);
      display: block;
    }
    .top-right {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--space-2);
    }
    .status-pill {
      display: inline-flex;
      align-items: center;
      gap: var(--space-2);
      padding: 6px 13px;
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
      box-shadow: 0 0 0 0 rgba(52, 211, 153, 0.7);
      animation: dotPulse 2.2s infinite;
    }
    .handle-pill {
      display: inline-flex;
      align-items: center;
      padding: 6px 12px;
      border-radius: 999px;
      background: var(--color-surface);
      border: 1px solid var(--color-border);
      color: #7dd3fc;
      text-decoration: none;
      font-size: 13px;
      font-weight: 600;
    }
    .handle-pill:hover { border-color: var(--color-cyan); }

    /* Hero 2 cột */
    .hero-grid {
      display: grid;
      grid-template-columns: 1fr;
      gap: var(--space-6);
      align-items: center;
    }
    h1 {
      margin: 0 0 var(--space-3);
      font-family: var(--font-display);
      font-size: clamp(30px, 4.8vw, 46px);
      line-height: 1.14;
      letter-spacing: -0.025em;
    }
    .grad-text {
      display: block;
      background: linear-gradient(95deg, #38bdf8 0%, #fbbf24 52%, #fb923c 100%);
      -webkit-background-clip: text;
      background-clip: text;
      color: transparent;
    }
    .lead {
      margin: 0 0 var(--space-5);
      color: var(--color-text-muted);
      max-width: 52ch;
    }

    .stats-row {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: var(--space-2);
      margin-bottom: var(--space-5);
    }
    .stat-card {
      background: rgba(13, 22, 38, 0.85);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      padding: 12px 10px;
      text-align: center;
    }
    .stat-val {
      display: block;
      font-family: var(--font-display);
      font-size: 18px;
      font-weight: 800;
      color: var(--color-gold);
      line-height: 1.2;
    }
    .stat-lbl {
      display: block;
      font-size: 12px;
      color: var(--color-text-muted);
      margin-top: 2px;
    }

    .btn-tg {
      position: relative;
      overflow: hidden;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: var(--space-3);
      width: 100%;
      min-height: 54px;
      padding: 14px 28px;
      border-radius: var(--radius-md);
      background: var(--color-tg);
      color: #ffffff;
      text-decoration: none;
      font-family: var(--font-display);
      font-size: 16px;
      font-weight: 700;
      border: 1px solid rgba(125, 211, 252, 0.45);
      box-shadow: 0 14px 34px rgba(3, 105, 161, 0.45);
      transition: background-color 180ms ease, transform 180ms ease, box-shadow 180ms ease;
    }
    .btn-tg::after {
      content: "";
      position: absolute;
      inset: 0;
      background: linear-gradient(110deg, transparent 20%, rgba(255, 255, 255, 0.24) 50%, transparent 80%);
      transform: translateX(-120%);
      animation: shimmer 3.8s infinite;
    }
    .btn-tg svg { transition: transform 200ms ease; }
    .btn-tg:hover {
      background: var(--color-tg-hover);
      transform: translateY(-2px);
      box-shadow: 0 18px 40px rgba(3, 105, 161, 0.6);
    }
    .btn-tg:hover svg { transform: translate(2px, -2px); }

    .platforms {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-2);
      margin-top: var(--space-4);
    }
    .chip {
      display: inline-flex;
      align-items: center;
      gap: 7px;
      font-size: 13px;
      font-weight: 600;
      color: var(--color-text-muted);
      background: var(--color-surface);
      border: 1px solid var(--color-border);
      padding: 6px 12px;
      border-radius: 999px;
    }
    .chip-dot { width: 8px; height: 8px; border-radius: 50%; }

    /* Sân khấu 3D gọn gàng cân đối (perspective + preserve-3d + lớp nổi translateZ) */
    .stage-3d {
      position: relative;
      max-width: 440px;
      width: 100%;
      margin: 0 auto;
      perspective: 1100px;
      padding: 14px 6px 26px;
    }
    .holo-floor {
      position: absolute;
      left: 50%;
      bottom: 8px;
      width: 78%;
      height: 110px;
      transform: translateX(-50%) rotateX(74deg);
      border-radius: 50%;
      background: radial-gradient(circle, rgba(56, 189, 248, 0.34), rgba(249, 115, 22, 0.14) 55%, transparent 72%);
      border: 1.5px solid rgba(56, 189, 248, 0.35);
      box-shadow: 0 0 36px rgba(56, 189, 248, 0.25);
      pointer-events: none;
      animation: holoSpin 12s linear infinite;
    }
    .card-3d {
      position: relative;
      transform-style: preserve-3d;
      transform: rotateX(var(--rx)) rotateY(var(--ry));
      transition: transform 220ms cubic-bezier(0.22, 1, 0.36, 1);
      animation: idle3d 7s ease-in-out infinite;
    }
    .stage-3d.is-interacting .card-3d {
      animation: none;
    }
    .hero-frame {
      position: relative;
      border-radius: var(--radius-lg);
      overflow: hidden;
      background: radial-gradient(circle at 50% 45%, rgba(249, 115, 22, 0.16), rgba(6, 13, 24, 0.96) 72%);
      border: 1px solid rgba(56, 189, 248, 0.32);
      box-shadow: 0 28px 64px rgba(0, 0, 0, 0.65), 0 0 42px rgba(56, 189, 248, 0.14);
      transform: translateZ(20px);
    }
    .hero-frame::after {
      content: "";
      position: absolute;
      inset: 0;
      pointer-events: none;
      background: radial-gradient(circle at var(--gx) var(--gy), rgba(255, 255, 255, 0.16), transparent 52%);
    }
    .hero-art {
      display: block;
      width: 100%;
      height: auto;
      aspect-ratio: 1 / 1;
      object-fit: cover;
      transform: scale(1.02);
    }

    /* Các lớp thẻ 3D tách chiều sâu */
    .layer-ticket-3d {
      position: relative;
      z-index: 4;
      margin: 0 0 -20px 10px;
      width: fit-content;
      display: inline-flex;
      align-items: center;
      gap: 10px;
      padding: 8px 14px;
      border-radius: 12px;
      background: rgba(13, 22, 38, 0.92);
      border: 1px dashed rgba(251, 191, 36, 0.72);
      box-shadow: 0 14px 32px rgba(0, 0, 0, 0.55);
      transform: translateZ(56px);
      animation: badgeBob 4.2s ease-in-out infinite;
    }
    .layer-ticket-pct {
      padding: 4px 9px;
      border-radius: 8px;
      background: linear-gradient(135deg, #f97316, #fbbf24);
      color: #060d18;
      font-family: var(--font-display);
      font-size: 13px;
      font-weight: 800;
    }
    .layer-ticket-txt {
      font-size: 12px;
      font-weight: 600;
      color: #fde68a;
    }

    .layer-cart-3d {
      display: none;
      position: absolute;
      top: 14px;
      right: 12px;
      z-index: 4;
      align-items: center;
      gap: 7px;
      padding: 8px 13px;
      border-radius: 999px;
      background: rgba(3, 105, 161, 0.92);
      border: 1px solid rgba(125, 211, 252, 0.55);
      color: #ffffff;
      font-size: 12px;
      font-weight: 700;
      box-shadow: 0 14px 30px rgba(0, 0, 0, 0.5);
      transform: translateZ(64px);
      animation: badgeBob 4.8s ease-in-out infinite reverse;
    }

    .layer-dock-3d {
      position: relative;
      z-index: 5;
      margin: -34px 12px 0;
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      padding: 11px 14px;
      border-radius: 16px;
      background: rgba(13, 22, 38, 0.88);
      backdrop-filter: blur(14px);
      -webkit-backdrop-filter: blur(14px);
      border: 1px solid rgba(56, 189, 248, 0.36);
      box-shadow: 0 18px 36px rgba(0, 0, 0, 0.6);
      text-decoration: none;
      color: var(--color-text);
      transform: translateZ(72px);
      transition: border-color 180ms ease, transform 180ms ease;
    }
    .layer-dock-3d:hover {
      border-color: var(--color-orange);
    }
    .dock-left {
      display: flex;
      align-items: center;
      gap: 10px;
      min-width: 0;
    }
    .dock-left img {
      width: 38px;
      height: 38px;
      border-radius: 50%;
      object-fit: cover;
      border: 1.5px solid var(--color-cyan);
      flex-shrink: 0;
    }
    .dock-title {
      display: block;
      font-family: var(--font-display);
      font-size: 13px;
      font-weight: 700;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .dock-sub {
      display: block;
      font-size: 12px;
      color: #7dd3fc;
    }
    .dock-btn {
      flex-shrink: 0;
      padding: 8px 13px;
      border-radius: 10px;
      background: linear-gradient(95deg, #f97316, #fbbf24);
      color: #060d18;
      font-family: var(--font-display);
      font-size: 12px;
      font-weight: 800;
    }

    /* Dải băng chuyền vé giảm giá */
    .marquee-strip {
      position: relative;
      overflow: hidden;
      border-radius: var(--radius-md);
      background: rgba(13, 22, 38, 0.85);
      border: 1px solid var(--color-border);
      padding: 11px 0;
    }
    .marquee-track {
      display: flex;
      width: max-content;
      gap: var(--space-3);
      animation: marquee 28s linear infinite;
    }
    .marquee-strip:hover .marquee-track { animation-play-state: paused; }
    .marquee-item {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 6px 14px;
      border-radius: 999px;
      background: var(--color-surface-2);
      border: 1px solid var(--color-border);
      font-size: 13px;
      font-weight: 600;
      color: var(--color-text-muted);
      white-space: nowrap;
    }
    .marquee-item strong { color: #fdba74; }

    /* Quy trình 3 bước */
    .steps-grid {
      display: grid;
      grid-template-columns: 1fr;
      gap: var(--space-4);
    }
    .step-card {
      background: rgba(13, 22, 38, 0.88);
      border: 1px solid var(--color-border);
      border-radius: var(--radius-md);
      padding: var(--space-4);
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
      transition: transform 180ms ease, border-color 180ms ease;
    }
    .step-card:hover {
      transform: translateY(-2px);
      border-color: rgba(249, 115, 22, 0.55);
    }
    .step-head { display: flex; align-items: center; gap: var(--space-3); }
    .step-icon {
      width: 38px;
      height: 38px;
      border-radius: 10px;
      background: rgba(249, 115, 22, 0.14);
      border: 1px solid rgba(249, 115, 22, 0.35);
      color: #fdba74;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      flex-shrink: 0;
    }
    .step-card h2 {
      margin: 0;
      font-family: var(--font-display);
      font-size: 16px;
      line-height: 1.3;
    }
    .step-card p {
      margin: 0;
      font-size: 14px;
      color: var(--color-text-muted);
    }

    footer {
      position: relative;
      z-index: 1;
      border-top: 1px solid rgba(56, 189, 248, 0.14);
      text-align: center;
      padding: var(--space-4);
      font-size: 13px;
      color: #94a3b8;
    }

    @keyframes ambientPulse {
      0% { transform: scale(1) translate(0, 0); opacity: 0.25; }
      100% { transform: scale(1.12) translate(16px, -12px); opacity: 0.4; }
    }
    @keyframes spinRing {
      to { transform: rotate(360deg); }
    }
    @keyframes holoSpin {
      0% { transform: translateX(-50%) rotateX(74deg) rotateZ(0deg); }
      100% { transform: translateX(-50%) rotateX(74deg) rotateZ(360deg); }
    }
    @keyframes idle3d {
      0%, 100% { transform: rotateX(2deg) rotateY(-5deg) translateY(0px); }
      50% { transform: rotateX(-3deg) rotateY(5deg) translateY(-8px); }
    }
    @keyframes badgeBob {
      0%, 100% { transform: translateZ(56px) translateY(0px); }
      50% { transform: translateZ(64px) translateY(-6px); }
    }
    @keyframes shimmer {
      0%, 60% { transform: translateX(-120%); }
      100% { transform: translateX(140%); }
    }
    @keyframes dotPulse {
      0% { box-shadow: 0 0 0 0 rgba(52, 211, 153, 0.65); }
      70% { box-shadow: 0 0 0 8px rgba(52, 211, 153, 0); }
      100% { box-shadow: 0 0 0 0 rgba(52, 211, 153, 0); }
    }
    @keyframes marquee {
      0% { transform: translateX(0); }
      100% { transform: translateX(-50%); }
    }

    @media (min-width: 480px) {
      .layer-cart-3d { display: inline-flex; }
    }
    @media (min-width: 640px) {
      .btn-tg { width: auto; }
    }
    @media (min-width: 820px) {
      .page { padding: var(--space-6) var(--space-5); }
      .hero-grid { grid-template-columns: 1fr 1.05fr; gap: var(--space-6); }
      .steps-grid { grid-template-columns: repeat(3, 1fr); }
    }
    @media (prefers-reduced-motion: reduce) {
      *, *::before, *::after {
        animation: none !important;
        transition: none !important;
      }
      .card-3d, .btn-tg:hover, .step-card:hover { transform: none !important; }
    }
  </style>
</head>
<body>
  <div class="ambient-bg" aria-hidden="true">
    <div class="ambient-glow ambient-glow--cyan"></div>
    <div class="ambient-glow ambient-glow--orange"></div>
  </div>

  <div class="page">
    <header class="topbar">
      <a class="brand" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">
        <span class="logo-ring">
          <img src="/assets/bot-logo.webp" alt="Logo ${BOT_DISPLAY_NAME}" width="48" height="48">
        </span>
        <span>${BOT_DISPLAY_NAME}</span>
      </a>
      <div class="top-right">
        <span class="status-pill">
          <span class="dot" aria-hidden="true"></span>
          <span>Bot đang săn mã 24/7</span>
        </span>
        <a class="handle-pill" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">@${BOT_USERNAME}</a>
      </div>
    </header>

    <main class="hero-grid">
      <section>
        <h1>
          Trợ Lý Săn Mã
          <span class="grad-text">${BOT_DISPLAY_NAME}</span>
        </h1>
        <p class="lead">
          Gửi link sản phẩm vào bot Telegram để tự động tìm trong kho voucher giảm sâu đúng shop, gắn ưu đãi vào giỏ hàng và mở thẳng ứng dụng mua sắm chỉ với 1 chạm.
        </p>

        <div class="stats-row" aria-label="Thông số nổi bật">
          <div class="stat-card">
            <span class="stat-val">5+ Sàn</span>
            <span class="stat-lbl">TMĐT Hỗ Trợ</span>
          </div>
          <div class="stat-card">
            <span class="stat-val">5 Link</span>
            <span class="stat-lbl">Đổi Cùng Lúc</span>
          </div>
          <div class="stat-card">
            <span class="stat-val">Miễn Phí</span>
            <span class="stat-lbl">100% Tự Động</span>
          </div>
        </div>

        <a class="btn-tg" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
            <path d="M21.9 4.2c.3-.9-.5-1.7-1.4-1.3L2.8 9.8c-.9.4-.9 1.7.1 2l4.5 1.5 1.7 5.3c.3.8 1.3 1 1.9.4l2.6-2.5 4.9 3.6c.7.5 1.7.1 1.9-.7l3.5-15.2zM9.7 13.7l-.5 3.1-1.1-3.6 9.9-6.2-8.3 6.7z"/>
          </svg>
          <span>Mở Bot Telegram @${BOT_USERNAME}</span>
        </a>

        <div class="platforms" aria-label="Các sàn thương mại điện tử hỗ trợ">
          <span class="chip"><span class="chip-dot" style="background:#f97316" aria-hidden="true"></span>Shopee</span>
          <span class="chip"><span class="chip-dot" style="background:#ec4899" aria-hidden="true"></span>Lazada</span>
          <span class="chip"><span class="chip-dot" style="background:#38bdf8" aria-hidden="true"></span>Tiki</span>
          <span class="chip"><span class="chip-dot" style="background:#f43f5e" aria-hidden="true"></span>TikTok Shop</span>
          <span class="chip"><span class="chip-dot" style="background:#fbbf24" aria-hidden="true"></span>Alibaba</span>
        </div>
      </section>

      <aside class="stage-3d" id="stage3d" aria-label="Minh họa 3D Robot đẩy xe hàng chứa kho mã giảm giá">
        <div class="holo-floor" aria-hidden="true"></div>
        <div class="card-3d">
          <div class="layer-ticket-3d" aria-hidden="true">
            <span class="layer-ticket-pct">-50%</span>
            <span class="layer-ticket-txt">Kho mã cập nhật liên tục</span>
          </div>

          <div class="layer-cart-3d" aria-hidden="true">
            <span>🛒 Tự động áp vào giỏ</span>
          </div>

          <div class="hero-frame">
            <img class="hero-art" src="/assets/bot-cart-3d.webp" alt="Bot Thế Giới Siêu Sale đẩy xe hàng chứa kho mã giảm giá 3D" width="960" height="960">
          </div>

          <a class="layer-dock-3d" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">
            <span class="dock-left">
              <img src="/assets/bot-logo.webp" alt="" width="38" height="38" aria-hidden="true">
              <span>
                <span class="dock-title">${BOT_DISPLAY_NAME}</span>
                <span class="dock-sub">@${BOT_USERNAME} · Bấm gửi link ngay</span>
              </span>
            </span>
            <span class="dock-btn">Săn Mã ↗</span>
          </a>
        </div>
      </aside>
    </main>

    <div class="marquee-strip" aria-hidden="true">
      <div class="marquee-track">
        <span class="marquee-item">🛒 <strong>Shopee Video</strong> Giảm tới 50%</span>
        <span class="marquee-item">⚡ <strong>Back mã</strong> 0H · 9H · 12H · 15H · 20H</span>
        <span class="marquee-item">🎟️ <strong>Lazada</strong> Bonus + Freeship</span>
        <span class="marquee-item">🛍️ <strong>Tiki</strong> Mã gian hàng chính hãng</span>
        <span class="marquee-item">🔥 <strong>TikTok Shop</strong> Deal Live giảm sâu</span>
        <span class="marquee-item">🤖 <strong>Đổi tối đa 5 link</strong> chỉ trong 1 tin nhắn</span>
        <span class="marquee-item">🛒 <strong>Shopee Video</strong> Giảm tới 50%</span>
        <span class="marquee-item">⚡ <strong>Back mã</strong> 0H · 9H · 12H · 15H · 20H</span>
        <span class="marquee-item">🎟️ <strong>Lazada</strong> Bonus + Freeship</span>
        <span class="marquee-item">🛍️ <strong>Tiki</strong> Mã gian hàng chính hãng</span>
        <span class="marquee-item">🔥 <strong>TikTok Shop</strong> Deal Live giảm sâu</span>
        <span class="marquee-item">🤖 <strong>Đổi tối đa 5 link</strong> chỉ trong 1 tin nhắn</span>
      </div>
    </div>

    <section class="steps-grid" aria-label="Quy trình 3 bước săn mã">
      <article class="step-card">
        <div class="step-head">
          <span class="step-icon" aria-hidden="true">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><circle cx="9" cy="20" r="1.5"/><circle cx="19" cy="20" r="1.5"/><path d="M2 3h3l2.4 11.2a2 2 0 0 0 2 1.6h8.8a2 2 0 0 0 2-1.5L22 7H6"/></svg>
          </span>
          <h2>1. Chọn món vào giỏ</h2>
        </div>
        <p>Tìm sản phẩm muốn mua trên Shopee, Lazada, Tiki hoặc TikTok Shop rồi bấm sao chép liên kết.</p>
      </article>

      <article class="step-card">
        <div class="step-head">
          <span class="step-icon" aria-hidden="true">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="7" width="16" height="12" rx="4"/><path d="M12 3v4"/><circle cx="9" cy="13" r="1" fill="currentColor"/><circle cx="15" cy="13" r="1" fill="currentColor"/></svg>
          </span>
          <h2>2. Gửi cho Bot Telegram</h2>
        </div>
        <p>Dán link vào khung chat của <strong>@${BOT_USERNAME}</strong> (hỗ trợ cả bài gom nhiều link một lúc).</p>
      </article>

      <article class="step-card">
        <div class="step-head">
          <span class="step-icon" aria-hidden="true">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 12v6a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-6"/><path d="M4 12a2 2 0 0 1 0-4h16a2 2 0 0 1 0 4"/><path d="M12 4v16"/></svg>
          </span>
          <h2>3. Nhận mã &amp; thanh toán</h2>
        </div>
        <p>Mở link bot trả về trên điện thoại để vào thẳng ứng dụng và áp ngay voucher giảm giá vừa khớp.</p>
      </article>
    </section>
  </div>

  <footer>
    ${BOT_DISPLAY_NAME} © 2026 · Trợ lý săn mã tự động chính thức trên Telegram (@${BOT_USERNAME})
  </footer>
  <script src="/assets/landing.js" defer></script>
</body>
</html>`;
}

async function serveEdgeAsset(request, path) {
  if (path === "/assets/landing.js") {
    return new Response(request.method === "HEAD" ? null : LANDING_JS, {
      status: 200,
      headers: {
        "content-type": "application/javascript; charset=utf-8",
        "cache-control": "public, max-age=3600",
        "x-content-type-options": "nosniff",
      },
    });
  }

  const fileName = path === "/assets/bot-logo.webp" ? "bot-logo.webp" : "bot-cart-3d.webp";
  let assetResp = await fetch(`${ASSET_FALLBACK_BASE}/${fileName}`, {
    cf: { cacheEverything: true, cacheTtl: 86400 },
  }).catch(() => null);

  if (!assetResp || !assetResp.ok) {
    assetResp = await fetch(ORIGIN_URL.replace(/\/+$/, "") + path, {
      headers: { "x-dealx-cloak": "1" },
    }).catch(() => null);
  }

  if (!assetResp || !assetResp.ok) {
    return new Response("Not Found", { status: 404 });
  }

  return new Response(request.method === "HEAD" ? null : assetResp.body, {
    status: 200,
    headers: {
      "content-type": "image/webp",
      "cache-control": "public, max-age=86400",
      "x-content-type-options": "nosniff",
    },
  });
}

export default {
  async fetch(request) {
    const url = new URL(request.url);
    const path = url.pathname;
    const isAllowedMethod = request.method === "GET" || request.method === "HEAD";

    // Trang gốc `/` phục vụ trực tiếp Trang Đích Bot Telegram 3D từ Cloudflare Edge
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
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; img-src 'self' data:",
        },
      });
    }

    // Phục vụ tức thì 2 ảnh 3D WebP & hiệu ứng chuột 3D ngay tại Cloudflare Edge
    if (
      isAllowedMethod &&
      (path === "/assets/landing.js" ||
        path === "/assets/bot-logo.webp" ||
        path === "/assets/bot-cart-3d.webp")
    ) {
      return serveEdgeAsset(request, path);
    }

    // Chỉ cho phép GET/HEAD vào mã shortlink 6-12 ký tự (/{code}, /go/{code}) hoặc /assets/bridge.js
    const isReserved = ["/health", "/convert", "/status", "/metrics", "/robots"].includes(path.toLowerCase());
    const isShortCode = !isReserved && /^\/(?:go\/)?[A-Za-z0-9]{6,12}$/.test(path);
    const isBridgeAsset = path === "/assets/bridge.js";

    // Mọi đường dẫn khác (/admin, /admin/login, /convert, /health, /assets/home.js...) -> 404 Not Found
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
