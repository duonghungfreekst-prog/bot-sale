// Cloudflare Worker Proxy & Trang Đích Bot Telegram 3D "Thế Giới Siêu Sale"
// Hướng dẫn nhanh:
//   1. Đăng nhập Cloudflare -> Workers & Pages -> chọn Worker (VD: thegioisieusale) -> Edit code.
//   2. Xóa code mẫu, dán toàn bộ file này vào -> bấm Deploy.
//   3. Gắn tên miền (VD: thegioisieusale.click) ở tab Domains của Worker.
// Lưu ý: Trang gốc `/` hiển thị Trang Đích Bot Telegram 3D; các đường dẫn hệ thống (/admin, /convert, /health...) luôn trả 404.

const ORIGIN_URL = "https://origin.thegioisieusale.click";
const ASSET_FALLBACK_BASE = "https://raw.githubusercontent.com/duonghungfreekst-prog/bot-sale/main/assets";
const BOT_USERNAME = "vvipass_vip_bot";
const BOT_DISPLAY_NAME = "Thế Giới Siêu Sale";

const LANDING_JS = `(function () {
  function syncTheme() {
    var isDark = document.documentElement.getAttribute("data-theme") === "dark";
    var ico = document.getElementById("cf-theme-ico");
    var lbl = document.getElementById("cf-theme-lbl");
    var btn = document.getElementById("cf-theme-btn");
    if (ico) ico.setAttribute("href", isDark ? "#ico-sun" : "#ico-moon");
    if (lbl) lbl.textContent = isDark ? "Sáng" : "Tối";
    if (btn) btn.setAttribute("aria-pressed", isDark ? "true" : "false");
  }
  try {
    if (localStorage.getItem("dealx-theme") === "dark") {
      document.documentElement.setAttribute("data-theme", "dark");
    }
  } catch (_) {}
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", syncTheme);
  } else {
    syncTheme();
  }
  document.addEventListener("click", function (e) {
    var t = e.target.closest("#cf-theme-btn");
    if (!t) return;
    var next = document.documentElement.getAttribute("data-theme") === "dark" ? "light" : "dark";
    if (next === "dark") document.documentElement.setAttribute("data-theme", "dark");
    else document.documentElement.removeAttribute("data-theme");
    try { localStorage.setItem("dealx-theme", next); } catch (_) {}
    syncTheme();
  });
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
    stage.style.setProperty("--rx", ((0.5 - py) * 14).toFixed(2) + "deg");
    stage.style.setProperty("--ry", ((px - 0.5) * 16).toFixed(2) + "deg");
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
  <meta name="theme-color" content="#F4F6F9">
  <link rel="icon" type="image/webp" href="/assets/bot-logo.webp">
  <meta property="og:title" content="${BOT_DISPLAY_NAME} — Trợ Lý Săn Mã &amp; Giỏ Hàng Telegram">
  <meta property="og:description" content="Gửi link sản phẩm vào @${BOT_USERNAME} để tìm mã giảm sâu và mở thẳng ứng dụng mua sắm.">
  <meta property="og:image" content="/assets/bot-logo.webp">
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Be+Vietnam+Pro:wght@400;500;600;700&amp;family=Lexend:wght@600;700;800&amp;family=JetBrains+Mono:wght@500;700&amp;display=swap">
  <style>
    :root {
      color-scheme: light;
      --color-bg: #F4F6F9;
      --color-surface: #FFFFFF;
      --color-surface-2: #F8FAFC;
      --color-inset: #EEF2F6;
      --color-border: #E2E8F0;
      --color-border-strong: #CBD5E1;
      --color-text: #0F172A;
      --color-text-muted: #475569;
      --color-primary: #C2410C;
      --color-primary-hover: #9A3412;
      --color-primary-bright: #EA580C;
      --color-primary-contrast: #FFFFFF;
      --color-primary-soft: rgba(194, 65, 12, 0.10);
      --color-primary-border: rgba(194, 65, 12, 0.28);
      --color-cyan: #0284C7;
      --color-cyan-soft: rgba(2, 132, 199, 0.10);
      --color-cyan-border: rgba(2, 132, 199, 0.28);
      --color-emerald: #15803D;
      --color-emerald-soft: #DCFCE7;
      --color-emerald-border: #86EFAC;
      --shadow-sm: 0 2px 6px rgba(15, 23, 42, 0.05);
      --shadow-md: 0 14px 34px -10px rgba(15, 23, 42, 0.10), 0 2px 6px rgba(15, 23, 42, 0.04);
      --shadow-3d: 0 24px 50px -14px rgba(194, 65, 12, 0.20), 0 8px 20px rgba(15, 23, 42, 0.08);
      --rx: 0deg;
      --ry: 0deg;
      --gx: 50%;
      --gy: 45%;
      --radius-md: 16px;
      --radius-lg: 24px;
      --font-sans: "Be Vietnam Pro", system-ui, -apple-system, sans-serif;
      --font-display: "Lexend", "Be Vietnam Pro", system-ui, sans-serif;
      --font-mono: "JetBrains Mono", monospace;
    }
    html[data-theme="dark"] {
      color-scheme: dark;
      --color-bg: #0B1120;
      --color-surface: #131C31;
      --color-surface-2: #18233C;
      --color-inset: #0F172A;
      --color-border: rgba(148, 163, 184, 0.20);
      --color-border-strong: rgba(148, 163, 184, 0.34);
      --color-text: #F8FAFC;
      --color-text-muted: #CBD5E1;
      --color-primary: #F97316;
      --color-primary-hover: #FB923C;
      --color-primary-bright: #FB923C;
      --color-primary-contrast: #090D16;
      --color-primary-soft: rgba(249, 115, 22, 0.16);
      --color-primary-border: rgba(249, 115, 22, 0.36);
      --color-cyan: #38BDF8;
      --color-cyan-soft: rgba(56, 189, 248, 0.14);
      --color-cyan-border: rgba(56, 189, 248, 0.34);
      --color-emerald: #34D399;
      --color-emerald-soft: rgba(52, 211, 153, 0.14);
      --color-emerald-border: rgba(52, 211, 153, 0.36);
      --shadow-sm: 0 2px 6px rgba(0, 0, 0, 0.35);
      --shadow-md: 0 16px 36px -12px rgba(0, 0, 0, 0.5);
      --shadow-3d: 0 28px 54px -14px rgba(0, 0, 0, 0.65);
    }
    * { box-sizing: border-box; }
    html, body { margin: 0; padding: 0; }
    body {
      min-height: 100vh;
      overflow-x: hidden;
      background-color: var(--color-bg);
      background-image:
        radial-gradient(circle at 84% 0%, rgba(234, 88, 12, 0.08), transparent 44%),
        radial-gradient(circle at 10% 30%, rgba(2, 132, 199, 0.07), transparent 42%);
      color: var(--color-text);
      font-family: var(--font-sans);
      font-size: 16px;
      line-height: 1.6;
      display: flex;
      flex-direction: column;
      -webkit-font-smoothing: antialiased;
    }
    a:focus-visible, button:focus-visible {
      outline: 2px solid var(--color-primary);
      outline-offset: 2px;
    }
    .ico { width: 17px; height: 17px; display: inline-block; vertical-align: middle; flex-shrink: 0; }

    .page {
      width: 100%;
      max-width: 1180px;
      margin: 0 auto;
      padding: 18px 18px 36px;
      flex: 1;
      display: flex;
      flex-direction: column;
      gap: 28px;
    }

    /* SaaS Topbar */
    .topbar {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      padding: 11px 16px;
      border-radius: 18px;
      background: var(--color-surface);
      border: 1px solid var(--color-border);
      box-shadow: var(--shadow-sm);
    }
    .brand {
      display: inline-flex;
      align-items: center;
      gap: 12px;
      text-decoration: none;
      color: var(--color-text);
      font-family: var(--font-display);
      font-weight: 800;
      font-size: 18px;
      letter-spacing: -0.02em;
    }
    .logo-ring {
      position: relative;
      width: 46px;
      height: 46px;
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
      background: conic-gradient(from 0deg, var(--color-primary-bright), var(--color-cyan), var(--color-emerald), var(--color-primary-bright));
      animation: spinRing 6s linear infinite;
    }
    .logo-ring img {
      position: relative;
      z-index: 1;
      width: 100%;
      height: 100%;
      border-radius: 50%;
      object-fit: cover;
      background: var(--color-surface);
      display: block;
    }
    .top-right {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 8px;
    }
    .status-pill {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 7px 13px;
      border-radius: 999px;
      background: var(--color-emerald-soft);
      border: 1px solid var(--color-emerald-border);
      color: var(--color-emerald);
      font-size: 13px;
      font-weight: 700;
    }
    .dot {
      width: 8px;
      height: 8px;
      border-radius: 50%;
      background: var(--color-emerald);
      animation: dotPulse 2.2s infinite;
    }
    .handle-pill {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      min-height: 38px;
      padding: 7px 14px;
      border-radius: 999px;
      background: var(--color-cyan-soft);
      border: 1px solid var(--color-cyan-border);
      color: var(--color-cyan);
      text-decoration: none;
      font-size: 13px;
      font-weight: 700;
      transition: transform 150ms ease, filter 150ms ease;
    }
    .handle-pill:hover { filter: brightness(1.06); transform: translateY(-1px); }
    .theme-btn {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      min-height: 38px;
      padding: 7px 13px;
      border-radius: 999px;
      background: var(--color-surface-2);
      border: 1px solid var(--color-border-strong);
      color: var(--color-text);
      font-family: var(--font-sans);
      font-size: 13px;
      font-weight: 700;
      cursor: pointer;
    }
    .theme-btn:hover { border-color: var(--color-primary); }

    /* Hero 2 Columns */
    .hero-grid {
      display: grid;
      grid-template-columns: 1fr;
      gap: 28px;
      align-items: center;
    }
    .hero-badge {
      display: inline-flex;
      align-items: center;
      gap: 7px;
      padding: 6px 13px;
      border-radius: 999px;
      background: var(--color-primary-soft);
      border: 1px solid var(--color-primary-border);
      color: var(--color-primary);
      font-size: 13px;
      font-weight: 700;
      margin-bottom: 14px;
    }
    h1 {
      margin: 0 0 12px;
      font-family: var(--font-display);
      font-size: clamp(32px, 4.8vw, 50px);
      line-height: 1.12;
      letter-spacing: -0.03em;
      color: var(--color-text);
    }
    .grad-text {
      display: block;
      margin-top: 4px;
      background: linear-gradient(95deg, var(--color-primary-bright) 0%, var(--color-cyan) 100%);
      -webkit-background-clip: text;
      background-clip: text;
      color: transparent;
    }
    .lead {
      margin: 0 0 20px;
      color: var(--color-text-muted);
      font-size: 16px;
      max-width: 54ch;
    }

    .stats-row {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      gap: 10px;
      margin-bottom: 22px;
    }
    .stat-card {
      background: var(--color-surface);
      border: 1px solid var(--color-border);
      border-top: 3px solid var(--accent-line, var(--color-primary-bright));
      border-radius: var(--radius-md);
      padding: 13px 10px;
      text-align: center;
      box-shadow: var(--shadow-sm);
    }
    .stat-card--orange { --accent-line: var(--color-primary-bright); }
    .stat-card--cyan { --accent-line: var(--color-cyan); }
    .stat-card--emerald { --accent-line: var(--color-emerald); }
    .stat-val {
      display: block;
      font-family: var(--font-display);
      font-size: 21px;
      font-weight: 800;
      color: var(--color-text);
      line-height: 1.2;
    }
    .stat-card--orange .stat-val { color: var(--color-primary); }
    .stat-card--cyan .stat-val { color: var(--color-cyan); }
    .stat-card--emerald .stat-val { color: var(--color-emerald); }
    .stat-lbl {
      display: block;
      font-size: 12px;
      font-weight: 600;
      color: var(--color-text-muted);
      margin-top: 3px;
    }

    .cta-row {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 12px;
    }
    .btn-hot {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: 9px;
      width: 100%;
      min-height: 52px;
      padding: 13px 24px;
      border-radius: var(--radius-md);
      background: var(--color-primary);
      color: var(--color-primary-contrast);
      text-decoration: none;
      font-family: var(--font-display);
      font-size: 15.5px;
      font-weight: 800;
      box-shadow: 0 10px 24px -6px rgba(194, 65, 12, 0.45);
      transition: background-color 160ms ease, transform 160ms ease;
    }
    .btn-hot:hover {
      background: var(--color-primary-hover);
      transform: translateY(-1px);
    }
    .btn-sub {
      display: inline-flex;
      align-items: center;
      justify-content: center;
      gap: 7px;
      width: 100%;
      min-height: 52px;
      padding: 13px 20px;
      border-radius: var(--radius-md);
      background: var(--color-surface);
      border: 1.5px solid var(--color-border-strong);
      color: var(--color-text);
      text-decoration: none;
      font-family: var(--font-display);
      font-size: 14.5px;
      font-weight: 700;
      transition: border-color 160ms ease, transform 160ms ease;
    }
    .btn-sub:hover {
      border-color: var(--color-primary);
      transform: translateY(-1px);
    }

    .platforms {
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
      margin-top: 18px;
    }
    .chip {
      display: inline-flex;
      align-items: center;
      gap: 7px;
      font-size: 13px;
      font-weight: 700;
      padding: 6px 13px;
      border-radius: 999px;
      background: var(--color-surface);
      border: 1px solid var(--color-border);
      color: var(--color-text);
      box-shadow: var(--shadow-sm);
    }
    .chip-dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }

    /* 3D Showcase Stage */
    .stage-3d {
      position: relative;
      max-width: 450px;
      width: 100%;
      margin: 0 auto;
      perspective: 1100px;
      padding: 14px 6px 26px;
    }
    .stage-aura {
      position: absolute;
      inset: 6% 8% 12%;
      border-radius: 32px;
      background: radial-gradient(circle at var(--gx) var(--gy), rgba(234, 88, 12, 0.18), rgba(2, 132, 199, 0.14) 55%, transparent 75%);
      pointer-events: none;
    }
    .card-3d {
      position: relative;
      border-radius: 26px;
      padding: 16px;
      background: linear-gradient(155deg, var(--color-surface) 0%, var(--color-surface-2) 100%);
      border: 1px solid var(--color-border);
      box-shadow: var(--shadow-3d);
      transform-style: preserve-3d;
      transform: rotateX(var(--rx)) rotateY(var(--ry));
      transition: transform 200ms cubic-bezier(0.22, 1, 0.36, 1);
      animation: idle3d 8s ease-in-out infinite;
    }
    .stage-3d.is-interacting .card-3d { animation: none; }
    .hero-pop { position: relative; transform: translateZ(20px); }
    .hero-art {
      display: block;
      width: 100%;
      height: auto;
      aspect-ratio: 1 / 1;
      object-fit: cover;
      border-radius: 18px;
      box-shadow: 0 12px 26px rgba(15, 23, 42, 0.16);
    }
    .layer-ticket-3d {
      position: absolute;
      top: -12px;
      left: 14px;
      z-index: 4;
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 7px 13px;
      border-radius: 12px;
      background: var(--color-surface);
      border: 1px solid var(--color-primary-border);
      box-shadow: var(--shadow-md);
      transform: translateZ(52px);
      animation: badgeBob 4.5s ease-in-out infinite;
    }
    .layer-ticket-pct {
      display: inline-flex;
      align-items: center;
      gap: 5px;
      padding: 3px 9px;
      border-radius: 7px;
      background: var(--color-primary);
      color: var(--color-primary-contrast);
      font-family: var(--font-display);
      font-size: 12px;
      font-weight: 800;
    }
    .layer-ticket-txt {
      font-size: 12px;
      font-weight: 700;
      color: var(--color-text);
    }
    .layer-dock-3d {
      position: relative;
      z-index: 5;
      margin: -34px 10px 0;
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 10px;
      padding: 11px 14px;
      border-radius: 16px;
      background: var(--color-surface);
      border: 1px solid var(--color-border-strong);
      box-shadow: var(--shadow-md);
      text-decoration: none;
      color: var(--color-text);
      transform: translateZ(62px);
      transition: border-color 160ms ease;
    }
    .layer-dock-3d:hover { border-color: var(--color-primary); }
    .dock-left { display: flex; align-items: center; gap: 10px; min-width: 0; }
    .dock-left img {
      width: 40px; height: 40px; border-radius: 50%; object-fit: cover;
      border: 2px solid var(--color-primary-bright); flex-shrink: 0;
    }
    .dock-title {
      display: block; font-family: var(--font-display); font-size: 14px; font-weight: 800;
      white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    }
    .dock-sub { display: block; font-size: 12px; color: var(--color-text-muted); }
    .dock-btn {
      flex-shrink: 0;
      display: inline-flex; align-items: center; gap: 5px;
      padding: 8px 13px; border-radius: 10px;
      background: var(--color-primary); color: var(--color-primary-contrast);
      font-family: var(--font-display); font-size: 12.5px; font-weight: 800;
    }

    /* Feature Marquee Ticker */
    .marquee-strip {
      position: relative;
      overflow: hidden;
      border-radius: var(--radius-md);
      background: var(--color-surface);
      border: 1px solid var(--color-border);
      box-shadow: var(--shadow-sm);
      padding: 11px 0;
    }
    .marquee-track {
      display: flex;
      width: max-content;
      gap: 12px;
      animation: marquee 30s linear infinite;
    }
    .marquee-strip:hover .marquee-track { animation-play-state: paused; }
    .marquee-item {
      display: inline-flex;
      align-items: center;
      gap: 7px;
      padding: 6px 13px;
      border-radius: 999px;
      background: var(--color-inset);
      border: 1px solid var(--color-border);
      font-size: 13px;
      font-weight: 600;
      color: var(--color-text-muted);
      white-space: nowrap;
    }
    .marquee-item strong { color: var(--color-text); }
    .marquee-item .ico { color: var(--color-primary-bright); }

    /* 3 Steps Grid */
    .steps-grid {
      display: grid;
      grid-template-columns: 1fr;
      gap: 16px;
    }
    .step-card {
      background: var(--color-surface);
      border: 1px solid var(--color-border);
      border-top: 3px solid var(--step-color, var(--color-primary-bright));
      border-radius: var(--radius-md);
      padding: 18px;
      box-shadow: var(--shadow-sm);
      display: flex;
      flex-direction: column;
      gap: 9px;
      transition: transform 160ms ease, box-shadow 160ms ease;
    }
    .step-card--1 { --step-color: var(--color-primary-bright); }
    .step-card--2 { --step-color: var(--color-cyan); }
    .step-card--3 { --step-color: var(--color-emerald); }
    .step-card:hover { transform: translateY(-2px); box-shadow: var(--shadow-md); }
    .step-head { display: flex; align-items: center; gap: 11px; }
    .step-num {
      width: 34px; height: 34px; border-radius: 10px;
      background: var(--color-inset); border: 1px solid var(--color-border-strong);
      color: var(--step-color);
      font-family: var(--font-mono); font-size: 13px; font-weight: 800;
      display: inline-flex; align-items: center; justify-content: center; flex-shrink: 0;
    }
    .step-card h2 {
      margin: 0; font-family: var(--font-display); font-size: 16px; line-height: 1.3;
      color: var(--color-text);
    }
    .step-card p { margin: 0; font-size: 14px; color: var(--color-text-muted); }

    footer {
      border-top: 1px solid var(--color-border);
      background: var(--color-surface);
      text-align: center;
      padding: 18px;
      font-size: 13px;
      color: var(--color-text-muted);
    }

    @keyframes spinRing { to { transform: rotate(360deg); } }
    @keyframes idle3d {
      0%, 100% { transform: rotateX(2deg) rotateY(-4deg) translateY(0px); }
      50% { transform: rotateX(-2deg) rotateY(4deg) translateY(-6px); }
    }
    @keyframes badgeBob {
      0%, 100% { transform: translateZ(52px) translateY(0px); }
      50% { transform: translateZ(58px) translateY(-5px); }
    }
    @keyframes dotPulse {
      0% { box-shadow: 0 0 0 0 rgba(21, 128, 61, 0.5); }
      70% { box-shadow: 0 0 0 7px rgba(21, 128, 61, 0); }
      100% { box-shadow: 0 0 0 0 rgba(21, 128, 61, 0); }
    }
    @keyframes marquee {
      0% { transform: translateX(0); }
      100% { transform: translateX(-50%); }
    }

    @media (min-width: 640px) {
      .btn-hot, .btn-sub { width: auto; }
    }
    @media (min-width: 820px) {
      .page { padding: 24px 24px 40px; }
      .hero-grid { grid-template-columns: 1.08fr 0.92fr; gap: 36px; }
      .steps-grid { grid-template-columns: repeat(3, 1fr); }
    }
    @media (prefers-reduced-motion: reduce) {
      *, *::before, *::after { animation: none !important; transition: none !important; }
      .card-3d, .btn-hot:hover, .btn-sub:hover, .step-card:hover { transform: none !important; }
    }
  </style>
</head>
<body>
  <svg aria-hidden="true" style="position:absolute;width:0;height:0;overflow:hidden">
    <defs>
      <symbol id="ico-sun" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/></symbol>
      <symbol id="ico-moon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/></symbol>
      <symbol id="ico-flame" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.072-2.143-.224-4.054 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.153.433-2.294 1-3a2.5 2.5 0 0 0 2.5 2.5z"/></symbol>
      <symbol id="ico-zap" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></symbol>
      <symbol id="ico-ticket" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 9a3 3 0 0 1 0 6v2a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-2a3 3 0 0 1 0-6V7a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2Z"/></symbol>
      <symbol id="ico-bag" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 2 3 6v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V6l-3-4z"/><line x1="3" y1="6" x2="21" y2="6"/><path d="M16 10a4 4 0 0 1-8 0"/></symbol>
      <symbol id="ico-bot" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 8V4H8"/><rect width="16" height="12" x="4" y="8" rx="2"/><path d="M2 14h2"/><path d="M20 14h2"/><path d="M15 13v2"/><path d="M9 13v2"/></symbol>
      <symbol id="ico-shield" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="m9 12 2 2 4-4"/></symbol>
      <symbol id="ico-external" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></symbol>
    </defs>
  </svg>

  <div class="page">
    <header class="topbar">
      <a class="brand" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">
        <span class="logo-ring">
          <img src="/assets/bot-logo.webp" alt="Logo ${BOT_DISPLAY_NAME}" width="46" height="46">
        </span>
        <span>${BOT_DISPLAY_NAME}</span>
      </a>
      <div class="top-right">
        <span class="status-pill">
          <span class="dot" aria-hidden="true"></span>
          <span>Bot đang săn mã 24/7</span>
        </span>
        <button type="button" id="cf-theme-btn" class="theme-btn" aria-label="Chuyển đổi giao diện Sáng/Tối">
          <svg class="ico" aria-hidden="true"><use id="cf-theme-ico" href="#ico-moon"/></svg>
          <span id="cf-theme-lbl">Tối</span>
        </button>
        <a class="handle-pill" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">@${BOT_USERNAME}</a>
      </div>
    </header>

    <main class="hero-grid">
      <section>
        <span class="hero-badge">
          <svg class="ico" aria-hidden="true"><use href="#ico-shield"/></svg>
          <span>Trợ Lý Săn Sale &amp; Khớp Mã Thông Minh</span>
        </span>
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
            <svg width="19" height="19" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path d="M21.9 4.2c.3-.9-.5-1.7-1.4-1.3L2.8 9.8c-.9.4-.9 1.7.1 2l4.5 1.5 1.7 5.3c.3.8 1.3 1 1.9.4l2.6-2.5 4.9 3.6c.7.5 1.7.1 1.9-.7l3.5-15.2zM9.7 13.7l-.5 3.1-1.1-3.6 9.9-6.2-8.3 6.7z"/>
            </svg>
            <span>Mở Bot Săn Mã Ngay</span>
          </a>
          <a class="btn-sub" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">@${BOT_USERNAME}</a>
        </div>

        <div class="platforms" aria-label="Các sàn thương mại điện tử hỗ trợ">
          <span class="chip"><span class="chip-dot" style="background:#EA580C" aria-hidden="true"></span>Shopee</span>
          <span class="chip"><span class="chip-dot" style="background:#DB2777" aria-hidden="true"></span>Lazada</span>
          <span class="chip"><span class="chip-dot" style="background:#0284C7" aria-hidden="true"></span>Tiki</span>
          <span class="chip"><span class="chip-dot" style="background:#E11D48" aria-hidden="true"></span>TikTok Shop</span>
          <span class="chip"><span class="chip-dot" style="background:#D97706" aria-hidden="true"></span>Alibaba</span>
        </div>
      </section>

      <aside class="stage-3d" id="stage3d" aria-label="Minh họa 3D Robot đẩy xe hàng chứa kho mã giảm giá">
        <div class="stage-aura" aria-hidden="true"></div>
        <div class="card-3d">
          <div class="layer-ticket-3d" aria-hidden="true">
            <span class="layer-ticket-pct">
              <svg class="ico" style="width:13px;height:13px"><use href="#ico-flame"/></svg>
              <span>GIẢM TỚI 50%</span>
            </span>
            <span class="layer-ticket-txt">Kho mã mới mỗi giờ</span>
          </div>

          <div class="hero-pop">
            <img class="hero-art" src="/assets/bot-cart-3d.webp" alt="Bot Thế Giới Siêu Sale đẩy xe hàng chứa kho mã giảm giá 3D" width="960" height="960">
          </div>

          <a class="layer-dock-3d" href="https://t.me/${BOT_USERNAME}" target="_blank" rel="noopener noreferrer">
            <span class="dock-left">
              <img src="/assets/bot-logo.webp" alt="" width="40" height="40" aria-hidden="true">
              <span>
                <span class="dock-title">${BOT_DISPLAY_NAME}</span>
                <span class="dock-sub">@${BOT_USERNAME} · Gửi link nhận mã</span>
              </span>
            </span>
            <span class="dock-btn">
              <span>Mở Bot</span>
              <svg class="ico" style="width:13px;height:13px" aria-hidden="true"><use href="#ico-external"/></svg>
            </span>
          </a>
        </div>
      </aside>
    </main>

    <div class="marquee-strip" aria-hidden="true">
      <div class="marquee-track">
        <span class="marquee-item"><svg class="ico"><use href="#ico-bag"/></svg> <strong>Shopee Video</strong> Giảm tới 50%</span>
        <span class="marquee-item"><svg class="ico"><use href="#ico-zap"/></svg> <strong>Cập nhật liên tục</strong> Kho mã độc quyền &amp; Mall</span>
        <span class="marquee-item"><svg class="ico"><use href="#ico-ticket"/></svg> <strong>Lazada</strong> Bonus + Freeship</span>
        <span class="marquee-item"><svg class="ico"><use href="#ico-bag"/></svg> <strong>Tiki</strong> Mã gian hàng chính hãng</span>
        <span class="marquee-item"><svg class="ico"><use href="#ico-flame"/></svg> <strong>TikTok Shop</strong> Deal Live giảm sâu</span>
        <span class="marquee-item"><svg class="ico"><use href="#ico-bot"/></svg> <strong>Đổi tối đa 5 link</strong> chỉ trong 1 tin nhắn</span>
        <span class="marquee-item"><svg class="ico"><use href="#ico-bag"/></svg> <strong>Shopee Video</strong> Giảm tới 50%</span>
        <span class="marquee-item"><svg class="ico"><use href="#ico-zap"/></svg> <strong>Cập nhật liên tục</strong> Kho mã độc quyền &amp; Mall</span>
        <span class="marquee-item"><svg class="ico"><use href="#ico-ticket"/></svg> <strong>Lazada</strong> Bonus + Freeship</span>
        <span class="marquee-item"><svg class="ico"><use href="#ico-bag"/></svg> <strong>Tiki</strong> Mã gian hàng chính hãng</span>
        <span class="marquee-item"><svg class="ico"><use href="#ico-flame"/></svg> <strong>TikTok Shop</strong> Deal Live giảm sâu</span>
        <span class="marquee-item"><svg class="ico"><use href="#ico-bot"/></svg> <strong>Đổi tối đa 5 link</strong> chỉ trong 1 tin nhắn</span>
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
