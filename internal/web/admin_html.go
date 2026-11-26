package web

import (
	"fmt"
	"math"
	"net/http"
	"strings"

	salehunterassets "salehunter-go"
	"salehunter-go/internal/db"
	"salehunter-go/internal/services/accesstrade"
	"salehunter-go/internal/services/shopee"
	"salehunter-go/internal/utils"
)

const adminSVGSprite = `<svg aria-hidden="true" style="position:absolute;width:0;height:0;overflow:hidden">
    <defs>
        <symbol id="ico-overview" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <rect x="3" y="3" width="7" height="7" rx="1.8"/><rect x="14" y="3" width="7" height="7" rx="1.8"/>
            <rect x="14" y="14" width="7" height="7" rx="1.8"/><rect x="3" y="14" width="7" height="7" rx="1.8"/>
        </symbol>
        <symbol id="ico-platforms" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/>
            <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>
        </symbol>
        <symbol id="ico-vouchers" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M2 9a3 3 0 0 1 0 6v2a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-2a3 3 0 0 1 0-6V7a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2Z"/>
            <path d="M13 5v2"/><path d="M13 17v2"/><path d="M13 11v2"/>
        </symbol>
        <symbol id="ico-orders" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/>
        </symbol>
        <symbol id="ico-users" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/>
            <path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/>
        </symbol>
        <symbol id="ico-clicks" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>
        </symbol>
        <symbol id="ico-flame" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.072-2.143-.224-4.054 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.153.433-2.294 1-3a2.5 2.5 0 0 0 2.5 2.5z"/>
        </symbol>
        <symbol id="ico-clock" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="9"/><polyline points="12 7 12 12 15.5 14"/>
        </symbol>
        <symbol id="ico-bot" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 8V4H8"/><rect width="16" height="12" x="4" y="8" rx="2"/>
            <path d="M2 14h2"/><path d="M20 14h2"/><path d="M15 13v2"/><path d="M9 13v2"/>
        </symbol>
        <symbol id="ico-history" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l4 2"/>
        </symbol>
        <symbol id="ico-chart" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <line x1="18" y1="20" x2="18" y2="10"/><line x1="12" y1="20" x2="12" y2="4"/><line x1="6" y1="20" x2="6" y2="14"/>
        </symbol>
        <symbol id="ico-pie" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M21.21 15.89A10 10 0 1 1 8 2.83"/><path d="M22 12A10 10 0 0 0 12 2v10z"/>
        </symbol>
        <symbol id="ico-rocket" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09z"/>
            <path d="m12 15-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2z"/>
            <path d="M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0"/><path d="M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5"/>
        </symbol>
        <symbol id="ico-crown" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="m2 4 3 12h14l3-12-6 7-4-7-4 7-6-7zm3 16h14"/>
        </symbol>
        <symbol id="ico-globe" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/>
            <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/>
        </symbol>
        <symbol id="ico-shield" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="m9 12 2 2 4-4"/>
        </symbol>
        <symbol id="ico-moon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>
        </symbol>
        <symbol id="ico-sun" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/>
            <path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/>
        </symbol>
    </defs>
</svg>`

