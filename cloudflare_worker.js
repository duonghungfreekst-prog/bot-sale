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
    stage.style.setProperty("--rx", ((0.5 - py) * 15).toFixed(2) + "deg");
    stage.style.setProperty("--ry", ((px - 0.5) * 18).toFixed(2) + "deg");
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
  <title>${BOT_DISPLAY_NAME} — Săn Mã Giảm Sâu Tự Động Trên Telegram</title>
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
      --color-border: rgba(56, 189, 248, 0.22);
      --color-text: #f8fafc;
      --color-text-muted: #dbe4f0;
      --color-cyan: #38bdf8;
      --color-orange: #f97316;
      --color-gold: #fde047;
      --color-emerald: #34d399;
      --rx: 0deg;
      --ry: 0deg;
      --gx: 50%;
      --gy: 45%;
      --space-2: 8px;
      --space-3: 12px;
      --space-4: 16px;
      --space-5: 24px;
      --space-6: 32px;
      --radius-md: 16px;
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
        radial-gradient(rgba(56, 189, 248, 0.11) 1px, transparent 1px),
        radial-gradient(circle at 74% 28%, rgba(56, 189, 248, 0.22), transparent 46%),
        radial-gradient(circle at 20% 24%, rgba(249, 115, 22, 0.2), transparent 44%);
      background-size: 28px 28px, 100% 100%, 100% 100%;
      color: var(--color-text);
      font-family: var(--font-sans);
      font-size: 16px;
      line-height: 1.6;
      display: flex;
      flex-direction: column;
    }
    a:focus-visible {
      outline: 2px solid var(--color-gold);
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
      width: 520px;
      height: 520px;
      border-radius: 50%;
      filter: blur(110px);
      opacity: 0.36;
      animation: ambientPulse 9s ease-in-out infinite alternate;
    }
    .ambient-glow--cyan { top: -140px; right: 2%; background: #0ea5e9; }
    .ambient-glow--orange { bottom: -140px; left: -60px; background: #ff5e00; animation-delay: -4.5s; }

    .page {
      position: relative;
      z-index: 1;
      width: 100%;
      max-width: 1160px;
      margin: 0 auto;
      padding: var(--space-4) var(--space-4) var(--space-6);
      flex: 1;
      display: flex;
      flex-direction: column;
      gap: var(--space-5);
    }

    /* Glass Navbar nổi bật */
    .topbar {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-3);
      padding: 10px 16px;
      border-radius: 18px;
      background: rgba(13, 22, 38, 0.78);
      backdrop-filter: blur(16px);
      -webkit-backdrop-filter: blur(16px);
      border: 1px solid rgba(56, 189, 248, 0.26);
      box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
    }
    .brand {
      display: inline-flex;
      align-items: center;
      gap: 12px;
      text-decoration: none;
      color: var(--color-text);
      font-family: var(--font-display);
      font-weight: 800;
      font-size: 19px;
      letter-spacing: -0.01em;
    }
    .logo-ring {
      position: relative;
      width: 52px;
      height: 52px;
      border-radius: 50%;
      padding: 3px;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      flex-shrink: 0;
      box-shadow: 0 0 22px rgba(56, 189, 248, 0.45);
    }
    .logo-ring::before {
      content: "";
      position: absolute;
      inset: 0;
      border-radius: 50%;
      background: conic-gradient(from 0deg, #38bdf8, #ff5e00, #fde047, #38bdf8);
      animation: spinRing 4.8s linear infinite;
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
      padding: 7px 14px;
      border-radius: 999px;
      background: rgba(52, 211, 153, 0.14);
      border: 1px solid rgba(52, 211, 153, 0.42);
      color: #a7f3d0;
      font-size: 13px;
      font-weight: 600;
    }
    .dot {
      width: 8px;
      height: 8px;
      border-radius: 50%;
      background: var(--color-emerald);
      box-shadow: 0 0 0 0 rgba(52, 211, 153, 0.7);
      animation: dotPulse 2s infinite;
    }
    .handle-pill {
      display: inline-flex;
      align-items: center;
      padding: 7px 14px;
      border-radius: 999px;
      background: rgba(56, 189, 248, 0.12);
      border: 1px solid rgba(56, 189, 248, 0.35);
      color: #bae6fd;
      text-decoration: none;
      font-size: 13px;
      font-weight: 700;
      transition: background-color 160ms ease, border-color 160ms ease;
    }
    .handle-pill:hover {
      background: rgba(56, 189, 248, 0.22);
      border-color: var(--color-cyan);
    }

    /* Hero 2 cột */
    .hero-grid {
      display: grid;
      grid-template-columns: 1fr;
      gap: var(--space-5);
      align-items: center;
    }
    h1 {
      margin: 0 0 var(--space-3);
      font-family: var(--font-display);
      font-size: clamp(34px, 5.2vw, 54px);
      line-height: 1.1;
      letter-spacing: -0.03em;
      color: #ffffff;
    }
    .grad-text {
      display: block;
      margin-top: 4px;
      background: linear-gradient(95deg, #38bdf8 0%, #fde047 50%, #ff7a18 100%);
      -webkit-background-clip: text;
      background-clip: text;
      color: transparent;
      filter: drop-shadow(0 0 28px rgba(249, 115, 22, 0.42));
    }
    .lead {
      margin: 0 0 var(--space-4);
      color: var(--color-text-muted);
      font-size: 16px;
      max-width: 52ch;
    }

    /* 3 ô thống kê có đường nhấn màu riêng */
    .stats-row {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: var(--space-2);
      margin-bottom: var(--space-5);
    }
    .stat-card {
      position: relative;
      overflow: hidden;
      background: rgba(13, 22, 38, 0.9);
      border: 1px solid rgba(148, 163, 184, 0.2);
      border-top: 3px solid var(--accent-line, #f97316);
      border-radius: var(--radius-md);
      padding: 13px 10px;
      text-align: center;
      box-shadow: 0 10px 24px rgba(0, 0, 0, 0.35);
    }
    .stat-card--orange { --accent-line: #f97316; }
    .stat-card--cyan { --accent-line: #38bdf8; }
    .stat-card--emerald { --accent-line: #34d399; }
    .stat-val {
      display: block;
      font-family: var(--font-display);
      font-size: 22px;
      font-weight: 800;
      color: #ffffff;
      line-height: 1.15;
    }
    .stat-card--orange .stat-val { color: #fb923c; }
    .stat-card--cyan .stat-val { color: #7dd3fc; }
    .stat-card--emerald .stat-val { color: #6ee7b7; }
    .stat-lbl {
      display: block;
      font-size: 12px;
      font-weight: 500;
      color: var(--color-text-muted);
      margin-top: 3px;
    }

    /* Nhóm nút CTA siêu nổi bật */
    .cta-row {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--space-3);
    }
    .btn-hot {
      position: relative;
      overflow: hidden;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: 10px;
      width: 100%;
      min-height: 54px;
      padding: 14px 26px;
      border-radius: var(--radius-md);
      background: linear-gradient(100deg, #ff5e00 0%, #f97316 48%, #fbbf24 100%);
      color: #060d18;
      text-decoration: none;
      font-family: var(--font-display);
      font-size: 16px;
      font-weight: 800;
      box-shadow: 0 12px 34px rgba(249, 115, 22, 0.48), 0 0 0 1px rgba(254, 240, 138, 0.35) inset;
      animation: ctaGlow 3.2s ease-in-out infinite;
      transition: transform 180ms ease, filter 180ms ease;
    }
    .btn-hot::after {
      content: "";
      position: absolute;
      inset: 0;
      background: linear-gradient(110deg, transparent 20%, rgba(255, 255, 255, 0.45) 50%, transparent 80%);
      transform: translateX(-120%);
      animation: shimmer 3.4s infinite;
    }
    .btn-hot:hover {
      transform: translateY(-2px);
      filter: brightness(1.06);
    }
    .btn-sub {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      width: 100%;
      min-height: 54px;
      padding: 14px 20px;
      border-radius: var(--radius-md);
      background: rgba(14, 165, 233, 0.14);
      border: 1.5px solid rgba(56, 189, 248, 0.55);
      color: #e0f2fe;
      text-decoration: none;
      font-family: var(--font-display);
      font-size: 14px;
      font-weight: 700;
      transition: background-color 180ms ease, transform 180ms ease;
    }
    .btn-sub:hover {
      background: rgba(14, 165, 233, 0.25);
      transform: translateY(-2px);
    }

    /* 5 chip sàn mang sắc màu phát sáng riêng */
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
      font-weight: 700;
      padding: 6px 13px;
      border-radius: 999px;
      background: rgba(13, 22, 38, 0.9);
      border: 1px solid var(--chip-bd, rgba(148, 163, 184, 0.3));
      color: var(--chip-fg, #f8fafc);
      box-shadow: 0 4px 14px rgba(0, 0, 0, 0.25);
    }
    .chip--shopee { --chip-bd: rgba(249, 115, 22, 0.55); --chip-fg: #fed7aa; }
    .chip--lazada { --chip-bd: rgba(236, 72, 153, 0.55); --chip-fg: #fbcfe8; }
    .chip--tiki { --chip-bd: rgba(56, 189, 248, 0.55); --chip-fg: #bae6fd; }
    .chip--tiktok { --chip-bd: rgba(244, 63, 94, 0.55); --chip-fg: #fecdd3; }
    .chip--alibaba { --chip-bd: rgba(251, 191, 36, 0.55); --chip-fg: #fef08a; }
    .chip-dot {
      width: 8px;
      height: 8px;
      border-radius: 50%;
      box-shadow: 0 0 8px currentColor;
    }

    /* Sân khấu 3D Pop-Out không viền hộp cứng */
    .stage-3d {
      position: relative;
      max-width: 490px;
      width: 100%;
      margin: 0 auto;
      perspective: 1100px;
      padding: 18px 4px 28px;
    }
    .stage-aura {
      position: absolute;
      inset: 8% 10% 14%;
      border-radius: 50%;
      background: radial-gradient(circle, rgba(249, 115, 22, 0.36) 0%, rgba(56, 189, 248, 0.26) 46%, transparent 72%);
      filter: blur(28px);
      pointer-events: none;
      animation: auraPulse 5.5s ease-in-out infinite alternate;
    }
    .holo-floor {
      position: absolute;
      left: 50%;
      bottom: 6px;
      width: 82%;
      height: 118px;
      transform: translateX(-50%) rotateX(74deg);
      border-radius: 50%;
      background: radial-gradient(circle, rgba(56, 189, 248, 0.42), rgba(249, 115, 22, 0.2) 55%, transparent 72%);
      border: 2px solid rgba(56, 189, 248, 0.48);
      box-shadow: 0 0 42px rgba(56, 189, 248, 0.32);
      pointer-events: none;
      animation: holoSpin 11s linear infinite;
    }
    .card-3d {
      position: relative;
      transform-style: preserve-3d;
      transform: rotateX(var(--rx)) rotateY(var(--ry));
      transition: transform 200ms cubic-bezier(0.22, 1, 0.36, 1);
      animation: idle3d 7s ease-in-out infinite;
    }
    .stage-3d.is-interacting .card-3d { animation: none; }

    /* Ảnh 3D hòa tan viền ngoài vào nền tối sân khấu */
    .hero-pop {
      position: relative;
      transform: translateZ(24px);
    }
    .hero-pop::after {
      content: "";
      position: absolute;
      inset: 10%;
      border-radius: 50%;
      pointer-events: none;
      background: radial-gradient(circle at var(--gx) var(--gy), rgba(255, 255, 255, 0.14), transparent 55%);
    }
    .hero-art {
      display: block;
      width: 100%;
      height: auto;
      aspect-ratio: 1 / 1;
      object-fit: cover;
      -webkit-mask-image: radial-gradient(circle at 50% 48%, #000 58%, transparent 74%);
      mask-image: radial-gradient(circle at 50% 48%, #000 58%, transparent 74%);
      filter: drop-shadow(0 26px 40px rgba(0, 0, 0, 0.72));
    }

    /* 8 hạt tia sáng bay quanh xe hàng */
    .sparks {
      position: absolute;
      inset: 0;
      pointer-events: none;
      transform: translateZ(42px);
    }
    .spark {
      position: absolute;
      width: 7px;
      height: 7px;
      border-radius: 50%;
      background: var(--color-gold);
      box-shadow: 0 0 12px var(--color-gold);
      animation: sparkRise 3.6s ease-in-out infinite;
    }
    .spark--1 { left: 16%; bottom: 28%; animation-delay: 0s; }
    .spark--2 { left: 26%; bottom: 20%; background: var(--color-cyan); box-shadow: 0 0 12px var(--color-cyan); animation-delay: 0.9s; }
    .spark--3 { left: 74%; bottom: 24%; animation-delay: 1.5s; }
    .spark--4 { left: 82%; bottom: 38%; background: var(--color-orange); box-shadow: 0 0 12px var(--color-orange); animation-delay: 2.1s; }
    .spark--5 { left: 20%; top: 30%; background: var(--color-emerald); box-shadow: 0 0 12px var(--color-emerald); animation-delay: 2.7s; }
    .spark--6 { left: 48%; bottom: 18%; animation-delay: 0.4s; }
    .spark--7 { left: 66%; top: 22%; background: var(--color-cyan); box-shadow: 0 0 12px var(--color-cyan); animation-delay: 1.8s; }
    .spark--8 { left: 36%; top: 20%; background: var(--color-orange); box-shadow: 0 0 12px var(--color-orange); animation-delay: 3.1s; }

    /* Huy hiệu 3D nổi tách thoáng không che robot */
    .layer-ticket-3d {
      position: absolute;
      top: 4px;
      left: 8px;
      z-index: 4;
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 8px 14px;
      border-radius: 14px;
      background: rgba(13, 22, 38, 0.92);
      border: 1.5px solid rgba(251, 191, 36, 0.78);
      box-shadow: 0 12px 30px rgba(249, 115, 22, 0.35);
      transform: translateZ(65px);
      animation: badgeBob 4.2s ease-in-out infinite;
    }
    .layer-ticket-pct {
      padding: 4px 10px;
      border-radius: 8px;
      background: linear-gradient(135deg, #ff5e00, #fde047);
      color: #060d18;
      font-family: var(--font-display);
      font-size: 13px;
      font-weight: 800;
    }
    .layer-ticket-txt {
      font-size: 12px;
      font-weight: 700;
      color: #fef08a;
    }

    .layer-dock-3d {
      position: relative;
      z-index: 5;
      margin: -42px 16px 0;
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      padding: 12px 15px;
      border-radius: 18px;
      background: rgba(13, 22, 38, 0.9);
      backdrop-filter: blur(16px);
      -webkit-backdrop-filter: blur(16px);
      border: 1.5px solid rgba(56, 189, 248, 0.48);
      box-shadow: 0 18px 40px rgba(0, 0, 0, 0.65), 0 0 24px rgba(56, 189, 248, 0.22);
      text-decoration: none;
      color: var(--color-text);
      transform: translateZ(78px);
      transition: border-color 180ms ease, transform 180ms ease;
    }
    .layer-dock-3d:hover {
      border-color: var(--color-gold);
    }
    .dock-left {
      display: flex;
      align-items: center;
      gap: 11px;
      min-width: 0;
    }
    .dock-left img {
      width: 42px;
      height: 42px;
      border-radius: 50%;
      object-fit: cover;
      border: 2px solid var(--color-cyan);
      flex-shrink: 0;
    }
    .dock-title {
      display: block;
      font-family: var(--font-display);
      font-size: 14px;
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
      padding: 9px 15px;
      border-radius: 11px;
      background: linear-gradient(95deg, #ff5e00, #fbbf24);
      color: #060d18;
      font-family: var(--font-display);
      font-size: 13px;
      font-weight: 800;
      box-shadow: 0 6px 18px rgba(249, 115, 22, 0.45);
    }

    /* Dải băng chuyền neon */
    .marquee-strip {
      position: relative;
      overflow: hidden;
      border-radius: var(--radius-md);
      background: linear-gradient(90deg, rgba(249, 115, 22, 0.14), rgba(13, 22, 38, 0.92) 25%, rgba(13, 22, 38, 0.92) 75%, rgba(56, 189, 248, 0.14));
      border: 1px solid rgba(56, 189, 248, 0.28);
      padding: 12px 0;
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
      background: rgba(19, 32, 54, 0.92);
      border: 1px solid rgba(148, 163, 184, 0.24);
      font-size: 13px;
      font-weight: 600;
      color: #e2e8f0;
      white-space: nowrap;
    }
    .marquee-item strong { color: #fde047; }

    /* 3 thẻ quy trình với 3 màu nhấn riêng biệt */
    .steps-grid {
      display: grid;
      grid-template-columns: 1fr;
      gap: var(--space-4);
    }
    .step-card {
      background: rgba(13, 22, 38, 0.92);
      border: 1px solid rgba(148, 163, 184, 0.22);
      border-top: 3px solid var(--step-color, #f97316);
      border-radius: var(--radius-md);
      padding: var(--space-4);
      display: flex;
      flex-direction: column;
      gap: var(--space-2);
      transition: transform 180ms ease, box-shadow 180ms ease;
    }
    .step-card--1 { --step-color: #f97316; }
    .step-card--2 { --step-color: #38bdf8; }
    .step-card--3 { --step-color: #34d399; }
    .step-card:hover {
      transform: translateY(-3px);
      box-shadow: 0 14px 32px rgba(0, 0, 0, 0.45);
    }
    .step-head { display: flex; align-items: center; gap: 12px; }
    .step-num {
      width: 36px;
      height: 36px;
      border-radius: 10px;
      background: rgba(255, 255, 255, 0.06);
      border: 1px solid var(--step-color);
      color: var(--step-color);
      font-family: var(--font-display);
      font-size: 14px;
      font-weight: 800;
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
      border-top: 1px solid rgba(56, 189, 248, 0.16);
      text-align: center;
      padding: var(--space-4);
      font-size: 13px;
      color: #94a3b8;
    }

    @keyframes ambientPulse {
      0% { transform: scale(1) translate(0, 0); opacity: 0.28; }
      100% { transform: scale(1.12) translate(16px, -12px); opacity: 0.44; }
    }
    @keyframes spinRing {
      to { transform: rotate(360deg); }
    }
    @keyframes auraPulse {
      0% { transform: scale(0.96); opacity: 0.7; }
      100% { transform: scale(1.06); opacity: 1; }
    }
    @keyframes holoSpin {
      0% { transform: translateX(-50%) rotateX(74deg) rotateZ(0deg); }
      100% { transform: translateX(-50%) rotateX(74deg) rotateZ(360deg); }
    }
    @keyframes idle3d {
      0%, 100% { transform: rotateX(3deg) rotateY(-6deg) translateY(0px); }
      50% { transform: rotateX(-3deg) rotateY(6deg) translateY(-10px); }
    }
    @keyframes badgeBob {
      0%, 100% { transform: translateZ(65px) translateY(0px); }
      50% { transform: translateZ(75px) translateY(-7px); }
    }
    @keyframes sparkRise {
      0%, 100% { transform: translateY(0) scale(0.8); opacity: 0.25; }
      50% { transform: translateY(-22px) scale(1.25); opacity: 1; }
    }
    @keyframes ctaGlow {
      0%, 100% { box-shadow: 0 12px 32px rgba(249, 115, 22, 0.46), 0 0 0 1px rgba(254, 240, 138, 0.35) inset; }
      50% { box-shadow: 0 16px 42px rgba(249, 115, 22, 0.72), 0 0 0 1px rgba(254, 240, 138, 0.65) inset; }
    }
    @keyframes shimmer {
      0%, 55% { transform: translateX(-120%); }
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

    @media (min-width: 640px) {
      .btn-hot, .btn-sub { width: auto; }
    }
    @media (min-width: 820px) {
      .page { padding: var(--space-5) var(--space-5) var(--space-6); }
      .hero-grid { grid-template-columns: 1fr 1.05fr; gap: var(--space-6); }
      .steps-grid { grid-template-columns: repeat(3, 1fr); }
    }
    @media (prefers-reduced-motion: reduce) {
      *, *::before, *::after {
        animation: none !important;
        transition: none !important;
      }
      .card-3d, .btn-hot:hover, .btn-sub:hover, .step-card:hover { transform: none !important; }
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
          <img src="/assets/bot-logo.webp" alt="Logo ${BOT_DISPLAY_NAME}" width="52" height="52">
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
          Săn Mã Giảm Sâu
          <span class="grad-text">${BOT_DISPLAY_NAME}</span>
        </h1>
        <p class="lead">
          Dán link sản phẩm vào bot Telegram để tự động quét kho mã giảm giá đúng shop, gắn sẵn vào giỏ hàng và mở thẳng ứng dụng mua sắm chỉ với 1 chạm.
        </p>

        <div class="stats-row" aria-label="Thông số nổi bật">
          <div class="stat-card stat-card--orange">
            <span class="stat-val">5+ Sàn</span>
            <span class="stat-lbl">TMĐT Hỗ Trợ</span>
          </div>
          <div class="stat-card stat-card--cyan">
            <span class="stat-val">5 Link</span>
            <span class="stat-lbl">Đổi Cùng Lúc</span>
          </div>
          <div class="stat-card stat-card--emerald">
            <span class="stat-val">Miễn Phí</span>
            <span class="stat-lbl">100% Tự Động</span>
          </div>
        </div>

        <div class="cta-row">
          <a class="btn-hot" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path d="M21.9 4.2c.3-.9-.5-1.7-1.4-1.3L2.8 9.8c-.9.4-.9 1.7.1 2l4.5 1.5 1.7 5.3c.3.8 1.3 1 1.9.4l2.6-2.5 4.9 3.6c.7.5 1.7.1 1.9-.7l3.5-15.2zM9.7 13.7l-.5 3.1-1.1-3.6 9.9-6.2-8.3 6.7z"/>
            </svg>
            <span>Mở Bot Săn Mã Ngay</span>
          </a>
          <a class="btn-sub" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">@${BOT_USERNAME}</a>
        </div>

        <div class="platforms" aria-label="Các sàn thương mại điện tử hỗ trợ">
          <span class="chip chip--shopee"><span class="chip-dot" style="background:#f97316" aria-hidden="true"></span>Shopee</span>
          <span class="chip chip--lazada"><span class="chip-dot" style="background:#ec4899" aria-hidden="true"></span>Lazada</span>
          <span class="chip chip--tiki"><span class="chip-dot" style="background:#38bdf8" aria-hidden="true"></span>Tiki</span>
          <span class="chip chip--tiktok"><span class="chip-dot" style="background:#f43f5e" aria-hidden="true"></span>TikTok Shop</span>
          <span class="chip chip--alibaba"><span class="chip-dot" style="background:#fbbf24" aria-hidden="true"></span>Alibaba</span>
        </div>
      </section>

      <aside class="stage-3d" id="stage3d" aria-label="Minh họa 3D Robot đẩy xe hàng chứa kho mã giảm giá">
        <div class="stage-aura" aria-hidden="true"></div>
        <div class="holo-floor" aria-hidden="true"></div>
        <div class="card-3d">
          <div class="layer-ticket-3d" aria-hidden="true">
            <span class="layer-ticket-pct">🔥 GIẢM TỚI 50%</span>
            <span class="layer-ticket-txt">Kho mã mới mỗi giờ</span>
          </div>

          <div class="sparks" aria-hidden="true">
            <span class="spark spark--1"></span>
            <span class="spark spark--2"></span>
            <span class="spark spark--3"></span>
            <span class="spark spark--4"></span>
            <span class="spark spark--5"></span>
            <span class="spark spark--6"></span>
            <span class="spark spark--7"></span>
            <span class="spark spark--8"></span>
          </div>

          <div class="hero-pop">
            <img class="hero-art" src="/assets/bot-cart-3d.webp" alt="Bot Thế Giới Siêu Sale đẩy xe hàng chứa kho mã giảm giá 3D" width="960" height="960">
          </div>

          <a class="layer-dock-3d" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">
            <span class="dock-left">
              <img src="/assets/bot-logo.webp" alt="" width="42" height="42" aria-hidden="true">
              <span>
                <span class="dock-title">${BOT_DISPLAY_NAME}</span>
                <span class="dock-sub">@${BOT_USERNAME} · Gửi link nhận mã</span>
              </span>
            </span>
            <span class="dock-btn">Mở Bot ↗</span>
          </a>
        </div>
      </aside>
    </main>

    <div class="marquee-strip" aria-hidden="true">
      <div class="marquee-track">
        <span class="marquee-item">🛒 <strong>Shopee Video</strong> Giảm tới 50%</span>
        <span class="marquee-item">⚡ <strong>Cập nhật liên tục</strong> Kho mã độc quyền &amp; Mall</span>
        <span class="marquee-item">🎟️ <strong>Lazada</strong> Bonus + Freeship</span>
        <span class="marquee-item">🛍️ <strong>Tiki</strong> Mã gian hàng chính hãng</span>
        <span class="marquee-item">🔥 <strong>TikTok Shop</strong> Deal Live giảm sâu</span>
        <span class="marquee-item">🤖 <strong>Đổi tối đa 5 link</strong> chỉ trong 1 tin nhắn</span>
        <span class="marquee-item">🛒 <strong>Shopee Video</strong> Giảm tới 50%</span>
        <span class="marquee-item">⚡ <strong>Cập nhật liên tục</strong> Kho mã độc quyền &amp; Mall</span>
        <span class="marquee-item">🎟️ <strong>Lazada</strong> Bonus + Freeship</span>
        <span class="marquee-item">🛍️ <strong>Tiki</strong> Mã gian hàng chính hãng</span>
        <span class="marquee-item">🔥 <strong>TikTok Shop</strong> Deal Live giảm sâu</span>
        <span class="marquee-item">🤖 <strong>Đổi tối đa 5 link</strong> chỉ trong 1 tin nhắn</span>
      </div>
    </div>

    <section class="steps-grid" aria-label="Quy trình 3 bước săn mã">
      <article class="step-card step-card--1">
        <div class="step-head">
          <span class="step-num" aria-hidden="true">01</span>
          <h2>Chọn món vào giỏ</h2>
        </div>
        <p>Tìm sản phẩm muốn mua trên Shopee, Lazada, Tiki hoặc TikTok Shop rồi bấm sao chép liên kết.</p>
      </article>

      <article class="step-card step-card--2">
        <div class="step-head">
          <span class="step-num" aria-hidden="true">02</span>
          <h2>Gửi cho Bot Telegram</h2>
        </div>
        <p>Dán link vào khung chat của <strong>@${BOT_USERNAME}</strong> (hỗ trợ cả bài gom nhiều link một lúc).</p>
      </article>

      <article class="step-card step-card--3">
        <div class="step-head">
          <span class="step-num" aria-hidden="true">03</span>
          <h2>Nhận mã &amp; thanh toán</h2>
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
