package salehunterassets

import _ "embed"

//go:embed assets/bot-logo.webp
var BotLogoWebp []byte

//go:embed assets/bot-cart-3d.webp
var BotCart3DWebp []byte

//go:embed cloudflare_worker.js
var CloudflareWorkerTemplate string