const adminCSS = `<style>
        :root {
            color-scheme: light;
            --bg-base: #F4F6F9;
            --bg-sidebar: #FFFFFF;
            --bg-topbar: rgba(255, 255, 255, 0.94);
            --bg-surface: #FFFFFF;
            --bg-elevated: #F8FAFC;
            --bg-inset: #F1F5F9;
            --bg-input: #FFFFFF;
            --bg-table-head: #F8FAFC;
            --bg-table-hover: #F1F5F9;
            --border-subtle: #E2E8F0;
            --border-strong: #CBD5E1;
            --text-primary: #0F172A;
            --text-secondary: #334155;
            --text-muted: #475569;
            --text-placeholder: #94A3B8;
            --brand-primary: #C2410C;
            --brand-hover: #9A3412;
            --brand-bright: #EA580C;
            --brand-accent: #C2410C;
            --brand-contrast: #FFFFFF;
            --brand-soft-bg: #FFF7ED;
            --brand-soft-border: #FED7AA;
            --brand-soft-text: #C2410C;
            --brand-ring: rgba(194, 65, 12, 0.22);
            --link-text: #0369A1;
            --ok-text: #15803D;
            --ok-bg: #DCFCE7;
            --ok-border: #86EFAC;
            --warn-text: #92400E;
            --warn-bg: #FEF3C7;
            --warn-border: #FCD34D;
            --err-text: #B91C1C;
            --err-bg: #FEE2E2;
            --err-border: #FCA5A5;
            --info-text: #0369A1;
            --info-bg: #E0F2FE;
            --info-border: #7DD3FC;
            --purple-text: #6D28D9;
            --purple-bg: #EDE9FE;
            --purple-border: #C4B5FD;
            --pink-text: #BE185D;
            --pink-bg: #FCE7F3;
            --pink-border: #F9A8D4;
            --code-bg: #F1F5F9;
            --code-text: #0F172A;
            --code-border: #E2E8F0;
            --radius-sm: 0.5rem;
            --radius-md: 0.75rem;
            --radius-lg: 1rem;
            --shadow-card: 0 1px 2px rgba(15, 23, 42, 0.05), 0 4px 16px rgba(15, 23, 42, 0.04);
        }
        html[data-theme="dark"] {
            color-scheme: dark;
            --bg-base: #0B1120;
            --bg-sidebar: #0F172A;
            --bg-topbar: rgba(15, 23, 42, 0.92);
            --bg-surface: #131C31;
            --bg-elevated: #18233C;
            --bg-inset: #0D1526;
            --bg-input: #18233C;
            --bg-table-head: #0F172A;
            --bg-table-hover: rgba(51, 65, 85, 0.35);
            --border-subtle: rgba(148, 163, 184, 0.16);
            --border-strong: rgba(148, 163, 184, 0.32);
            --text-primary: #F8FAFC;
            --text-secondary: #E2E8F0;
            --text-muted: #94A3B8;
            --text-placeholder: #64748B;
            --brand-primary: #F97316;
            --brand-hover: #EA580C;
            --brand-bright: #FB923C;
            --brand-accent: #FB923C;
            --brand-contrast: #090D16;
            --brand-soft-bg: rgba(249, 115, 22, 0.16);
            --brand-soft-border: rgba(249, 115, 22, 0.38);
            --brand-soft-text: #FDBA74;
            --brand-ring: rgba(249, 115, 22, 0.28);
            --link-text: #7DD3FC;
            --ok-text: #86EFAC;
            --ok-bg: rgba(34, 197, 94, 0.16);
            --ok-border: rgba(34, 197, 94, 0.38);
            --warn-text: #FDE68A;
            --warn-bg: rgba(245, 158, 11, 0.16);
            --warn-border: rgba(245, 158, 11, 0.38);
            --err-text: #FCA5A5;
            --err-bg: rgba(239, 68, 68, 0.16);
            --err-border: rgba(239, 68, 68, 0.38);
            --info-text: #7DD3FC;
            --info-bg: rgba(14, 165, 233, 0.16);
            --info-border: rgba(14, 165, 233, 0.38);
            --purple-text: #D8B4FE;
            --purple-bg: rgba(168, 85, 247, 0.16);
            --purple-border: rgba(168, 85, 247, 0.38);
            --pink-text: #F9A8D4;
            --pink-bg: rgba(236, 72, 153, 0.16);
            --pink-border: rgba(236, 72, 153, 0.38);
            --code-bg: #1E293B;
            --code-text: #FDE68A;
            --code-border: rgba(148, 163, 184, 0.22);
            --shadow-card: 0 8px 24px -8px rgba(0, 0, 0, 0.45);
        }

        *, *::before, *::after { box-sizing: border-box; }
        html, body { margin: 0; padding: 0; overflow-x: clip; }
        body {
            min-height: 100vh;
            background-color: var(--bg-base);
            color: var(--text-primary);
            font-family: 'Be Vietnam Pro', "Segoe UI", system-ui, -apple-system, sans-serif;
            font-size: 14px;
            line-height: 1.5;
            -webkit-font-smoothing: antialiased;
        }
        a { color: inherit; text-decoration: none; }
        button, input, select, textarea { font-family: inherit; }
        button:focus-visible, a:focus-visible, input:focus-visible, select:focus-visible, textarea:focus-visible, summary:focus-visible {
            outline: 2px solid var(--brand-primary);
            outline-offset: 2px;
        }
        code {
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.78rem;
            background: var(--code-bg);
            color: var(--code-text);
            border: 1px solid var(--code-border);
            padding: 0.1rem 0.4rem;
            border-radius: 0.35rem;
        }

        .app-shell {
            display: grid;
            grid-template-columns: 260px minmax(0, 1fr);
            min-height: 100vh;
        }
        .sidebar {
            position: sticky;
            top: 0;
            height: 100vh;
            background: var(--bg-sidebar);
            border-right: 1px solid var(--border-subtle);
            display: flex;
            flex-direction: column;
            padding: 1.25rem 1rem;
            gap: 1.25rem;
            overflow-y: auto;
            z-index: 45;
        }
        .brand {
            display: flex;
            align-items: center;
            gap: 0.75rem;
            padding: 0.25rem 0.25rem 1rem;
            border-bottom: 1px solid var(--border-subtle);
        }
        .brand__logo-wrap {
            position: relative;
            width: 42px;
            height: 42px;
            flex-shrink: 0;
        }
        .brand__logo {
            width: 42px;
            height: 42px;
            border-radius: 9999px;
            object-fit: cover;
            display: block;
            border: 2px solid var(--brand-bright);
        }
        .brand__dot {
            position: absolute;
            bottom: 0;
            right: 0;
            width: 11px;
            height: 11px;
            border-radius: 9999px;
            background: #22C55E;
            border: 2px solid var(--bg-sidebar);
        }
        .brand__title-row {
            display: flex;
            align-items: center;
            gap: 0.4rem;
            flex-wrap: wrap;
        }
        .brand__title {
            margin: 0;
            font-size: 0.92rem;
            font-weight: 800;
            letter-spacing: -0.02em;
            color: var(--text-primary);
        }
        .brand__tag {
            padding: 0.1rem 0.42rem;
            border-radius: 0.35rem;
            font-size: 0.62rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            color: var(--brand-soft-text);
            background: var(--brand-soft-bg);
            border: 1px solid var(--brand-soft-border);
        }
        .brand__sub {
            margin: 0.18rem 0 0;
            font-size: 0.72rem;
            color: var(--text-muted);
            line-height: 1.35;
        }
        .sidebar__section-label {
            font-size: 0.68rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.08em;
            color: var(--text-muted);
            padding: 0 0.65rem;
            margin-bottom: 0.4rem;
        }
        .tab-nav {
            display: flex;
            flex-direction: column;
            gap: 0.35rem;
        }
        .tab-btn {
            display: flex;
            align-items: center;
            gap: 0.7rem;
            width: 100%;
            min-height: 44px;
            padding: 0.62rem 0.85rem;
            border-radius: var(--radius-md);
            font-size: 0.86rem;
            font-weight: 600;
            color: var(--text-secondary);
            background: transparent;
            border: 1px solid transparent;
            cursor: pointer;
            text-align: left;
            white-space: nowrap;
            transition: background-color 0.15s ease, color 0.15s ease, border-color 0.15s ease;
        }
        .tab-btn__icon {
            width: 28px;
            height: 28px;
            border-radius: 0.5rem;
            display: inline-flex;
            align-items: center;
            justify-content: center;
            background: var(--bg-inset);
            font-size: 0.92rem;
            flex-shrink: 0;
        }
        .tab-btn:hover {
            color: var(--text-primary);
            background: var(--bg-elevated);
            border-color: var(--border-subtle);
        }
        .tab-btn.active {
            color: var(--brand-soft-text);
            background: var(--brand-soft-bg);
            border-color: var(--brand-soft-border);
            font-weight: 700;
            box-shadow: inset 3px 0 0 var(--brand-bright);
        }
        .tab-btn.active .tab-btn__icon {
            background: var(--brand-primary);
            color: var(--brand-contrast);
        }
        .sidebar__footer {
            margin-top: auto;
            display: flex;
            flex-direction: column;
            gap: 0.75rem;
            padding-top: 1rem;
            border-top: 1px solid var(--border-subtle);
        }
        .bot-status-card {
            background: var(--bg-elevated);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 0.75rem 0.85rem;
            display: flex;
            flex-direction: column;
            gap: 0.45rem;
            font-size: 0.78rem;
        }
        .bot-status-card__row {
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 0.4rem;
        }
        .bot-link {
            color: var(--link-text);
            font-weight: 700;
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.76rem;
        }
        .bot-link:hover { text-decoration: underline; }
        .pulse-dot {
            width: 8px;
            height: 8px;
            border-radius: 9999px;
            background: #22C55E;
            display: inline-block;
            flex-shrink: 0;
            box-shadow: 0 0 0 3px rgba(34, 197, 94, 0.22);
        }
        .mode-tag { font-weight: 700; font-size: 0.74rem; }
        .mode-tag--ok { color: var(--ok-text); }
        .mode-tag--info { color: var(--info-text); }

        .shell-main {
            display: flex;
            flex-direction: column;
            min-width: 0;
        }
        .topbar {
            position: sticky;
            top: 0;
            z-index: 40;
            background: var(--bg-topbar);
            backdrop-filter: blur(14px);
            -webkit-backdrop-filter: blur(14px);
            border-bottom: 1px solid var(--border-subtle);
            padding: 0.85rem 1.75rem;
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 1rem;
            flex-wrap: wrap;
        }
        .topbar__left { min-width: 0; }
        .topbar__title {
            margin: 0;
            font-size: 1.15rem;
            font-weight: 800;
            color: var(--text-primary);
            letter-spacing: -0.02em;
        }
        .topbar__sub {
            margin: 0.12rem 0 0;
            font-size: 0.78rem;
            color: var(--text-muted);
        }
        .topbar__actions {
            display: flex;
            align-items: center;
            gap: 0.6rem;
            flex-wrap: wrap;
        }
        .admin-pill {
            display: inline-flex;
            align-items: center;
            gap: 0.45rem;
            min-height: 36px;
            padding: 0.35rem 0.75rem;
            background: var(--bg-elevated);
            border: 1px solid var(--border-subtle);
            border-radius: 9999px;
            font-size: 0.78rem;
            color: var(--text-muted);
        }
        .admin-pill strong { color: var(--text-primary); }
        .theme-btn {
            display: inline-flex;
            align-items: center;
            gap: 0.38rem;
            min-height: 36px;
            padding: 0.38rem 0.85rem;
            border-radius: 9999px;
            background: var(--bg-elevated);
            border: 1px solid var(--border-strong);
            color: var(--text-primary);
            font-size: 0.78rem;
            font-weight: 700;
            cursor: pointer;
            transition: border-color 0.15s ease, background-color 0.15s ease;
        }
        .theme-btn:hover {
            border-color: var(--brand-primary);
            background: var(--bg-inset);
        }
        .admin-main {
            width: 100%;
            max-width: 1380px;
            margin: 0 auto;
            padding: 1.5rem 1.75rem 2.5rem;
            flex: 1;
            display: flex;
            flex-direction: column;
            gap: 1.25rem;
        }
        .admin-footer {
            width: 100%;
            max-width: 1380px;
            margin: 0 auto;
            padding: 1.15rem 1.75rem 1.75rem;
            text-align: center;
            font-size: 0.78rem;
            color: var(--text-muted);
            border-top: 1px solid var(--border-subtle);
        }
        .tab-pane {
            display: flex;
            flex-direction: column;
            gap: 1.35rem;
        }
        .tab-pane.is-hidden { display: none !important; }

        .notice-banner {
            display: flex;
            align-items: flex-start;
            gap: 0.6rem;
            padding: 0.72rem 1rem;
            border-radius: var(--radius-md);
            font-size: 0.84rem;
            font-weight: 500;
            line-height: 1.5;
            border: 1px solid var(--border-subtle);
        }
        .notice-banner--ok {
            background: var(--bg-surface);
            border-color: var(--border-subtle);
            border-left: 4px solid var(--ok-text);
            color: var(--text-secondary);
        }
        .notice-banner--warn {
            background: var(--warn-bg);
            border-color: var(--warn-border);
            border-left: 4px solid var(--warn-text);
            color: var(--warn-text);
        }
        .notice-banner--info {
            background: var(--info-bg);
            border-color: var(--info-border);
            border-left: 4px solid var(--info-text);
            color: var(--info-text);
        }

        .card {
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-lg);
            padding: 1.4rem 1.5rem;
            box-shadow: var(--shadow-card);
            display: flex;
            flex-direction: column;
            gap: 1.15rem;
        }
        .card--table {
            padding: 0;
            gap: 0;
            overflow: hidden;
        }
        .card--orange { border-top: 3px solid var(--brand-bright); }
        .card--green { border-top: 3px solid var(--ok-text); }
        .card--purple { border-top: 3px solid var(--purple-text); }
        .card--social {
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-left: 4px solid var(--brand-bright);
            flex-direction: row;
            flex-wrap: wrap;
            align-items: center;
            justify-content: space-between;
            gap: 1rem;
        }
        .social-left {
            display: flex;
            align-items: center;
            gap: 0.95rem;
            flex: 1 1 380px;
        }
        .social-icon {
            width: 46px;
            height: 46px;
            border-radius: 0.85rem;
            background: var(--brand-soft-bg);
            border: 1px solid var(--brand-soft-border);
            display: flex;
            align-items: center;
            justify-content: center;
            font-size: 1.35rem;
            flex-shrink: 0;
            color: var(--brand-primary);
        }
        .social-meta {
            display: flex;
            flex-direction: column;
            align-items: flex-end;
            gap: 0.35rem;
            text-align: right;
            min-width: 225px;
        }
        .meta-time {
            color: var(--text-primary);
            font-family: 'JetBrains Mono', monospace;
            font-weight: 600;
        }
        .card-head {
            display: flex;
            flex-wrap: wrap;
            align-items: flex-start;
            justify-content: space-between;
            gap: 0.75rem;
            padding-bottom: 0.95rem;
            border-bottom: 1px solid var(--border-subtle);
        }
        .card-head--table {
            padding: 1.15rem 1.5rem;
            margin: 0;
            background: var(--bg-elevated);
        }
        .card-head-actions {
            display: flex;
            align-items: center;
            gap: 0.55rem;
            flex-wrap: wrap;
        }
        .card-title {
            margin: 0;
            font-size: 1.02rem;
            font-weight: 700;
            color: var(--text-primary);
            display: flex;
            flex-wrap: wrap;
            align-items: center;
            gap: 0.5rem;
        }
        .card-desc {
            margin: 0.32rem 0 0;
            font-size: 0.82rem;
            color: var(--text-muted);
            line-height: 1.55;
        }

        .kpi-grid {
            display: grid;
            grid-template-columns: repeat(4, minmax(0, 1fr));
            gap: 1rem;
        }
        .kpi-card {
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-lg);
            padding: 1.2rem 1.25rem;
            display: flex;
            flex-direction: column;
            justify-content: space-between;
            gap: 0.9rem;
            box-shadow: var(--shadow-card);
            transition: border-color 0.15s ease;
        }
        .kpi-card:hover { border-color: var(--border-strong); }
        .kpi-card--orange { border-top: 3px solid var(--brand-bright); }
        .kpi-card--green { border-top: 3px solid var(--ok-text); }
        .kpi-card__top {
            display: flex;
            align-items: center;
            justify-content: space-between;
            font-size: 0.73rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            color: var(--text-muted);
        }
        .kpi-card__icon {
            width: 2rem;
            height: 2rem;
            border-radius: 0.55rem;
            display: inline-flex;
            align-items: center;
            justify-content: center;
            font-size: 0.95rem;
            background: var(--bg-inset);
            border: 1px solid var(--border-subtle);
        }
        .kpi-card__val {
            margin-top: 0.45rem;
            font-size: 1.95rem;
            font-weight: 800;
            line-height: 1.15;
            letter-spacing: -0.02em;
            font-variant-numeric: tabular-nums;
            color: var(--text-primary);
        }
        .kpi-card__val--amber, .kpi-card__val--orange { color: var(--brand-primary); }
        .kpi-card__val--green { color: var(--ok-text); font-size: 1rem; font-weight: 700; margin-top: 0.5rem; }
        .kpi-card__meta {
            background: var(--bg-inset);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 0.58rem 0.75rem;
            font-size: 0.76rem;
            color: var(--text-secondary);
            display: flex;
            flex-direction: column;
            gap: 0.32rem;
        }
        .kpi-card__pill {
            background: var(--bg-inset);
            border: 1px solid var(--border-subtle);
            color: var(--text-secondary);
            border-radius: var(--radius-sm);
            padding: 0.48rem 0.72rem;
            font-size: 0.76rem;
            font-weight: 600;
        }
        .refresh-chip {
            background: var(--info-bg);
            border: 1px solid var(--info-border);
            color: var(--info-text);
            border-radius: var(--radius-sm);
            padding: 0.48rem 0.72rem;
            font-size: 0.76rem;
            font-weight: 700;
            text-align: center;
        }
        .kpi-row {
            display: flex;
            justify-content: space-between;
            align-items: center;
            gap: 0.5rem;
            flex-wrap: wrap;
        }
        .kpi-row strong { font-family: 'JetBrains Mono', monospace; color: var(--text-primary); }

        .split-at {
            display: grid;
            grid-template-columns: minmax(340px, 1.08fr) minmax(0, 1.42fr);
            gap: 1.5rem;
            align-items: start;
        }
        .grid-2 {
            display: grid;
            grid-template-columns: repeat(2, minmax(0, 1fr));
            gap: 1.25rem;
        }
        .grid-3 {
            display: grid;
            grid-template-columns: repeat(3, minmax(0, 1fr));
            gap: 1rem;
        }
        .form-grid-2 {
            display: grid;
            grid-template-columns: repeat(2, minmax(0, 1fr));
            gap: 0.85rem;
        }

        .plat-panel {
            background: var(--bg-elevated);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 1rem;
            display: flex;
            flex-direction: column;
            gap: 0.85rem;
        }
        .plat-panel__title {
            font-size: 0.73rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.06em;
            color: var(--text-muted);
        }
        .plat-list { display: flex; flex-direction: column; gap: 0.55rem; }
        .plat-row {
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 0.75rem;
            padding: 0.65rem 0.85rem;
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
        }
        .plat-row__left { display: flex; flex-direction: column; gap: 0.18rem; min-width: 0; }
        .plat-row__name { font-size: 0.85rem; font-weight: 600; color: var(--text-primary); }
        .plat-row__sub {
            font-size: 0.74rem;
            color: var(--text-muted);
            font-family: 'JetBrains Mono', monospace;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
        }
        .inset-note {
            background: var(--bg-surface);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-sm);
            padding: 0.78rem 0.9rem;
            font-size: 0.78rem;
            color: var(--text-muted);
            line-height: 1.55;
            display: flex;
            flex-direction: column;
            gap: 0.3rem;
        }
        .inset-note strong, .inset-note b { color: var(--text-primary); }
        .inset-note--ok {
            border-color: var(--ok-border);
            background: var(--ok-bg);
            color: var(--ok-text);
        }
        .err-text { color: var(--err-text); font-size: 0.82rem; font-weight: 600; }
        .muted-text { color: var(--text-muted); font-size: 0.78rem; margin-top: 0.3rem; }
        .ok-strong { color: var(--ok-text); }
        .primary-strong { color: var(--text-primary); }
        .label-accent { color: var(--brand-soft-text); font-weight: 700; }
        .label-info { color: var(--info-text); font-weight: 700; }
        .warn-hint { color: var(--warn-text); }
        .comm-val { color: var(--ok-text); font-weight: 700; }
        .font-mono { font-family: 'JetBrains Mono', monospace; }
        .inline-form { margin: 0; }

        .badge {
            display: inline-flex;
            align-items: center;
            gap: 0.3rem;
            padding: 0.28rem 0.65rem;
            border-radius: 9999px;
            font-size: 0.74rem;
            font-weight: 600;
            border: 1px solid transparent;
            white-space: nowrap;
            line-height: 1.25;
            flex-shrink: 0;
        }
        .badge--ok { background: var(--ok-bg); border-color: var(--ok-border); color: var(--ok-text); }
        .badge--err { background: var(--err-bg); border-color: var(--err-border); color: var(--err-text); }
        .badge--warn { background: var(--warn-bg); border-color: var(--warn-border); color: var(--warn-text); }
        .badge--info { background: var(--info-bg); border-color: var(--info-border); color: var(--info-text); }
        .badge--muted {
            background: var(--bg-inset);
            border-color: var(--border-strong);
            color: var(--text-secondary);
        }
        .badge--orange {
            background: var(--brand-soft-bg);
            border-color: var(--brand-soft-border);
            color: var(--brand-soft-text);
        }
        .badge--purple { background: var(--purple-bg); border-color: var(--purple-border); color: var(--purple-text); }
        .badge--pink { background: var(--pink-bg); border-color: var(--pink-border); color: var(--pink-text); }
        .badge--cyan {
            background: var(--info-bg);
            border-color: var(--info-border);
            color: var(--info-text);
        }
        .code-pill {
            display: inline-block;
            padding: 0.22rem 0.55rem;
            border-radius: 0.45rem;
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.76rem;
            font-weight: 600;
            background: var(--code-bg);
            color: var(--code-text);
            border: 1px solid var(--code-border);
        }

        .form-stack { display: flex; flex-direction: column; gap: 1rem; }
        .subpanel {
            background: var(--bg-elevated);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 1.15rem;
            display: flex;
            flex-direction: column;
            gap: 0.85rem;
        }
        .subpanel__title {
            font-size: 0.76rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            color: var(--text-secondary);
        }
        .subpanel__title--purple { color: var(--purple-text); }
        .divider-top { border-top: 1px solid var(--border-subtle); padding-top: 0.95rem; }
        .field-label {
            display: block;
            font-size: 0.78rem;
            font-weight: 600;
            color: var(--text-secondary);
            margin-bottom: 0.35rem;
        }
        input[type="text"],
        input[type="password"],
        input[type="url"],
        input[type="number"],
        select,
        textarea,
        .field-input,
        .code-textarea {
            width: 100%;
            min-height: 42px;
            background: var(--bg-input);
            border: 1px solid var(--border-strong);
            border-radius: var(--radius-md);
            color: var(--text-primary);
            padding: 0.58rem 0.85rem;
            font-size: 0.84rem;
            transition: border-color 0.15s ease, box-shadow 0.15s ease;
        }
        textarea.code-textarea { min-height: 110px; }
        .field-input--mono, .code-textarea {
            font-family: 'JetBrains Mono', monospace;
            font-size: 0.8rem;
        }
        input::placeholder, textarea::placeholder { color: var(--text-placeholder); }
        input:focus, select:focus, textarea:focus {
            outline: none;
            border-color: var(--brand-primary);
            box-shadow: 0 0 0 3px var(--brand-ring);
        }
        .check-row {
            display: inline-flex;
            align-items: center;
            gap: 0.55rem;
            font-size: 0.82rem;
            color: var(--text-secondary);
            cursor: pointer;
        }
        .check-row--mt { margin-top: 0.45rem; }
        .check-row input[type="checkbox"] {
            width: 1.05rem;
            height: 1.05rem;
            accent-color: var(--brand-primary);
            cursor: pointer;
        }
        .filter-bar {
            display: flex;
            flex-wrap: wrap;
            align-items: center;
            gap: 0.65rem;
            background: var(--bg-elevated);
            border: 1px solid var(--border-subtle);
            padding: 0.78rem 0.95rem;
            border-radius: var(--radius-md);
        }
        .filter-bar select { width: auto; min-width: 195px; }
        .inline-form-grow {
            display: flex;
            flex: 1 1 280px;
            align-items: flex-end;
            gap: 0.65rem;
            flex-wrap: wrap;
        }
        .grow-field { flex: 1 1 220px; min-width: 180px; width: auto !important; }

        .btn {
            display: inline-flex;
            align-items: center;
            justify-content: center;
            gap: 0.42rem;
            min-height: 40px;
            padding: 0.55rem 1.05rem;
            border-radius: var(--radius-md);
            font-size: 0.82rem;
            font-weight: 700;
            border: 1px solid transparent;
            cursor: pointer;
            text-decoration: none;
            white-space: nowrap;
            transition: background-color 0.15s ease, border-color 0.15s ease, color 0.15s ease;
        }
        .btn--primary {
            background: var(--brand-primary);
            color: var(--brand-contrast);
            border-color: var(--brand-primary);
        }
        .btn--primary:hover {
            background: var(--brand-hover);
            border-color: var(--brand-hover);
        }
        .btn--secondary {
            background: var(--bg-surface);
            color: var(--text-primary);
            border-color: var(--border-strong);
        }
        .btn--secondary:hover {
            background: var(--bg-inset);
            border-color: var(--brand-primary);
        }
        .btn--success {
            background: var(--ok-text);
            color: #FFFFFF;
            border-color: var(--ok-text);
        }
        .btn--success:hover { opacity: 0.92; }
        .btn--purple {
            background: var(--purple-text);
            color: #FFFFFF;
            border-color: var(--purple-text);
        }
        .btn--purple:hover { opacity: 0.92; }
        html[data-theme="dark"] .btn--success,
        html[data-theme="dark"] .btn--purple { color: #090D16; }
        .btn--danger {
            background: var(--err-bg);
            color: var(--err-text);
            border-color: var(--err-border);
        }
        .btn--danger:hover { opacity: 0.9; }
        .btn--orange-soft {
            background: var(--brand-soft-bg);
            color: var(--brand-soft-text);
            border-color: var(--brand-soft-border);
        }
        .btn--orange-soft:hover { border-color: var(--brand-primary); }
        .btn--sm { min-height: 34px; padding: 0.35rem 0.78rem; font-size: 0.76rem; border-radius: var(--radius-sm); }
        .btn--block { width: 100%; }
        .row-actions { display: flex; align-items: center; gap: 0.45rem; flex-wrap: nowrap; }

        .table-wrap {
            overflow-x: auto;
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            background: var(--bg-surface);
        }
        .card--table .table-wrap { border: none; border-radius: 0; }
        .data-table {
            width: 100%;
            border-collapse: collapse;
            text-align: left;
            font-size: 0.82rem;
        }
        .data-table thead {
            background: var(--bg-table-head);
            border-bottom: 1px solid var(--border-subtle);
        }
        .data-table th {
            padding: 0.82rem 1rem;
            font-size: 0.72rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            color: var(--text-muted);
            white-space: nowrap;
        }
        .data-table td {
            padding: 0.82rem 1rem;
            border-bottom: 1px solid var(--border-subtle);
            vertical-align: middle;
        }
        .data-table tbody tr:hover { background: var(--bg-table-hover); }
        .data-table tbody tr:last-child td { border-bottom: none; }
        .cell-strong { font-weight: 600; color: var(--text-primary); }
        .cell-sub { font-size: 0.76rem; color: var(--text-muted); margin-top: 0.22rem; }
        .cell-muted { color: var(--text-muted); font-size: 0.78rem; }
        .table-link { color: var(--link-text); font-weight: 600; }
        .table-link:hover { text-decoration: underline; }
        .table-link--amber { color: var(--brand-primary); font-weight: 700; }
        .truncate-url {
            max-width: 260px;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
        }
        .empty-cell {
            padding: 2.25rem 1rem !important;
            text-align: center;
            color: var(--text-muted);
            font-size: 0.84rem;
        }

        .cf-details {
            background: var(--bg-elevated);
            border: 1px solid var(--border-subtle);
            border-radius: var(--radius-md);
            padding: 0.78rem 1rem;
        }
        .cf-summary {
            cursor: pointer;
            font-size: 0.82rem;
            font-weight: 700;
            color: var(--brand-soft-text);
        }
        .cf-body {
            margin-top: 0.78rem;
            font-size: 0.8rem;
            color: var(--text-secondary);
            line-height: 1.55;
            display: flex;
            flex-direction: column;
            gap: 0.5rem;
        }
        .cf-steps {
            margin: 0.25rem 0 0 1.25rem;
            padding: 0;
            display: flex;
            flex-direction: column;
            gap: 0.35rem;
        }

        .ico { width: 18px; height: 18px; display: inline-block; vertical-align: middle; flex-shrink: 0; }
        .ico--sm { width: 16px; height: 16px; }
        .ico--xs { width: 14px; height: 14px; }
        .kpi-card__icon--blue { background: var(--info-bg); color: var(--info-text); border-color: var(--info-border); }
        .kpi-card__icon--amber { background: var(--warn-bg); color: var(--warn-text); border-color: var(--warn-border); }
        .kpi-card__icon--orange { background: var(--brand-soft-bg); color: var(--brand-soft-text); border-color: var(--brand-soft-border); }
        .kpi-card__icon--green { background: var(--ok-bg); color: var(--ok-text); border-color: var(--ok-border); }
        .signal-meter {
            display: inline-flex; align-items: center; gap: 0.4rem;
            padding: 0.28rem 0.65rem; border-radius: 9999px;
            background: var(--bg-elevated); border: 1px solid var(--border-subtle);
            font-size: 0.74rem; font-weight: 700; color: var(--text-secondary);
        }

        .charts-grid {
            display: grid;
            grid-template-columns: repeat(2, minmax(0, 1fr));
            gap: 1.15rem;
        }
        .chart-card { gap: 1rem; transition: border-color 0.16s ease, box-shadow 0.16s ease; }
        .chart-card:hover { border-color: var(--border-strong); }
        .chart-head {
            display: flex; align-items: flex-start; justify-content: space-between;
            gap: 0.75rem; flex-wrap: wrap;
            padding-bottom: 0.85rem; border-bottom: 1px solid var(--border-subtle);
        }
        .chart-head__title { display: flex; align-items: center; gap: 0.72rem; }
        .chart-head__title h3 { margin: 0; font-size: 0.96rem; font-weight: 700; color: var(--text-primary); }
        .chart-head__title p { margin: 0.15rem 0 0; font-size: 0.76rem; color: var(--text-muted); }
        .chart-ico {
            width: 38px; height: 38px; border-radius: 0.68rem;
            display: inline-flex; align-items: center; justify-content: center; flex-shrink: 0;
            border: 1px solid var(--border-subtle);
        }
        .chart-ico--brand { background: var(--brand-soft-bg); color: var(--brand-soft-text); border-color: var(--brand-soft-border); }
        .chart-ico--purple { background: var(--purple-bg); color: var(--purple-text); border-color: var(--purple-border); }

        .chart-bars { display: flex; flex-direction: column; gap: 0.85rem; }
        .chart-bar-row {
            display: grid; grid-template-columns: 108px minmax(0, 1fr) 82px;
            align-items: center; gap: 0.75rem;
        }
        .chart-bar-label {
            display: inline-flex; align-items: center; gap: 0.45rem;
            font-size: 0.82rem; font-weight: 600; color: var(--text-secondary);
            white-space: nowrap;
        }
        .plat-dot, .legend-dot { width: 9px; height: 9px; border-radius: 9999px; flex-shrink: 0; }
        .bar-track {
            height: 10px; border-radius: 9999px; background: var(--bg-inset);
            border: 1px solid var(--border-subtle); overflow: hidden;
        }
        .bar-fill { height: 100%; border-radius: 9999px; transition: width 0.35s ease; }
        .chart-bar-val {
            text-align: right; font-size: 0.8rem; font-family: 'JetBrains Mono', monospace;
            color: var(--text-primary); white-space: nowrap;
        }
        .chart-bar-val span { color: var(--text-muted); font-size: 0.73rem; margin-left: 0.25rem; }

        .donut-layout {
            display: flex; align-items: center; justify-content: space-between;
            gap: 1.25rem; flex-wrap: wrap;
        }
        .donut-wrap {
            position: relative; width: 124px; height: 124px; flex-shrink: 0;
            display: inline-flex; align-items: center; justify-content: center;
        }
        .chart-donut { display: block; }
        .donut-center {
            position: absolute; inset: 0; display: flex; flex-direction: column;
            align-items: center; justify-content: center; pointer-events: none;
        }
        .donut-center strong {
            font-size: 1.35rem; font-weight: 800; line-height: 1.1;
            font-variant-numeric: tabular-nums; color: var(--text-primary);
        }
        .donut-center span { font-size: 0.7rem; font-weight: 600; color: var(--text-muted); }
        .donut-legend { flex: 1 1 200px; display: flex; flex-direction: column; gap: 0.68rem; }
        .legend-item {
            display: flex; align-items: center; gap: 0.5rem;
            padding: 0.45rem 0.7rem; border-radius: var(--radius-sm);
            background: var(--bg-elevated); border: 1px solid var(--border-subtle);
            font-size: 0.8rem;
        }
        .legend-name { flex: 1; color: var(--text-secondary); font-weight: 600; }
        .legend-item strong { font-family: 'JetBrains Mono', monospace; color: var(--text-primary); }
        .legend-pct { font-family: 'JetBrains Mono', monospace; font-size: 0.74rem; color: var(--text-muted); }

        ::-webkit-scrollbar { width: 7px; height: 7px; }
        ::-webkit-scrollbar-track { background: var(--bg-base); }
        ::-webkit-scrollbar-thumb { background: var(--border-strong); border-radius: 9999px; }
        ::-webkit-scrollbar-thumb:hover { background: var(--brand-primary); }

        @media (max-width: 1180px) {
            .kpi-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
            .charts-grid, .split-at, .grid-2, .grid-3 { grid-template-columns: minmax(0, 1fr); }
        }
        @media (max-width: 960px) {
            .app-shell { grid-template-columns: minmax(0, 1fr); }
            .sidebar {
                position: static; height: auto; border-right: none;
                border-bottom: 1px solid var(--border-subtle); padding: 0.9rem 1rem;
                gap: 0.8rem; overflow-y: visible;
            }
            .brand { padding-bottom: 0.75rem; }
            .sidebar__section-label { display: none; }
            .tab-nav { flex-direction: row; overflow-x: auto; padding-bottom: 0.2rem; }
            .tab-btn { width: auto; flex: 0 0 auto; padding: 0.5rem 0.85rem; }
            .tab-btn.active { box-shadow: none; }
            .sidebar__footer { display: none; }
            .topbar { padding: 0.75rem 1rem; }
        }
        @media (max-width: 640px) {
            .kpi-grid, .form-grid-2 { grid-template-columns: minmax(0, 1fr); }
            .chart-bar-row { grid-template-columns: 92px minmax(0, 1fr) 72px; gap: 0.5rem; }
            .admin-main { padding: 1rem; }
            .hide-sm { display: none !important; }
            .social-meta { align-items: flex-start; text-align: left; min-width: 0; }
        }
        @media (prefers-reduced-motion: no-preference) {
            .kpi-card:hover, .chart-card:hover { transform: translateY(-2px); }
        }
        @media (prefers-reduced-motion: reduce) {
            *, *::before, *::after { transition: none !important; animation: none !important; }
        }
    </style>`

