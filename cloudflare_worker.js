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
  <meta name="description" content="Dán link Shopee, Lazada, Tiki, TikTok Shop hoặc Alibaba vào bot Telegram @${BOT_USERNAME} để nhận link kèm mã giảm giá sâu ngay lập tức.">
  <meta name="theme-color" content="#090d16">
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Be+Vietnam+Pro:wght@400;500;600;700&amp;family=Lexend:wght@600;700;800&amp;display=swap">
  <style>
    :root {
      --color-bg: #090d16;
      --color-surface: #121927;
      --color-surface-2: #192234;
      --color-border: #263248;
      --color-text: #f8fafc;
      --color-text-muted: #cbd5e1;
      --color-accent: #f97316;
      --color-accent-warm: #fbbf24;
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
      --radius-lg: 22px;
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
        radial-gradient(rgba(148, 163, 184, 0.1) 1px, transparent 1px),
        radial-gradient(circle at 84% 14%, rgba(14, 165, 233, 0.2), transparent 44%),
        radial-gradient(circle at 12% 18%, rgba(249, 115, 22, 0.18), transparent 42%);
      background-size: 28px 28px, 100% 100%, 100% 100%;
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

    /* Lớp trang trí nền động: giỏ hàng, voucher -50%, tia sét */
    .ambient-bg {
      position: fixed;
      inset: 0;
      pointer-events: none;
      overflow: hidden;
      z-index: 0;
    }
    .ambient-glow {
      position: absolute;
      width: 440px;
      height: 440px;
      border-radius: 50%;
      filter: blur(80px);
      opacity: 0.35;
      animation: ambientPulse 8s ease-in-out infinite alternate;
    }
    .ambient-glow--orange {
      top: -120px;
      left: -80px;
      background: #f97316;
    }
    .ambient-glow--blue {
      bottom: 5%;
      right: -100px;
      background: #0284c7;
      animation-delay: -4s;
    }
    .float-badge {
      display: none;
      position: absolute;
      align-items: center;
      gap: 6px;
      padding: 8px 13px;
      border-radius: 999px;
      background: rgba(18, 25, 39, 0.78);
      border: 1px solid rgba(251, 146, 60, 0.28);
      color: #fed7aa;
      font-size: 12px;
      font-weight: 700;
      box-shadow: 0 10px 26px rgba(0, 0, 0, 0.35);
      animation: floatDrift 7s ease-in-out infinite;
    }
    .float-badge--cart { top: 14%; left: 4%; }
    .float-badge--pct { top: 18%; right: 4%; animation-delay: -2.2s; color: #a7f3d0; border-color: rgba(52, 211, 153, 0.3); }
    .float-badge--voucher { bottom: 20%; left: 3%; animation-delay: -4.1s; }
    .float-badge--flash { bottom: 18%; right: 5%; animation-delay: -5.4s; color: #fde68a; border-color: rgba(251, 191, 36, 0.35); }

    .page {
      position: relative;
      z-index: 1;
      width: 100%;
      max-width: 1120px;
      margin: 0 auto;
      padding: var(--space-5) var(--space-4) var(--space-6);
      flex: 1;
      display: flex;
      flex-direction: column;
      gap: var(--space-6);
    }

    /* Topbar */
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
      width: 44px;
      height: 44px;
      border-radius: 12px;
      background: linear-gradient(135deg, #f97316, #f59e0b);
      color: #090d16;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      box-shadow: 0 8px 22px rgba(249, 115, 22, 0.35);
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
    .handle-pill:hover {
      border-color: #38bdf8;
    }

    /* Hero 2 cột */
    .hero-grid {
      display: grid;
      grid-template-columns: 1fr;
      gap: var(--space-6);
      align-items: center;
    }
    .flash-pill {
      position: relative;
      overflow: hidden;
      display: inline-flex;
      align-items: center;
      gap: var(--space-2);
      font-size: 13px;
      font-weight: 600;
      color: #fed7aa;
      background: rgba(249, 115, 22, 0.14);
      border: 1px solid rgba(249, 115, 22, 0.4);
      padding: 6px 14px;
      border-radius: 999px;
      margin-bottom: var(--space-4);
    }
    .flash-pill::after {
      content: "";
      position: absolute;
      inset: 0;
      background: linear-gradient(105deg, transparent 25%, rgba(255, 255, 255, 0.22) 50%, transparent 75%);
      transform: translateX(-100%);
      animation: shimmer 4.5s infinite;
    }
    h1 {
      margin: 0 0 var(--space-3);
      font-family: var(--font-display);
      font-size: clamp(30px, 4.8vw, 46px);
      line-height: 1.15;
      letter-spacing: -0.025em;
    }
    .grad-text {
      display: block;
      background: linear-gradient(95deg, #fb923c 0%, #fbbf24 55%, #fdba74 100%);
      -webkit-background-clip: text;
      background-clip: text;
      color: transparent;
    }
    .lead {
      margin: 0 0 var(--space-5);
      color: var(--color-text-muted);
      max-width: 54ch;
    }

    /* 3 ô thống kê glass card */
    .stats-row {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: var(--space-2);
      margin-bottom: var(--space-5);
    }
    .stat-card {
      background: rgba(18, 25, 39, 0.85);
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
      color: #fbbf24;
      line-height: 1.2;
    }
    .stat-lbl {
      display: block;
      font-size: 12px;
      color: var(--color-text-muted);
      margin-top: 2px;
    }

    /* Nút CTA Telegram chính */
    .cta-group {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--space-3);
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
      padding: 14px 26px;
      border-radius: var(--radius-md);
      background: var(--color-tg);
      color: #ffffff;
      text-decoration: none;
      font-family: var(--font-display);
      font-size: 16px;
      font-weight: 700;
      border: 1px solid rgba(125, 211, 252, 0.42);
      box-shadow: 0 14px 32px rgba(3, 105, 161, 0.42);
      transition: background-color 180ms ease, transform 180ms ease, box-shadow 180ms ease;
    }
    .btn-tg::after {
      content: "";
      position: absolute;
      inset: 0;
      background: linear-gradient(110deg, transparent 20%, rgba(255, 255, 255, 0.22) 50%, transparent 80%);
      transform: translateX(-120%);
      animation: shimmer 3.8s infinite;
    }
    .btn-tg svg {
      transition: transform 200ms ease;
    }
    .btn-tg:hover {
      background: var(--color-tg-hover);
      transform: translateY(-2px);
      box-shadow: 0 18px 36px rgba(3, 105, 161, 0.55);
    }
    .btn-tg:hover svg {
      transform: translate(2px, -2px);
    }
    .btn-tg:active {
      transform: translateY(0);
    }

    /* Chip các sàn */
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
    .chip-dot {
      width: 8px;
      height: 8px;
      border-radius: 50%;
    }

    /* Sân khấu Robot + Giỏ hàng + Vé Voucher */
    .stage {
      position: relative;
      padding-top: 18px;
    }
    .cart-mascot {
      position: relative;
      z-index: 3;
      margin: 0 0 -18px auto;
      width: fit-content;
      display: inline-flex;
      align-items: center;
      gap: 10px;
      padding: 8px 14px 8px 10px;
      border-radius: 999px;
      background: linear-gradient(135deg, #f97316, #ea580c);
      color: #090d16;
      font-family: var(--font-display);
      font-size: 13px;
      font-weight: 800;
      box-shadow: 0 12px 26px rgba(249, 115, 22, 0.42);
      animation: cartBob 3.6s ease-in-out infinite;
    }
    .cart-mascot__tag {
      background: #090d16;
      color: #fbbf24;
      padding: 3px 8px;
      border-radius: 999px;
      font-size: 12px;
    }

    .chat-card {
      position: relative;
      z-index: 2;
      background: rgba(18, 25, 39, 0.92);
      border: 1px solid rgba(148, 163, 184, 0.22);
      border-radius: var(--radius-lg);
      overflow: hidden;
      box-shadow: 0 24px 50px rgba(0, 0, 0, 0.55);
    }
    .chat-head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-3);
      padding: var(--space-3) var(--space-4);
      background: var(--color-surface-2);
      border-bottom: 1px solid var(--color-border);
    }
    .chat-user {
      display: flex;
      align-items: center;
      gap: var(--space-3);
    }
    .chat-avatar {
      width: 40px;
      height: 40px;
      border-radius: 12px;
      background: linear-gradient(135deg, #f97316, #f59e0b);
      color: #090d16;
      display: flex;
      align-items: center;
      justify-content: center;
      flex-shrink: 0;
    }
    .chat-name {
      margin: 0;
      font-family: var(--font-display);
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
      max-width: 90%;
      background: #075985;
      color: #ffffff;
      padding: 10px 14px;
      border-radius: 14px 14px 4px 14px;
      font-size: 13px;
      word-break: break-all;
    }
    .typing-row {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      font-size: 12px;
      color: #94a3b8;
      padding-left: 4px;
    }
    .typing-dots {
      display: inline-flex;
      gap: 4px;
    }
    .typing-dots span {
      width: 5px;
      height: 5px;
      border-radius: 50%;
      background: var(--color-accent-warm);
      animation: dotBounce 1.4s infinite ease-in-out;
    }
    .typing-dots span:nth-child(2) { animation-delay: 0.2s; }
    .typing-dots span:nth-child(3) { animation-delay: 0.4s; }

    .msg-bot {
      background: var(--color-surface-2);
      border: 1px solid var(--color-border);
      padding: 14px;
      border-radius: 16px 16px 16px 4px;
      display: grid;
      gap: var(--space-3);
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

    /* Vé giảm giá thiết kế kiểu Ticket có răng cưa */
    .voucher-ticket {
      position: relative;
      display: grid;
      grid-template-columns: 92px 1fr;
      background: #101726;
      border: 1px solid rgba(249, 115, 22, 0.55);
      border-radius: 12px;
      overflow: hidden;
    }
    .voucher-ticket::before,
    .voucher-ticket::after {
      content: "";
      position: absolute;
      top: 50%;
      width: 12px;
      height: 12px;
      border-radius: 50%;
      background: var(--color-surface-2);
      transform: translateY(-50%);
      border: 1px solid rgba(249, 115, 22, 0.55);
    }
    .voucher-ticket::before { left: -7px; }
    .voucher-ticket::after { right: -7px; }
    .ticket-left {
      background: linear-gradient(135deg, #f97316, #ea580c);
      color: #090d16;
      padding: 10px 8px;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      text-align: center;
      border-right: 2px dashed #101726;
    }
    .ticket-pct {
      font-family: var(--font-display);
      font-size: 20px;
      font-weight: 800;
      line-height: 1.05;
    }
    .ticket-cap {
      font-size: 10px;
      font-weight: 700;
      text-transform: uppercase;
      margin-top: 2px;
    }
    .ticket-right {
      padding: 10px 14px;
      display: flex;
      flex-direction: column;
      justify-content: center;
      gap: 3px;
    }
    .ticket-code {
      font-family: var(--font-display);
      font-size: 14px;
      font-weight: 700;
      color: #a7f3d0;
    }
    .ticket-desc {
      font-size: 12px;
      color: var(--color-text-muted);
    }

    .msg-cta {
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 8px;
      min-height: 44px;
      padding: 10px 14px;
      border-radius: 12px;
      background: linear-gradient(95deg, #f97316, #f59e0b);
      color: #090d16;
      text-decoration: none;
      font-family: var(--font-display);
      font-size: 13px;
      font-weight: 800;
      transition: filter 160ms ease, transform 160ms ease;
    }
    .msg-cta:hover {
      filter: brightness(1.06);
      transform: translateY(-1px);
    }

    /* 2 thẻ vé nổi góc dưới sân khấu */
    .floating-tickets {
      display: none;
      position: relative;
      z-index: 3;
      justify-content: space-between;
      margin-top: -14px;
      padding: 0 8px;
      pointer-events: none;
    }
    .mini-ticket {
      padding: 7px 12px;
      border-radius: 10px;
      background: #141d2e;
      border: 1px dashed rgba(52, 211, 153, 0.55);
      color: #a7f3d0;
      font-size: 12px;
      font-weight: 700;
      box-shadow: 0 10px 24px rgba(0, 0, 0, 0.45);
      animation: ticketFloat 5.2s ease-in-out infinite;
    }
    .mini-ticket--warm {
      border-color: rgba(251, 191, 36, 0.55);
      color: #fde68a;
      animation-delay: -2.4s;
    }

    /* Dải băng chuyền vé giảm giá */
    .marquee-strip {
      position: relative;
      overflow: hidden;
      border-radius: var(--radius-md);
      background: rgba(18, 25, 39, 0.85);
      border: 1px solid var(--color-border);
      padding: 11px 0;
    }
    .marquee-track {
      display: flex;
      width: max-content;
      gap: var(--space-3);
      animation: marquee 28s linear infinite;
    }
    .marquee-strip:hover .marquee-track {
      animation-play-state: paused;
    }
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
    .marquee-item strong {
      color: #fdba74;
    }

    /* Quy trình 3 bước */
    .steps-grid {
      display: grid;
      grid-template-columns: 1fr;
      gap: var(--space-4);
    }
    .step-card {
      background: rgba(18, 25, 39, 0.88);
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
      border-color: rgba(249, 115, 22, 0.5);
    }
    .step-head {
      display: flex;
      align-items: center;
      gap: var(--space-3);
    }
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
      border-top: 1px solid rgba(38, 50, 72, 0.7);
      text-align: center;
      padding: var(--space-4);
      font-size: 13px;
      color: #94a3b8;
    }

    @keyframes ambientPulse {
      0% { transform: scale(1) translate(0, 0); opacity: 0.28; }
      100% { transform: scale(1.12) translate(16px, -12px); opacity: 0.42; }
    }
    @keyframes floatDrift {
      0%, 100% { transform: translateY(0px) rotate(-1deg); }
      50% { transform: translateY(-10px) rotate(1.5deg); }
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
    @keyframes cartBob {
      0%, 100% { transform: translateY(0px); }
      50% { transform: translateY(-6px); }
    }
    @keyframes dotBounce {
      0%, 80%, 100% { transform: translateY(0); opacity: 0.45; }
      40% { transform: translateY(-4px); opacity: 1; }
    }
    @keyframes ticketFloat {
      0%, 100% { transform: translateY(0px) rotate(-2deg); }
      50% { transform: translateY(-5px) rotate(1.5deg); }
    }
    @keyframes marquee {
      0% { transform: translateX(0); }
      100% { transform: translateX(-50%); }
    }

    @media (min-width: 480px) {
      .floating-tickets { display: flex; }
    }
    @media (min-width: 640px) {
      .btn-tg { width: auto; }
    }
    @media (min-width: 768px) {
      .page { padding: var(--space-6) var(--space-5); }
      .hero-grid { grid-template-columns: 1.08fr 0.92fr; gap: var(--space-6); }
      .steps-grid { grid-template-columns: repeat(3, 1fr); }
    }
    @media (min-width: 1220px) {
      .float-badge { display: inline-flex; }
    }
    @media (prefers-reduced-motion: reduce) {
      *, *::before, *::after {
        animation: none !important;
        transition: none !important;
      }
      .btn-tg:hover, .msg-cta:hover, .step-card:hover { transform: none !important; }
    }
  </style>
</head>
<body>
  <div class="ambient-bg" aria-hidden="true">
    <div class="ambient-glow ambient-glow--orange"></div>
    <div class="ambient-glow ambient-glow--blue"></div>
    <div class="float-badge float-badge--cart">
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><circle cx="9" cy="21" r="1"/><circle cx="20" cy="21" r="1"/><path d="M1 1h4l2.7 13.4a2 2 0 0 0 2 1.6h9.7a2 2 0 0 0 2-1.6L23 6H6"/></svg>
      <span>Giỏ Hàng Ưu Đãi</span>
    </div>
    <div class="float-badge float-badge--pct">🎟️ Voucher -50%</div>
    <div class="float-badge float-badge--voucher">🛍️ Tự Động Áp Mã</div>
    <div class="float-badge float-badge--flash">⚡ Khung 0H · 12H · 20H</div>
  </div>

  <div class="page">
    <header class="topbar">
      <a class="brand" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">
        <span class="brand-mark" aria-hidden="true">
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
            <rect x="4" y="7" width="16" height="12" rx="4"/>
            <path d="M12 3v4"/><circle cx="9" cy="13" r="1.2" fill="currentColor"/><circle cx="15" cy="13" r="1.2" fill="currentColor"/>
          </svg>
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
        <span class="flash-pill">⚡ Bắt mã tự động khung 0H · 9H · 12H · 15H · 20H</span>
        <h1>
          Trợ Lý Săn Mã
          <span class="grad-text">${BOT_DISPLAY_NAME}</span>
        </h1>
        <p class="lead">
          Gửi link sản phẩm vào bot Telegram để tự động tìm voucher giảm sâu đúng shop, gắn sẵn ưu đãi vào giỏ hàng và mở thẳng ứng dụng mua sắm chỉ với 1 chạm.
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

        <div class="cta-group">
          <a class="btn-tg" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path d="M21.9 4.2c.3-.9-.5-1.7-1.4-1.3L2.8 9.8c-.9.4-.9 1.7.1 2l4.5 1.5 1.7 5.3c.3.8 1.3 1 1.9.4l2.6-2.5 4.9 3.6c.7.5 1.7.1 1.9-.7l3.5-15.2zM9.7 13.7l-.5 3.1-1.1-3.6 9.9-6.2-8.3 6.7z"/>
            </svg>
            <span>Mở Bot Telegram @${BOT_USERNAME}</span>
          </a>
        </div>

        <div class="platforms" aria-label="Các sàn thương mại điện tử hỗ trợ">
          <span class="chip"><span class="chip-dot" style="background:#f97316" aria-hidden="true"></span>Shopee</span>
          <span class="chip"><span class="chip-dot" style="background:#ec4899" aria-hidden="true"></span>Lazada</span>
          <span class="chip"><span class="chip-dot" style="background:#38bdf8" aria-hidden="true"></span>Tiki</span>
          <span class="chip"><span class="chip-dot" style="background:#f43f5e" aria-hidden="true"></span>TikTok Shop</span>
          <span class="chip"><span class="chip-dot" style="background:#fbbf24" aria-hidden="true"></span>Alibaba</span>
        </div>
      </section>

      <aside class="stage" aria-label="Minh họa bot xử lý giỏ hàng và mã giảm giá">
        <div class="cart-mascot" aria-hidden="true">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.3" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="9" cy="20" r="1.5"/><circle cx="19" cy="20" r="1.5"/>
            <path d="M2 3h3l2.4 11.2a2 2 0 0 0 2 1.6h8.8a2 2 0 0 0 2-1.5L22 7H6"/>
          </svg>
          <span>Bot Săn Giỏ Hàng</span>
          <span class="cart-mascot__tag">-50%</span>
        </div>

        <div class="chat-card">
          <div class="chat-head">
            <div class="chat-user">
              <div class="chat-avatar" aria-hidden="true">
                <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
                  <rect x="4" y="7" width="16" height="12" rx="4"/>
                  <path d="M12 3v4"/><circle cx="9" cy="13" r="1.2" fill="currentColor"/><circle cx="15" cy="13" r="1.2" fill="currentColor"/>
                </svg>
              </div>
              <div>
                <p class="chat-name">${BOT_DISPLAY_NAME}</p>
                <p class="chat-sub">@${BOT_USERNAME} · bot trực tuyến</p>
              </div>
            </div>
            <span style="font-size:12px;color:#34d399;font-weight:700;">● Sẵn sàng</span>
          </div>

          <div class="chat-body">
            <div class="msg-user" aria-hidden="true">https://shopee.vn/san-pham-hot</div>
            <div class="typing-row" aria-hidden="true">
              <span>🤖 Bot đang quét mã &amp; gắn giỏ hàng</span>
              <span class="typing-dots"><span></span><span></span><span></span></span>
            </div>
            <div class="msg-bot">
              <span class="msg-tag" aria-hidden="true">✨ Khớp ưu đãi thành công · Shopee</span>
              <span class="msg-title" aria-hidden="true">Tai Nghe Bluetooth Chống Ồn Pro</span>
              <div class="voucher-ticket" aria-hidden="true">
                <div class="ticket-left">
                  <span class="ticket-pct">-50%</span>
                  <span class="ticket-cap">Tối đa 100K</span>
                </div>
                <div class="ticket-right">
                  <span class="ticket-code">🎟️ SHOPGIAM50</span>
                  <span class="ticket-desc">Áp chung Freeship Xtra &amp; hoàn xu</span>
                </div>
              </div>
              <a class="msg-cta" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <circle cx="9" cy="20" r="1.5"/><circle cx="19" cy="20" r="1.5"/>
                  <path d="M2 3h3l2.4 11.2a2 2 0 0 0 2 1.6h8.8a2 2 0 0 0 2-1.5L22 7H6"/>
                </svg>
                <span>Mở App &amp; Áp Mã Vào Giỏ Ngay</span>
              </a>
            </div>
          </div>
        </div>

        <div class="floating-tickets" aria-hidden="true">
          <span class="mini-ticket">🎟️ VOUCHER -50%</span>
          <span class="mini-ticket mini-ticket--warm">🛒 FREESHIP 0Đ</span>
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
