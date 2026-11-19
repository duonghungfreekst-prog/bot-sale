// Cloudflare Worker Proxy che 100% Web Chính cho Shortlink Telegram (giống godeal.online)
// Hướng dẫn nhanh:
//   1. Đăng ký/đăng nhập Cloudflare (miễn phí) -> Workers & Pages -> Create Worker.
//   2. Xóa code mẫu, dán toàn bộ file này vào -> bấm Deploy.
//   3. Copy URL dạng https://ten-worker.ten-ban.workers.dev dán vào ô
//      "Tên Miền Bọc Link Riêng" trong /admin -> Lưu lại.
// Lưu ý: Không gắn dấu / ở cuối ORIGIN_URL.

const ORIGIN_URL = "https://bot-sale.onrender.com";

export default {
  async fetch(request) {
    const url = new URL(request.url);
    const path = url.pathname;

    // Chỉ cho phép GET/HEAD vào mã shortlink 6-12 ký tự (/{code}, /go/{code}) hoặc /assets/bridge.js
    const isReserved = ["/health", "/convert", "/status", "/metrics", "/robots"].includes(path.toLowerCase());
    const isShortCode = !isReserved && /^\/(?:go\/)?[A-Za-z0-9]{6,12}$/.test(path);
    const isBridgeAsset = path === "/assets/bridge.js";
    const isAllowedMethod = request.method === "GET" || request.method === "HEAD";

    // Mọi đường dẫn khác (/, /admin, /admin/login, /convert, /health, /robots.txt...) -> 404 Not Found
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