const adminJSContent = `
        const TAB_META = {
            overview: {
                title: 'Tổng Quan Hệ Thống',
                sub: 'Theo dõi lưu lượng Bot Telegram, Kho Mã Thị Trường & Social Harvester thời gian thực'
            },
            platforms: {
                title: 'Kết Nối Sàn & API',
                sub: 'Cấu hình ACCESSTRADE đa sàn, Shopee Chính Chủ & Cloudflare Tunnel tự động'
            },
            vouchers: {
                title: 'Kho Voucher Thị Trường',
                sub: 'Tra cứu mã giảm giá quét tự động và quản lý Mã Độc Quyền Hợp Tác'
            },
            orders: {
                title: 'Báo Đơn & Doanh Thu',
                sub: 'Cài đặt thông báo Telegram Admin và đối soát hoa hồng đơn hàng'
            }
        };

        function syncThemeButton() {
            const btn = document.getElementById('theme-toggle-btn');
            if (!btn) return;
            const isDark = document.documentElement.getAttribute('data-theme') === 'dark';
            const useEl = document.getElementById('theme-ico-use');
            const txtEl = document.getElementById('theme-btn-text');
            if (useEl) useEl.setAttribute('href', isDark ? '#ico-sun' : '#ico-moon');
            if (txtEl) txtEl.textContent = isDark ? 'Sáng' : 'Tối';
            btn.setAttribute('aria-pressed', isDark ? 'true' : 'false');
        }

        function toggleTheme() {
            const isDark = document.documentElement.getAttribute('data-theme') === 'dark';
            const next = isDark ? 'light' : 'dark';
            if (next === 'dark') {
                document.documentElement.setAttribute('data-theme', 'dark');
            } else {
                document.documentElement.removeAttribute('data-theme');
            }
            try { localStorage.setItem('dealx-theme', next); } catch (e) {}
            syncThemeButton();
        }

        function switchTab(tabId) {
            document.querySelectorAll('.tab-pane').forEach(el => el.classList.add('is-hidden'));
            document.querySelectorAll('.tab-btn').forEach(btn => {
                btn.classList.remove('active');
                btn.removeAttribute('aria-current');
            });
            const target = document.getElementById('tab-pane-' + tabId);
            const btn = document.getElementById('tab-btn-' + tabId);
            if (target) target.classList.remove('is-hidden');
            if (btn) {
                btn.classList.add('active');
                btn.setAttribute('aria-current', 'page');
            }
            const meta = TAB_META[tabId];
            if (meta) {
                const tEl = document.getElementById('top-page-title');
                const sEl = document.getElementById('top-page-subtitle');
                if (tEl) tEl.textContent = meta.title;
                if (sEl) sEl.textContent = meta.sub;
            }
            try {
                history.replaceState(null, '', '#' + tabId);
            } catch (e) {}
        }

        window.addEventListener('DOMContentLoaded', () => {
            syncThemeButton();
            const themeBtn = document.getElementById('theme-toggle-btn');
            if (themeBtn) {
                themeBtn.addEventListener('click', toggleTheme);
            }
            document.querySelectorAll('.tab-btn[data-tab]').forEach((btn) => {
                btn.addEventListener('click', () => {
                    const tabId = btn.getAttribute('data-tab');
                    if (tabId) switchTab(tabId);
                });
            });
            const csrfMeta = document.querySelector('meta[name="csrf-token"]');
            const csrfVal = csrfMeta ? csrfMeta.getAttribute('content') || '' : '';
            if (csrfVal) {
                document.querySelectorAll('form').forEach((formEl) => {
                    if ((formEl.method || 'GET').toUpperCase() === 'POST' && !formEl.querySelector('input[name="_csrf"]')) {
                        const hidden = document.createElement('input');
                        hidden.type = 'hidden';
                        hidden.name = '_csrf';
                        hidden.value = csrfVal;
                        formEl.appendChild(hidden);
                    }
                });
                if (!window.__dealxCsrfFetchWrapped && typeof window.fetch === 'function') {
                    window.__dealxCsrfFetchWrapped = true;
                    const origFetch = window.fetch.bind(window);
                    window.fetch = (input, init = {}) => {
                        const method = String((init && init.method) || (input && input.method) || 'GET').toUpperCase();
                        const reqUrl = typeof input === 'string' ? input : (input && input.url) || '';
                        const isSameOrigin = reqUrl.startsWith('/') || reqUrl.startsWith(window.location.origin);
                        if (isSameOrigin && method !== 'GET' && method !== 'HEAD') {
                            const headers = new Headers((init && init.headers) || {});
                            if (!headers.has('X-CSRF-Token')) {
                                headers.set('X-CSRF-Token', csrfVal);
                            }
                            return origFetch(input, Object.assign({}, init, { headers }));
                        }
                        return origFetch(input, init);
                    };
                }
            }
            let hash = window.location.hash.replace('#', '');
            if (!['overview', 'platforms', 'vouchers', 'orders'].includes(hash)) {
                const urlParams = new URLSearchParams(window.location.search);
                if (urlParams.has('q') || urlParams.has('plat')) {
                    hash = 'vouchers';
                } else {
                    hash = 'overview';
                }
            }
            switchTab(hash);
        });

        let refreshingOverview = false;
        setInterval(async () => {
            if (document.hidden || refreshingOverview) return;
            const overviewPane = document.getElementById('tab-pane-overview');
            if (!overviewPane || overviewPane.classList.contains('is-hidden')) return;
            const activeTag = document.activeElement ? document.activeElement.tagName : '';
            if (['INPUT', 'TEXTAREA', 'SELECT'].includes(activeTag)) return;
            refreshingOverview = true;
            try {
                const res = await fetch('/admin', { cache: 'no-store' });
                if (!res.ok) return;
                const html = await res.text();
                const doc = new DOMParser().parseFromString(html, 'text/html');
                const savedScrollY = window.scrollY;
                ['.kpi-grid', '.charts-grid', '.card--social', '#clicks-tbody'].forEach((sel) => {
                    const cur = document.querySelector(sel);
                    const next = doc.querySelector(sel);
                    if (cur && next && cur.innerHTML !== next.innerHTML) {
                        cur.innerHTML = next.innerHTML;
                    }
                });
                if (Math.abs(window.scrollY - savedScrollY) > 4) {
                    window.scrollTo(0, savedScrollY);
                }
            } catch (e) {} finally {
                refreshingOverview = false;
            }
        }, 30000);
`

// HandleAdminJS phục vụ `/assets/admin.js` tách khỏi inline script để bật Strict CSP.
func HandleAdminJS(w http.ResponseWriter, _ *http.Request) {
	w.Header().Set("Content-Type", "application/javascript; charset=utf-8")
	w.Header().Set("Cache-Control", "public, max-age=3600")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte(adminJSContent))
}

func renderPlatformStatus(label string, matched *accesstrade.CampaignPair, manualID, backupNote string) string {
	if strings.TrimSpace(manualID) != "" {
		return fmt.Sprintf(`<div class="plat-row">
                <div class="plat-row__left">
                    <div class="plat-row__name">%s</div>
                    <div class="plat-row__sub">ID thủ công: <code>%s</code></div>
                </div>
                <span class="badge badge--ok">🟢 Thủ công</span>
             </div>`, label, utils.EscapeHTML(strings.TrimSpace(manualID)))
	}
	if matched != nil {
		return fmt.Sprintf(`<div class="plat-row">
                <div class="plat-row__left">
                    <div class="plat-row__name">%s</div>
                    <div class="plat-row__sub">Campaign: <code>%s</code></div>
                </div>
                <span class="badge badge--ok">🟢 Đã duyệt</span>
             </div>`, label, utils.EscapeHTML(matched.ID))
	}
	if backupNote != "" {
		return fmt.Sprintf(`<div class="plat-row">
                        <div class="plat-row__left">
                            <div class="plat-row__name">%s</div>
                            <div class="plat-row__sub">%s</div>
                        </div>
                        <span class="badge badge--info">🔵 Sẵn sàng</span>
                     </div>`, label, utils.EscapeHTML(backupNote))
	}
	return fmt.Sprintf(`<div class="plat-row">
                        <div class="plat-row__left">
                            <div class="plat-row__name">%s</div>
                            <div class="plat-row__sub">Chưa duyệt trên pub2.accesstrade.vn</div>
                        </div>
                        <span class="badge badge--warn">🟡 Chưa duyệt AT</span>
                     </div>`, label)
}

func renderSignalMeterSVG(strength uint8) string {
	lvl := strength
	if lvl > 3 {
		lvl = 3
	}
	color := "var(--border-strong)"
	if lvl >= 3 {
		color = "var(--ok-text)"
	} else if lvl >= 1 {
		color = "var(--brand-bright)"
	}
	bar := func(idx, h, y uint8) string {
		fill := "var(--border-strong)"
		if lvl >= idx {
			fill = color
		}
		return fmt.Sprintf(`<rect x="%d" y="%d" width="4" height="%d" rx="1.5" fill="%s"/>`, (idx-1)*6+1, y, h, fill)
	}
	return fmt.Sprintf(`<span class="signal-meter" title="Độ mạnh chữ ký FB/IG: %d/3">
            <svg width="20" height="16" viewBox="0 0 20 16" aria-hidden="true">%s%s%s</svg>
            <span>Mức %d/3</span>
        </span>`, lvl, bar(1, 6, 10), bar(2, 10, 6), bar(3, 14, 2), lvl)
}

func renderDashboardChartsHTML(
	stats db.AdminStats,
	totalMarketVouchers, highPctVouchers, shopeeVouchers, tikiVouchers, exclusiveCount int,
) string {
	maxZero := func(v int64) uint64 {
		if v <= 0 {
			return 0
		}
		return uint64(v)
	}
	shopeeC := maxZero(stats.ShopeeClicks)
	lazadaC := maxZero(stats.LazadaClicks)
	tiktokC := maxZero(stats.TikTokClicks)
	tikiC := maxZero(stats.TikiClicks)
	sumClicks := shopeeC + lazadaC + tiktokC + tikiC
	if tc := maxZero(stats.TotalClicks); tc > sumClicks {
		sumClicks = tc
	}

	type platStat struct {
		name  string
		count uint64
		color string
	}
	platforms := []platStat{
		{"Shopee", shopeeC, "#EA580C"},
		{"Lazada", lazadaC, "#2563EB"},
		{"TikTok Shop", tiktokC, "#DB2777"},
		{"Tiki", tikiC, "#0284C7"},
	}

	var barsHTML strings.Builder
	for _, p := range platforms {
		var pct uint64
		if sumClicks > 0 {
			pct = uint64(math.Round((float64(p.count) / float64(sumClicks)) * 100.0))
		}
		widthPct := uint64(0)
		if p.count > 0 {
			widthPct = pct
			if widthPct < 4 {
				widthPct = 4
			}
		}
		barsHTML.WriteString(fmt.Sprintf(`<div class="chart-bar-row">
                <div class="chart-bar-label">
                    <span class="plat-dot" style="background:%s;"></span>
                    <span>%s</span>
                </div>
                <div class="bar-track" role="img" aria-label="%s: %d lượt (%d%%)">
                    <div class="bar-fill" style="width:%d%%;background:%s;"></div>
                </div>
                <div class="chart-bar-val"><strong>%d</strong> <span>%d%%</span></div>
            </div>`, p.color, p.name, p.name, p.count, pct, widthPct, p.color, p.count, pct))
	}

	circumference := 276.46
	otherV := totalMarketVouchers - shopeeVouchers - tikiVouchers
	if otherV < 0 {
		otherV = 0
	}
	otherV += exclusiveCount
	poolTotal := shopeeVouchers + tikiVouchers + otherV

	segCalc := func(val int) (float64, uint64) {
		if poolTotal == 0 {
			return 0, 0
		}
		ratio := float64(val) / float64(poolTotal)
		return ratio * circumference, uint64(math.Round(ratio * 100.0))
	}
	lenS, pctS := segCalc(shopeeVouchers)
	lenT, pctT := segCalc(tikiVouchers)
	lenO, pctO := segCalc(otherV)
	var deepPct uint64
	if totalMarketVouchers > 0 {
		deepPct = uint64(math.Round((float64(highPctVouchers) / float64(totalMarketVouchers)) * 100.0))
	}

	donutSegments := ""
	if poolTotal > 0 {
		donutSegments = fmt.Sprintf(
			`<circle cx="60" cy="60" r="44" fill="none" stroke="#EA580C" stroke-width="12" stroke-dasharray="%.2f %.2f" stroke-dashoffset="0" stroke-linecap="butt"/>
               <circle cx="60" cy="60" r="44" fill="none" stroke="#0284C7" stroke-width="12" stroke-dasharray="%.2f %.2f" stroke-dashoffset="%.2f" stroke-linecap="butt"/>
               <circle cx="60" cy="60" r="44" fill="none" stroke="#7C3AED" stroke-width="12" stroke-dasharray="%.2f %.2f" stroke-dashoffset="%.2f" stroke-linecap="butt"/>`,
			lenS, circumference, lenT, circumference, -lenS, lenO, circumference, -(lenS+lenT),
		)
	}

	return fmt.Sprintf(`<section class="charts-grid" aria-label="Biểu đồ trực quan hệ thống">
            <article class="card chart-card">
                <div class="chart-head">
                    <div class="chart-head__title">
                        <span class="chart-ico chart-ico--brand"><svg class="ico"><use href="#ico-chart"/></svg></span>
                        <div>
                            <h3>Lưu Lượng Dán Link Theo Sàn TMĐT</h3>
                            <p>Tỷ trọng chuyển đổi thực tế trên toàn hệ thống</p>
                        </div>
                    </div>
                    <span class="badge badge--muted font-mono">Tổng: %d lượt</span>
                </div>
                <div class="chart-bars">%s</div>
            </article>

            <article class="card chart-card">
                <div class="chart-head">
                    <div class="chart-head__title">
                        <span class="chart-ico chart-ico--purple"><svg class="ico"><use href="#ico-pie"/></svg></span>
                        <div>
                            <h3>Cơ Cấu Kho Mã &amp; Sức Mạnh Săn Sale</h3>
                            <p>Phân bổ kho ACCESSTRADE &amp; mã độc quyền FB/IG</p>
                        </div>
                    </div>
                    <span class="badge badge--orange">Giảm ≥20%%: %d mã (%d%%)</span>
                </div>
                <div class="donut-layout">
                    <div class="donut-wrap">
                        <svg class="chart-donut" viewBox="0 0 120 120" width="120" height="120" role="img" aria-label="Biểu đồ tròn phân bổ kho mã">
                            <circle cx="60" cy="60" r="44" fill="none" stroke="var(--bg-inset)" stroke-width="12"/>
                            <g transform="rotate(-90 60 60)">%s</g>
                        </svg>
                        <div class="donut-center">
                            <strong>%d</strong>
                            <span>Tổng mã</span>
                        </div>
                    </div>
                    <div class="donut-legend">
                        <div class="legend-item">
                            <span class="legend-dot" style="background:#EA580C;"></span>
                            <span class="legend-name">Shopee Thị Trường</span>
                            <strong>%d</strong><span class="legend-pct">(%d%%)</span>
                        </div>
                        <div class="legend-item">
                            <span class="legend-dot" style="background:#0284C7;"></span>
                            <span class="legend-name">Tiki Creator / Mall</span>
                            <strong>%d</strong><span class="legend-pct">(%d%%)</span>
                        </div>
                        <div class="legend-item">
                            <span class="legend-dot" style="background:#7C3AED;"></span>
                            <span class="legend-name">Độc Quyền &amp; Sàn Khác</span>
                            <strong>%d</strong><span class="legend-pct">(%d%%)</span>
                        </div>
                    </div>
                </div>
            </article>
        </section>`,
		sumClicks, barsHTML.String(),
		highPctVouchers, deepPct,
		donutSegments, poolTotal,
		shopeeVouchers, pctS,
		tikiVouchers, pctT,
		otherV, pctO,
	)
}

type shopeeCardView struct {
	AffID               string
	AppID               string
	AppSecret           string
	SocialParams        string
	ShortlinkBaseURL    string
	ShortlinkMobileOnly bool
	PublicHomeCloak     bool
	OriginBaseURL       string
	BotUsername         string
	HasCFTunnelToken    bool
}

func renderShopeeCard(view shopeeCardView) string {
	var statusBadge string
	if strings.TrimSpace(view.AppID) != "" && strings.TrimSpace(view.AppSecret) != "" {
		statusBadge = fmt.Sprintf(`<span class="badge badge--ok">🟢 Đang bật Open API (Secret: %s)</span>`, utils.EscapeHTML(utils.MaskToken(view.AppSecret)))
	} else if shopee.IsValidShopeeAffiliateID(view.AffID) {
		statusBadge = fmt.Sprintf(`<span class="badge badge--ok">🟢 Đang bật Direct Affiliate ID: %s</span>`, utils.EscapeHTML(strings.TrimSpace(view.AffID)))
	} else {
		statusBadge = `<span class="badge badge--muted">⚪ Chưa cấu hình — đang dùng luồng ACCESSTRADE</span>`
	}

	tunnelBadgeHTML := `<span class="badge badge--info">☁️ Cloud / Edge Origin Ready</span>`
	tunnelTokenBadge := "Chưa gắn — dùng nếu chạy Worker Tunnel"
	tunnelTokenPlaceholder := "Dán Cloudflare Tunnel Token (eyJ... hoặc cả lệnh service install)..."
	if view.HasCFTunnelToken {
		tunnelBadgeHTML = `<span class="badge badge--ok">🌐 Đã gắn Tunnel Token</span>`
		tunnelTokenBadge = "🟢 Đã lưu Token cố định"
		tunnelTokenPlaceholder = "Để trống để giữ Token hiện tại (hoặc dán Token mới để đổi)..."
	}

	secretPlaceholder := "Nhập Secret Key nếu đã được Shopee cấp Open API..."
	if strings.TrimSpace(view.AppSecret) != "" {
		secretPlaceholder = "Để trống để giữ Secret Key hiện tại (đã lưu an toàn)..."
	}
	socialPlaceholder := "Dán link/query phiên IG/FB Reels (có sm_sig_id / encrypted_payload) nếu có..."
	socialBadge := "Chưa gắn mẫu phiên IG/FB"
	if strings.TrimSpace(view.SocialParams) != "" {
		socialPlaceholder = "Đã lưu mẫu phiên IG/FB Reels (để trống để giữ nguyên mẫu hiện tại)..."
		socialBadge = "🟢 Đã gắn bộ tham số IG/FB Reels"
	}

	mobileChecked := ""
	if view.ShortlinkMobileOnly {
		mobileChecked = "checked"
	}
	homeCloakChecked := ""
	if view.PublicHomeCloak {
		homeCloakChecked = "checked"
	}

	workerCode := strings.ReplaceAll(
		salehunterassets.CloudflareWorkerTemplate,
		"https://origin.thegioisieusale.click",
		strings.TrimRight(view.OriginBaseURL, "/"),
	)
	botClean := strings.TrimPrefix(strings.TrimSpace(view.BotUsername), "@")
	if botClean != "" {
		workerCode = strings.ReplaceAll(
			workerCode,
			`const BOT_USERNAME = "vvipass_vip_bot";`,
			fmt.Sprintf(`const BOT_USERNAME = "%s";`, botClean),
		)
	}

	return fmt.Sprintf(`<section class="card card--orange">
            <div class="card-head">
                <div>
                    <h2 class="card-title"><span class="plat-dot" style="background:#EA580C;width:12px;height:12px;" aria-hidden="true"></span><span>Kết Nối Shopee Affiliate Chính Chủ &amp; Bọc Link Riêng</span></h2>
                    <p class="card-desc">
                        Ưu tiên link Shopee chính chủ qua Open API hoặc <code>s.shopee.vn/an_redir</code> và bọc link ngắn an toàn qua tên miền riêng (Cloudflare Worker / Hosting).
                    </p>
                </div>
                <div class="card-head-actions">%s%s</div>
            </div>
            <form method="POST" action="/admin/settings/shopee" class="form-stack">
                <div class="grid-3">
                    <div>
                        <label class="field-label">Shopee Affiliate ID (Dùng ngay qua an_redir)</label>
                        <input type="text" name="shopee_affiliate_id" value="%s" placeholder="Ví dụ: 17375520550 (5-20 chữ số)" class="field-input field-input--mono">
                    </div>
                    <div>
                        <label class="field-label">Shopee Open API AppID (Tùy chọn)</label>
                        <input type="text" name="shopee_app_id" value="%s" placeholder="Nhập AppID từ affiliate.shopee.vn..." class="field-input field-input--mono">
                    </div>
                    <div>
                        <label class="field-label">Shopee Open API Secret Key (Tùy chọn)</label>
                        <input type="password" name="shopee_app_secret" placeholder="%s" class="field-input field-input--mono">
                    </div>
                </div>
                <div class="grid-2 divider-top">
                    <div class="form-stack">
                        <div>
                            <label class="field-label">Bộ Tham Số Phiên IG/FB Reels Passthrough (<span class="label-accent">%s</span>)</label>
                            <input type="password" name="shopee_social_params" placeholder="%s" class="field-input field-input--mono">
                            <label class="check-row check-row--mt">
                                <input type="checkbox" name="clear_social_params" value="1">
                                <span>Xóa bộ tham số phiên IG/FB đang lưu</span>
                            </label>
                        </div>
                    </div>
                    <div class="form-stack">
                        <div>
                            <label class="field-label">Tên Miền Bọc Link Riêng Đang Dùng</label>
                            <input type="text" name="shortlink_base_url" value="%s" placeholder="Mặc định: https://thegioisieusale.click hoặc PUBLIC_BASE_URL (nhập off để tắt)" class="field-input field-input--mono">
                        </div>
                        <div>
                            <label class="field-label">Cloudflare Named Tunnel Token (<span class="label-info">%s</span>)</label>
                            <input type="password" name="cloudflare_tunnel_token" placeholder="%s" class="field-input field-input--mono">
                            <label class="check-row" style="margin-top:0.45rem;">
                                <input type="checkbox" name="clear_tunnel_token" value="1">
                                <span>Xóa Tunnel Token đã lưu</span>
                            </label>
                        </div>
                        <label class="check-row">
                            <input type="checkbox" name="shortlink_mobile_only" value="1" %s>
                            <span>🛡️ Chặn máy tính soi link bọc (Chỉ cho điện thoại mở giống godeal.online)</span>
                        </label>
                        <label class="check-row">
                            <input type="checkbox" name="public_home_cloak" value="1" %s>
                            <span>🙈 Ẩn luôn Trang chủ <code>/</code> thành <code>404 Not Found</code> (Chỉ bật nếu không chờ duyệt web Shopee/AT)</span>
                        </label>
                    </div>
                </div>
                <div>
                    <button type="submit" class="btn btn--primary">
                        💾 Lưu Cấu Hình Shopee &amp; Bọc Link
                    </button>
                </div>
            </form>
            <details class="cf-details">
                <summary class="cf-summary">
                    ☁️ Hướng dẫn gắn tên miền cố định (thegioisieusale.click) qua Cloudflare Worker
                </summary>
                <div class="cf-body">
                    <ol class="cf-steps">
                        <li>Trỏ Cloudflare Worker của <code>thegioisieusale.click</code> về Origin Hosting hiện tại (<code>%s</code>) hoặc Tunnel origin.</li>
                        <li>Vào <b>dash.cloudflare.com</b> → <b>Workers &amp; Pages</b> → chọn Worker của <code>thegioisieusale.click</code> → <b>Edit code</b>, dán toàn bộ code bên dưới vào rồi bấm <b>Deploy</b>.</li>
                        <li>Mọi đường dẫn bot gửi sẽ cố định dạng <code>https://thegioisieusale.click/8_ky_tu</code>; trang chủ <code>/</code> phục vụ Trang Đích 3D tại Cloudflare Edge và mọi đường dẫn nội bộ (<code>/admin</code>, <code>/convert</code>, <code>/health</code>...) đều trả <code>404 Not Found</code> khi truy cập từ Internet!</li>
                    </ol>
                    <textarea id="cf-worker-code" readonly rows="10" class="field-input field-input--mono code-textarea">%s</textarea>
                    <div>
                        <button type="button" class="btn btn--secondary js-copy-link" data-copy-target="cf-worker-code">
                            📋 Sao chép Code Cloudflare Worker
                        </button>
                    </div>
                </div>
            </details>
        </section>`,
		statusBadge,
		tunnelBadgeHTML,
		utils.EscapeHTML(view.AffID),
		utils.EscapeHTML(view.AppID),
		secretPlaceholder,
		socialBadge,
		socialPlaceholder,
		utils.EscapeHTML(view.ShortlinkBaseURL),
		tunnelTokenBadge,
		tunnelTokenPlaceholder,
		mobileChecked,
		homeCloakChecked,
		utils.EscapeHTML(view.OriginBaseURL),
		utils.EscapeHTML(workerCode),
	)
}

func renderOrderAlertsCard(adminTelegramID string, orders []db.TrackedOrder) string {
	statusBadge := `<span class="badge badge--warn">🟡 Chưa gắn Telegram ID nhận báo đơn</span>`
	if strings.TrimSpace(adminTelegramID) != "" {
		statusBadge = fmt.Sprintf(`<span class="badge badge--ok">🟢 Đang báo đơn tới ID: %s</span>`, utils.EscapeHTML(strings.TrimSpace(adminTelegramID)))
	}

	var orderRows strings.Builder
	for _, ord := range orders {
		statusCls := "badge badge--warn"
		switch ord.Status {
		case "APPROVED":
			statusCls = "badge badge--ok"
		case "REJECTED":
			statusCls = "badge badge--err"
		}
		orderRows.WriteString(fmt.Sprintf(`<tr>
                <td class="cell-strong">%s</td>
                <td class="font-mono cell-muted">%s</td>
                <td><span class="%s">%s</span></td>
                <td class="cell-strong">%s</td>
                <td class="font-mono comm-val">%s</td>
                <td class="font-mono cell-muted">%s</td>
            </tr>`,
			utils.EscapeHTML(ord.Platform),
			utils.EscapeHTML(ord.OrderID),
			statusCls,
			utils.EscapeHTML(ord.Status),
			utils.EscapeHTML(utils.FormatVND(ord.OrderAmount)),
			utils.EscapeHTML(utils.FormatVND(ord.Commission)),
			utils.EscapeHTML(formatTimestampVN(ord.OrderTime)),
		))
	}
	orderRowsStr := orderRows.String()
	if orderRowsStr == "" {
		orderRowsStr = `<tr><td colspan="6" class="empty-cell">Chưa có đơn hàng phát sinh được ghi nhận trong 7 ngày gần nhất. Khi có đơn thật từ Shopee / TikTok Shop / ACCESSTRADE, hệ thống sẽ tự lưu và báo ngay về Telegram!</td></tr>`
	}

	return fmt.Sprintf(`<section class="card card--green">
            <div class="card-head">
                <div>
                    <h2 class="card-title"><svg class="ico" aria-hidden="true"><use href="#ico-orders"/></svg><span>Báo Đơn Tự Động Về Telegram &amp; Đối Soát Hoa Hồng (Mỗi 10 Phút)</span></h2>
                    <p class="card-desc">
                        Gõ lệnh <code>/stats</code> trong bot Telegram để lấy ID số của bạn rồi dán vào ô bên dưới. Mỗi khi có đơn mới trên ACCESSTRADE / Shopee Open API, bot sẽ nhắn tin báo ngay!
                    </p>
                </div>
                <div>%s</div>
            </div>
            <div class="filter-bar">
                <form method="POST" action="/admin/settings/notifications" class="inline-form-grow">
                    <div class="grow-field">
                        <label class="field-label">Telegram ID Admin Nhận Thông Báo Đơn</label>
                        <input type="text" name="admin_telegram_id" value="%s" placeholder="Ví dụ: 5123456789 (hoặc để trống để tắt)" class="field-input font-mono">
                    </div>
                    <button type="submit" class="btn btn--success">
                        💾 Lưu Telegram ID Báo Đơn
                    </button>
                </form>
                <form method="POST" action="/admin/orders/sync">
                    <button type="submit" class="btn btn--secondary">
                        🔄 Quét Đơn Mới Ngay
                    </button>
                </form>
            </div>
            <div class="table-wrap">
                <table class="data-table">
                    <thead>
                        <tr>
                            <th>Sàn</th>
                            <th>Mã Đơn Hàng</th>
                            <th>Trạng Thái</th>
                            <th>Giá Trị Đơn</th>
                            <th>Hoa Hồng</th>
                            <th>Thời Gian Đặt</th>
                        </tr>
                    </thead>
                    <tbody>
                        %s
                    </tbody>
                </table>
            </div>
        </section>`, statusBadge, utils.EscapeHTML(strings.TrimSpace(adminTelegramID)), orderRowsStr)
}

func injectCSRFIntoPostForms(html, csrfToken string) string {
	csrfInput := fmt.Sprintf(`<input type="hidden" name="_csrf" value="%s">`, utils.EscapeHTML(csrfToken))
	var out strings.Builder
	out.Grow(len(html) + 512)
	rest := html
	for {
		formStart := strings.Index(rest, "<form")
		if formStart < 0 {
			break
		}
		tagEndRel := strings.IndexByte(rest[formStart:], '>')
		if tagEndRel < 0 {
			break
		}
		tagEnd := formStart + tagEndRel + 1
		out.WriteString(rest[:tagEnd])
		tagLower := strings.ToLower(rest[formStart:tagEnd])
		if strings.Contains(tagLower, `method="post"`) || strings.Contains(tagLower, `method='post'`) {
			out.WriteString(csrfInput)
		}
		rest = rest[tagEnd:]
	}
	out.WriteString(rest)
	return out.String()
}

type dashboardRenderData struct {
	BotUsername          string
	BotModeBadge         string
	AdminUsername        string
	AuthActionHTML       string
	NoticeBanner         string
	PersistenceBanner    string
	Stats                db.AdminStats
	TotalMarketVouchers  int
	HighPctVouchers      int
	OtherVouchersCount   int
	ShopeeVouchers       int
	TikiVouchers         int
	SavedDealsCount      int
	LastSyncDisplay      string
	ChartsHTML           string
	SocialHarvesterBadge string
	SocialAutoPanelHTML  string
	ClicksRows           string
	ATStatusBadge        string
	CampaignStatusHTML   string
	CampShopee           string
	CampLazada           string
	CampTiki             string
	CampTikTok           string
	ShopeeCardHTML       string
	AffiliateTemplate    string
	SearchQ              string
	SelAll               string
	SelPct20             string
	SelOther             string
	SelShopee            string
	SelTiki              string
	ShownVouchersCount   int
	MatchedVouchersCount int
	VoucherRows          string
	ExclusiveRows        string
	OrderAlertsCardHTML  string
	CSRFToken            string
}

func renderDashboardDocument(d dashboardRenderData) string {
	body := fmt.Sprintf(`<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Thế Giới Siêu Sale - Trung Tâm Quản Trị Hệ Thống</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Be+Vietnam+Pro:wght@400;500;600;700;800&family=JetBrains+Mono:wght@400;500;600&display=swap" rel="stylesheet">
    <meta name="csrf-token" content="%s">
    <script src="/assets/theme.js"></script>
    %s
</head>
<body>
    %s
    <div class="app-shell">
        <aside class="sidebar">
            <div class="brand">
                <div class="brand__logo-wrap">
                    <img src="/assets/bot-logo.webp" alt="Logo Thế Giới Siêu Sale" class="brand__logo">
                    <span class="brand__dot" aria-hidden="true"></span>
                </div>
                <div>
                    <div class="brand__title-row">
                        <h1 class="brand__title">THẾ GIỚI SIÊU SALE</h1>
                        <span class="brand__tag">Console</span>
                    </div>
                    <p class="brand__sub">Trung Tâm Điều Hành &amp; Săn Sale Đa Sàn</p>
                </div>
            </div>

            <div>
                <div class="sidebar__section-label">Điều Hướng Hệ Thống</div>
                <nav class="tab-nav" aria-label="Danh mục quản trị">
                    <button type="button" data-tab="overview" id="tab-btn-overview" class="tab-btn active" aria-current="page">
                        <span class="tab-btn__icon" aria-hidden="true"><svg class="ico"><use href="#ico-overview"/></svg></span>
                        <span>Tổng Quan</span>
                    </button>
                    <button type="button" data-tab="platforms" id="tab-btn-platforms" class="tab-btn">
                        <span class="tab-btn__icon" aria-hidden="true"><svg class="ico"><use href="#ico-platforms"/></svg></span>
                        <span>Kết Nối Sàn &amp; API</span>
                    </button>
                    <button type="button" data-tab="vouchers" id="tab-btn-vouchers" class="tab-btn">
                        <span class="tab-btn__icon" aria-hidden="true"><svg class="ico"><use href="#ico-vouchers"/></svg></span>
                        <span>Kho Voucher Thị Trường</span>
                    </button>
                    <button type="button" data-tab="orders" id="tab-btn-orders" class="tab-btn">
                        <span class="tab-btn__icon" aria-hidden="true"><svg class="ico"><use href="#ico-orders"/></svg></span>
                        <span>Báo Đơn &amp; Doanh Thu</span>
                    </button>
                </nav>
            </div>

            <div class="sidebar__footer">
                <div class="bot-status-card">
                    <div class="bot-status-card__row">
                        <span class="cell-muted">Bot Telegram</span>
                        <span style="display:inline-flex;align-items:center;gap:0.35rem;">
                            <span class="pulse-dot" aria-hidden="true"></span>
                            <a href="https://t.me/%s" target="_blank" rel="noopener noreferrer" class="bot-link">@%s</a>
                        </span>
                    </div>
                    <div class="bot-status-card__row">
                        <span class="cell-muted">Chế độ chạy</span>
                        %s
                    </div>
                </div>
                <a href="/" target="_blank" rel="noopener noreferrer" class="btn btn--secondary btn--sm btn--block">
                    <svg class="ico ico--sm" aria-hidden="true"><use href="#ico-globe"/></svg>
                    <span>Xem Trang Đích Bot 3D ↗</span>
                </a>
            </div>
        </aside>

        <div class="shell-main">
            <header class="topbar">
                <div class="topbar__left">
                    <h2 id="top-page-title" class="topbar__title">Tổng Quan Hệ Thống</h2>
                    <p id="top-page-subtitle" class="topbar__sub">Theo dõi lưu lượng Bot Telegram, Kho Mã Thị Trường &amp; Social Harvester thời gian thực</p>
                </div>
                <div class="topbar__actions">
                    <div class="admin-pill hide-sm">
                        <span>Admin: <strong>%s</strong></span>
                    </div>
                    %s
                    <button type="button" id="theme-toggle-btn" class="theme-btn" aria-label="Chuyển đổi giao diện Sáng/Tối">
                        <svg class="ico ico--xs" aria-hidden="true"><use id="theme-ico-use" href="#ico-moon"/></svg>
                        <span id="theme-btn-text">Tối</span>
                    </button>
                </div>
            </header>

            <main class="admin-main">
                %s
                %s

                <div id="tab-pane-overview" class="tab-pane">
                    <section class="kpi-grid" aria-label="Chỉ số tổng quan">
                        <div class="kpi-card">
                            <div>
                                <div class="kpi-card__top">
                                    <span>Người Dùng Telegram</span>
                                    <span class="kpi-card__icon kpi-card__icon--blue" aria-hidden="true"><svg class="ico ico--sm"><use href="#ico-users"/></svg></span>
                                </div>
                                <div class="kpi-card__val">%d</div>
                            </div>
                            <div class="kpi-card__pill">Khách thực tế đã tương tác bot</div>
                        </div>

                        <div class="kpi-card">
                            <div>
                                <div class="kpi-card__top">
                                    <span>Tổng Lượt Dán Link</span>
                                    <span class="kpi-card__icon kpi-card__icon--amber" aria-hidden="true"><svg class="ico ico--sm"><use href="#ico-clicks"/></svg></span>
                                </div>
                                <div class="kpi-card__val kpi-card__val--amber">%d</div>
                            </div>
                            <div class="kpi-card__meta">
                                <div class="kpi-row"><span>Shopee: <strong>%d</strong></span><span>Lazada: <strong>%d</strong></span></div>
                                <div class="kpi-row"><span>TikTok: <strong>%d</strong></span><span>Tiki: <strong>%d</strong></span></div>
                            </div>
                        </div>

                        <div class="kpi-card kpi-card--orange">
                            <div>
                                <div class="kpi-card__top">
                                    <span>Kho Mã Thị Trường</span>
                                    <span class="kpi-card__icon kpi-card__icon--orange" aria-hidden="true"><svg class="ico ico--sm"><use href="#ico-flame"/></svg></span>
                                </div>
                                <div class="kpi-card__val kpi-card__val--orange">%d</div>
                            </div>
                            <div class="kpi-card__meta">
                                <div class="kpi-row"><span>Giảm ≥20%%: <strong>%d</strong></span><span>Tiền/Khác: <strong>%d</strong></span></div>
                                <div class="kpi-row"><span>Shopee: <strong>%d</strong></span><span>Tiki: <strong>%d</strong></span><span>Độc quyền: <strong>%d</strong></span></div>
                            </div>
                        </div>

                        <div class="kpi-card kpi-card--green">
                            <div>
                                <div class="kpi-card__top">
                                    <span>Tự Động Quét Nền</span>
                                    <span class="kpi-card__icon kpi-card__icon--green" aria-hidden="true"><svg class="ico ico--sm"><use href="#ico-clock"/></svg></span>
                                </div>
                                <div class="kpi-card__val--green">Chu kỳ 60 giây / lần</div>
                                <div class="muted-text">Lần cuối: <strong class="meta-time">%s</strong></div>
                            </div>
                            <div class="refresh-chip">
                                Tự làm mới Dashboard mỗi 30 giây
                            </div>
                        </div>
                    </section>

                    %s

                    <section class="card card--social">
                        <div class="social-left">
                            <div class="social-icon" aria-hidden="true"><svg class="ico" style="width:24px;height:24px;"><use href="#ico-bot"/></svg></div>
                            <div>
                                <div class="card-title">
                                    <span>Shopee Social Harvester — Tự Động Nhặt Mã &amp; Chữ Ký FB / IG Reels</span>
                                    %s
                                </div>
                                <p class="card-desc">
                                    Hệ thống chạy tự động hoàn toàn ngay khi mở app &amp; tự quét lại mỗi 5 phút từ Telegram/Web deal công khai để nhặt <b>Mã Instagram / Facebook KOL</b> và chữ ký số (<code>channel_type=fb/ig</code>, tự nâng cấp <code>sm_sig_id</code> nếu phát hiện) — không cần thao tác tay!
                                </p>
                            </div>
                        </div>
                        %s
                    </section>

                    <section class="card card--table">
                        <div class="card-head card-head--table">
                            <div>
                                <h2 class="card-title"><svg class="ico" aria-hidden="true"><use href="#ico-history"/></svg><span>Lịch Sử Khách Dán Link Săn Sale Gần Đây (50 lượt mới nhất)</span></h2>
                                <p class="card-desc">Theo dõi thời gian thực các liên kết khách gửi vào bot Telegram</p>
                            </div>
                        </div>
                        <div class="table-wrap">
                            <table class="data-table">
                                <thead>
                                    <tr>
                                        <th>Telegram ID</th>
                                        <th>Sàn</th>
                                        <th>Link Khách Gửi</th>
                                        <th>Link Đầu Ra</th>
                                        <th>Thời Gian</th>
                                    </tr>
                                </thead>
                                <tbody id="clicks-tbody">
                                    %s
                                </tbody>
                            </table>
                        </div>
                    </section>
                </div>

                <div id="tab-pane-platforms" class="tab-pane is-hidden">
                    <section class="card">
                        <div class="card-head">
                            <div>
                                <h2 class="card-title"><svg class="ico" aria-hidden="true"><use href="#ico-rocket"/></svg><span>Kết Nối ACCESSTRADE API (Tự Động Đa Sàn)</span></h2>
                                <p class="card-desc">
                                    Bot tự động gọi API <code>v1/product_link/create</code> &amp; TikTok v2 Universal Deeplink, đồng thời kiểm tra HTTP 302/200 chống lỗi link 404/500.
                                </p>
                            </div>
                            <div class="card-head-actions">
                                %s
                                <form method="POST" action="/admin/settings/accesstrade/sync" class="inline-form">
                                    <button type="submit" class="btn btn--secondary btn--sm">🔄 Đồng Bộ Lại Chiến Dịch</button>
                                </form>
                            </div>
                        </div>

                        <div class="split-at">
                            <div class="plat-panel">
                                <div class="plat-panel__title">Trạng Thái Duyệt &amp; Chuẩn Hóa Link</div>
                                <div>%s</div>
                                <div class="inset-note">
                                    <div><b>🛡️ Cơ chế bảo vệ Link Đầu Ra 100%% Sống:</b></div>
                                    <div>• <b>TikTok Shop:</b> Link v2 <code>tiktok.com/t/...</code> mở thẳng sản phẩm trên App.</div>
                                    <div>• <b>Tiki &amp; Alibaba:</b> Tự dùng gói <b>TIKI CREATOR</b> &amp; <b>ALIBABA</b> đã duyệt + kiểm tra gateway <code>HTTP 200</code> trước khi gửi.</div>
                                    <div>• <b>Shopee / Lazada:</b> Hỗ trợ song song Shopee Chính Chủ &amp; Deeplink dự phòng.</div>
                                </div>
                            </div>

                            <form method="POST" action="/admin/settings/accesstrade" class="form-stack">
                                <div>
                                    <label class="field-label" for="at-api-token">ACCESSTRADE API Access Key (Token)</label>
                                    <input id="at-api-token" type="password" name="api_token" placeholder="Dán API Access Key mới (hoặc để trống để giữ Token hiện tại)..." class="field-input field-input--mono">
                                    <p class="muted-text">💡 Đã tích hợp sẵn API Token từ cấu hình hệ thống — tự động giữ kết nối khi khởi động lại.</p>
                                </div>

                                <div class="form-grid-2">
                                    <div>
                                        <label class="field-label" for="camp-shopee">Campaign ID Shopee (Tùy chọn)</label>
                                        <input id="camp-shopee" type="text" name="campaign_shopee" value="%s" placeholder="Tự động nhận diện nếu để trống" class="field-input field-input--mono">
                                    </div>
                                    <div>
                                        <label class="field-label" for="camp-lazada">Campaign ID Lazada (Tùy chọn)</label>
                                        <input id="camp-lazada" type="text" name="campaign_lazada" value="%s" placeholder="Tự động nhận diện nếu để trống" class="field-input field-input--mono">
                                    </div>
                                    <div>
                                        <label class="field-label" for="camp-tiki">Campaign ID Tiki (Tùy chọn)</label>
                                        <input id="camp-tiki" type="text" name="campaign_tiki" value="%s" placeholder="Tự động nhận diện nếu để trống" class="field-input field-input--mono">
                                    </div>
                                    <div>
                                        <label class="field-label" for="camp-tiktok">Campaign ID TikTok Shop (Tùy chọn)</label>
                                        <input id="camp-tiktok" type="text" name="campaign_tiktok" value="%s" placeholder="Tự động nhận diện nếu để trống" class="field-input field-input--mono">
                                    </div>
                                </div>

                                <div>
                                    <button type="submit" class="btn btn--primary">Lưu &amp; Kích Hoạt ACCESSTRADE API</button>
                                </div>
                            </form>
                        </div>
                    </section>

                    %s

                    <section class="card">
                        <div>
                            <h2 class="card-title"><svg class="ico" aria-hidden="true"><use href="#ico-platforms"/></svg><span>Cấu Hình Deeplink Dự Phòng (Nếu Chưa Dùng API Token)</span></h2>
                            <p class="card-desc">
                                Nếu chiến dịch trên API chưa duyệt, hệ thống sẽ kiểm tra mẫu link dự phòng này. Nếu để trống, bot tự động trả về link sản phẩm sạch để khách mở App mua hàng không bị lỗi 404.
                            </p>
                        </div>
                        <form method="POST" action="/admin/settings/affiliate" class="form-stack">
                            <div>
                                <label class="field-label" for="aff-template">Mẫu Deeplink Template</label>
                                <input id="aff-template" type="text" name="template" value="%s" placeholder="Ví dụ: https://go.isclix.com/deep_link/PUB_ID/CAMPAIGN_ID?url={{CLEAN_URL}}&amp;utm_source=facebook&amp;sub1={{USER_ID}}" class="field-input field-input--mono">
                            </div>
                            <div>
                                <button type="submit" class="btn btn--secondary">Lưu Deeplink Dự Phòng</button>
                            </div>
                        </form>
                    </section>
                </div>

                <div id="tab-pane-vouchers" class="tab-pane is-hidden">
                    <section class="card">
                        <div class="card-head">
                            <div>
                                <h2 class="card-title">
                                    <svg class="ico" aria-hidden="true"><use href="#ico-flame"/></svg>
                                    <span>Kho Mã Giảm Giá Toàn Thị Trường (Cập Nhật Mỗi 60 Giây)</span>
                                    <span class="badge badge--orange">%d mã đang sẵn (%d mã &gt;=20%%)</span>
                                </h2>
                                <p class="card-desc">
                                    Quét từ hệ thống ACCESSTRADE (mã %%, giảm tiền K/VNĐ, mã hot, coupon Tiki/Shopee/Lazada/TikTok). Lần quét gần nhất: <strong class="ok-strong">%s</strong>.
                                </p>
                            </div>
                            <form method="POST" action="/admin/vouchers/sync" class="inline-form">
                                <button type="submit" class="btn btn--orange-soft">Quét Sâu Toàn Bộ Ngay</button>
                            </form>
                        </div>

                        <form method="GET" action="/admin" class="filter-bar">
                            <input type="text" name="q" value="%s" aria-label="Tìm kiếm mã giảm giá" placeholder="Tìm tên shop, mã code, mức giảm (VD: 35%%, LANGXDKKP, Tiki)..." class="field-input grow-field">
                            <select name="plat" aria-label="Lọc theo sàn hoặc mức giảm" class="field-input">
                                <option value="all" %s>🌟 Tất Cả Sàn &amp; Loại</option>
                                <option value="pct20" %s>🔥 Giảm Sâu 20%% - 35%%+</option>
                                <option value="other" %s>💸 Giảm Tiền K/VNĐ &amp; Khác</option>
                                <option value="Shopee" %s>🟠 Chỉ Shopee</option>
                                <option value="Tiki" %s>🔹 Chỉ Tiki</option>
                            </select>
                            <button type="submit" class="btn btn--secondary btn--sm">Lọc Mã</button>
                            <span class="cell-muted">Hiển thị <strong class="primary-strong">%d</strong> / <strong>%d</strong> mã khớp</span>
                        </form>

                        <div class="table-wrap">
                            <table class="data-table">
                                <thead>
                                    <tr>
                                        <th>Mức Giảm</th>
                                        <th>Sàn</th>
                                        <th>Mã Code</th>
                                        <th>Gian Hàng &amp; Điều Kiện Áp Mã</th>
                                        <th>HSD</th>
                                        <th>Link Áp Mã</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    %s
                                </tbody>
                            </table>
                        </div>
                    </section>

                    <section class="card card--purple">
                        <div class="card-head">
                            <div>
                                <h2 class="card-title">
                                    <svg class="ico" aria-hidden="true"><use href="#ico-crown"/></svg>
                                    <span>Kho Mã Độc Quyền Hợp Tác (Cấp Riêng AM / MCN / Nhãn Hàng)</span>
                                    <span class="badge badge--purple">%d mã đã lưu</span>
                                </h2>
                                <p class="card-desc">
                                    Dán hàng loạt mã độc quyền nhận được từ Account Manager ACCESSTRADE (MCN / Super Hoa Hồng) hoặc hợp tác trực tiếp với Shop. Các mã này luôn được <b>ưu tiên hàng đầu</b> trên Telegram Bot!
                                </p>
                            </div>
                        </div>

                        <div class="grid-2">
                            <form method="POST" action="/admin/deals/import" class="subpanel">
                                <div class="subpanel__title subpanel__title--purple">⚡ Nhập Nhanh Hàng Loạt (Hỗ trợ dấu | hoặc copy thẳng từ Excel/Google Sheets)</div>
                                <p class="card-desc" style="margin: 0;">
                                    Mỗi dòng 1 mã theo thứ tự: <code>Sàn | Tên/Shop | Mã Code | Mức giảm | Link áp mã | Đơn tối thiểu</code>
                                </p>
                                <textarea name="bulk_lines" rows="5" required aria-label="Danh sách mã độc quyền nhập hàng loạt" placeholder="Shopee | Deal Độc Quyền MCN 30%% | ATVIP30 | Giảm 30%% tối đa 100k | https://shopee.vn/search?... | 200000" class="code-textarea"></textarea>
                                <div>
                                    <button type="submit" class="btn btn--purple">📥 Nhập Danh Sách Mã Độc Quyền</button>
                                </div>
                            </form>

                            <form method="POST" action="/admin/deals/add" class="subpanel">
                                <div class="subpanel__title">➕ Thêm Lẻ 1 Mã Độc Quyền</div>
                                <div class="form-grid-2">
                                    <select name="platform" aria-label="Chọn sàn" class="field-input">
                                        <option value="Shopee">Shopee</option>
                                        <option value="TikTok Shop">TikTok Shop</option>
                                        <option value="Lazada">Lazada</option>
                                        <option value="Tiki">Tiki</option>
                                        <option value="Alibaba">Alibaba</option>
                                    </select>
                                    <input type="text" name="coupon_code" required placeholder="Mã Code (VD: ATVIP35)" class="field-input field-input--mono">
                                    <input type="text" name="title" required placeholder="Tên Shop / Chương trình" class="field-input">
                                    <input type="text" name="discount_desc" required placeholder="Mức giảm (VD: Giảm 35%% tối đa 120k)" class="field-input">
                                    <input type="url" name="deal_url" required placeholder="Link áp mã (https://...)" class="field-input field-input--mono">
                                    <input type="number" name="min_spend" min="0" step="1000" value="0" placeholder="Đơn tối thiểu (VNĐ)" class="field-input field-input--mono">
                                </div>
                                <label class="check-row">
                                    <input type="checkbox" name="is_exclusive" value="1" checked>
                                    <span>Đánh dấu là 👑 Mã Độc Quyền Hợp Tác (Ưu tiên hiển thị đầu tiên)</span>
                                </label>
                                <div>
                                    <button type="submit" class="btn btn--secondary">💾 Lưu Mã Độc Quyền</button>
                                </div>
                            </form>
                        </div>

                        <div class="table-wrap">
                            <table class="data-table">
                                <thead>
                                    <tr>
                                        <th>Loại</th>
                                        <th>Sàn</th>
                                        <th>Tên &amp; Mô Tả</th>
                                        <th>Mã Code</th>
                                        <th>Trạng Thái</th>
                                        <th>Hành Động</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    %s
                                </tbody>
                            </table>
                        </div>
                    </section>
                </div>

                <div id="tab-pane-orders" class="tab-pane is-hidden">
                    %s
                </div>
            </main>

            <footer class="admin-footer">
                Thế Giới Siêu Sale Affiliate System · Bản Quyền 2026 · Vận hành bảo mật trên Go + LibSQL (Turso)
            </footer>
        </div>
    </div>

    <script src="/assets/admin.js" defer></script>
    <script src="/assets/home.js" defer></script>
</body>
</html>`,
		utils.EscapeHTML(d.CSRFToken),
		adminCSS,
		adminSVGSprite,
		utils.EscapeHTML(d.BotUsername),
		utils.EscapeHTML(d.BotUsername),
		d.BotModeBadge,
		utils.EscapeHTML(d.AdminUsername),
		d.AuthActionHTML,
		d.NoticeBanner,
		d.PersistenceBanner,
		d.Stats.TotalUsers,
		d.Stats.TotalClicks,
		d.Stats.ShopeeClicks,
		d.Stats.LazadaClicks,
		d.Stats.TikTokClicks,
		d.Stats.TikiClicks,
		d.TotalMarketVouchers,
		d.HighPctVouchers,
		d.OtherVouchersCount,
		d.ShopeeVouchers,
		d.TikiVouchers,
		d.SavedDealsCount,
		utils.EscapeHTML(d.LastSyncDisplay),
		d.ChartsHTML,
		d.SocialHarvesterBadge,
		d.SocialAutoPanelHTML,
		d.ClicksRows,
		d.ATStatusBadge,
		d.CampaignStatusHTML,
		utils.EscapeHTML(d.CampShopee),
		utils.EscapeHTML(d.CampLazada),
		utils.EscapeHTML(d.CampTiki),
		utils.EscapeHTML(d.CampTikTok),
		d.ShopeeCardHTML,
		utils.EscapeHTML(d.AffiliateTemplate),
		d.TotalMarketVouchers,
		d.HighPctVouchers,
		utils.EscapeHTML(d.LastSyncDisplay),
		utils.EscapeHTML(d.SearchQ),
		d.SelAll,
		d.SelPct20,
		d.SelOther,
		d.SelShopee,
		d.SelTiki,
		d.ShownVouchersCount,
		d.MatchedVouchersCount,
		d.VoucherRows,
		d.SavedDealsCount,
		d.ExclusiveRows,
		d.OrderAlertsCardHTML,
	)

	return injectCSRFIntoPostForms(body, d.CSRFToken)
}
