package db

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"salehunter-go/internal/config"
)

func strPtr(s string) *string { return &s }

func TestCloudModeRejectsLocalSQLiteFallback(t *testing.T) {
	forbiddenPath := filepath.Join(t.TempDir(), "must_not_create_cloud.db")
	cfg := config.Config{
		TelegramToken: "test",
		BotUsername:   "vvipass_vip_bot",
		DatabasePath:  forbiddenPath,
		AdminUsername: "admin",
		AdminPassword: "StrongPassword!123",
		Port:          10000,
		PublicBaseURL: "https://hosting.example",
	}

	appDB, err := Init(cfg)
	if err == nil || appDB != nil {
		t.Fatalf("expected error in cloud mode without Turso")
	}
	if !strings.Contains(err.Error(), "TURSO_DATABASE_URL") {
		t.Fatalf("expected TURSO_DATABASE_URL error, got %v", err)
	}
	if _, statErr := os.Stat(forbiddenPath); !os.IsNotExist(statErr) {
		t.Fatalf("cloud mode must NOT create local SQLite file")
	}

	cfg.PublicBaseURL = ""
	if !cfg.IsCloudRuntime() {
		localDB, localErr := Init(cfg)
		if localErr != nil {
			t.Fatalf("local mode init failed: %v", localErr)
		}
		_ = localDB.Close()
	}
}

func TestRuntimeL1CacheAndShortlinkBatch(t *testing.T) {
	dbPath := filepath.Join(t.TempDir(), "test_l1.db")
	appDB, err := InitLocalForTest(dbPath)
	if err != nil {
		t.Fatalf("init failed: %v", err)
	}
	defer func() { _ = appDB.Close() }()
	appDB.SetCacheEnabledForTest(true)

	if err := SetSetting(appDB.DB, "shortlink_base_url", "https://go.example.com"); err != nil {
		t.Fatal(err)
	}
	if err := SetSetting(appDB.DB, "market_vouchers_cache_json", `[{"big":true}]`); err != nil {
		t.Fatal(err)
	}

	m1, err := appDB.GetSettingsMapCached()
	if err != nil {
		t.Fatal(err)
	}
	if m1["shortlink_base_url"] != "https://go.example.com" {
		t.Fatalf("unexpected cached value: %q", m1["shortlink_base_url"])
	}
	if _, hasBlob := m1["market_vouchers_cache_json"]; hasBlob {
		t.Fatalf("must exclude market_vouchers_cache_json blob from settings cache")
	}

	if err := SetSetting(appDB.DB, "shortlink_base_url", "https://new.example.com"); err != nil {
		t.Fatal(err)
	}
	m2, _ := appDB.GetSettingsMapCached()
	if m2["shortlink_base_url"] != "https://go.example.com" {
		t.Fatalf("expected L1 cache hit before invalidation, got %q", m2["shortlink_base_url"])
	}

	appDB.InvalidateSettingsCache()
	m3, _ := appDB.GetSettingsMapCached()
	if m3["shortlink_base_url"] != "https://new.example.com" {
		t.Fatalf("expected fresh value after invalidation, got %q", m3["shortlink_base_url"])
	}

	wrapped := appDB.WrapURLsBatchCached("https://go.example.com", []WrapLinkItem{
		{TargetURL: "https://shopee.vn/product/1/2", Title: "SP 1", Platform: "Shopee"},
		{TargetURL: "https://shopee.vn/product/1/2", Title: "SP 1", Platform: "Shopee"},
		{TargetURL: "https://tiki.vn/p123.html", Title: "SP 2", Platform: "Tiki"},
	})
	if len(wrapped) != 3 || wrapped[0] != wrapped[1] || !strings.HasPrefix(wrapped[0], "https://go.example.com/") {
		t.Fatalf("unexpected wrapped batch: %#v", wrapped)
	}

	parts := strings.Split(wrapped[0], "/")
	code := parts[len(parts)-1]
	rec, err := appDB.ResolveShortLinkCached(code)
	if err != nil || rec == nil || rec.TargetURL != "https://shopee.vn/product/1/2" {
		t.Fatalf("unexpected cached shortlink resolve: %#v, err=%v", rec, err)
	}
}

func TestExclusiveDealsBatchAndPriority(t *testing.T) {
	appDB, err := InitLocalForTest(":memory:")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = appDB.Close() }()

	items := []NewDealInput{
		{
			Platform:     "Shopee",
			Title:        "Deal Thuong",
			CouponCode:   strPtr("NORM10"),
			DiscountDesc: "Giam 10%",
			MinSpend:     50000,
			DealURL:      "https://shopee.vn",
			IsExclusive:  false,
		},
		{
			Platform:     "Shopee",
			Title:        "Deal Doc Quyen",
			CouponCode:   strPtr("VIP35"),
			DiscountDesc: "Giam 35% doc quyen",
			MinSpend:     100000,
			DealURL:      "https://shopee.vn",
			ExpireAt:     strPtr("2026-12-31"),
			IsExclusive:  true,
		},
	}

	ins1, upd1, err := AddDealsBatch(appDB.DB, items)
	if err != nil || ins1 != 2 || upd1 != 0 {
		t.Fatalf("expected (2,0), got (%d,%d), err=%v", ins1, upd1, err)
	}
	ins2, upd2, err := AddDealsBatch(appDB.DB, items)
	if err != nil || ins2 != 0 || upd2 != 2 {
		t.Fatalf("expected (0,2), got (%d,%d), err=%v", ins2, upd2, err)
	}
	active, err := GetActiveDeals(appDB.DB, "")
	if err != nil || len(active) != 2 || !active[0].IsExclusive || active[0].CouponCode == nil || *active[0].CouponCode != "VIP35" {
		t.Fatalf("unexpected active deals order: %#v, err=%v", active, err)
	}
}

func TestShortLinkWrapAndDeduplicate(t *testing.T) {
	appDB, err := InitLocalForTest(":memory:")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = appDB.Close() }()

	target := "https://s.shopee.vn/an_redir?origin_link=https%3A%2F%2Fshopee.vn%2Fproduct%2F1%2F2&affiliate_id=17375520550"
	w1 := WrapURLIfPublic(appDB.DB, "https://bot-sale.onrender.com/", target, "Áo thun test", "Shopee")
	w2 := WrapURLIfPublic(appDB.DB, "https://bot-sale.onrender.com", target, "Áo thun test", "Shopee")
	if !strings.HasPrefix(w1, "https://bot-sale.onrender.com/") || w1 != w2 {
		t.Fatalf("expected deduplicated wrap: %q vs %q", w1, w2)
	}
	noDouble := WrapURLIfPublic(appDB.DB, "https://bot-sale.onrender.com", w1, "Áo thun test", "Shopee")
	if noDouble != w1 {
		t.Fatalf("unexpected double wrap: %q", noDouble)
	}
	parts := strings.Split(w1, "/")
	code := parts[len(parts)-1]
	if len(code) != 8 || !IsValidShortCode(code) {
		t.Fatalf("invalid generated short code: %q", code)
	}
}

func TestTrackedOrderUpsertIdempotentAndSession(t *testing.T) {
	appDB, err := InitLocalForTest(":memory:")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = appDB.Close() }()

	ord := TrackedOrder{
		OrderKey:    "AT:ORD123",
		Platform:    "Shopee",
		OrderID:     "ORD123",
		Status:      "PENDING",
		OrderAmount: 250000,
		Commission:  15000,
		SubID:       "u1_r0_abcd",
		OrderTime:   "2026-11-26T00:00:00Z",
		NotifiedAt:  time.Now().UTC().Format(time.RFC3339),
	}
	isNew1, err := UpsertTrackedOrder(appDB.DB, ord)
	if err != nil || !isNew1 {
		t.Fatalf("first upsert failed: new=%v err=%v", isNew1, err)
	}
	ord.Status = "APPROVED"
	isNew2, err := UpsertTrackedOrder(appDB.DB, ord)
	if err != nil || isNew2 {
		t.Fatalf("second upsert should return false: new=%v err=%v", isNew2, err)
	}

	rawToken := "secret-session-token-xyz"
	if err := CreateAdminSession(appDB.DB, rawToken, 3600); err != nil {
		t.Fatal(err)
	}
	var tokenHash string
	err = appDB.DB.QueryRow("SELECT token_hash FROM admin_sessions LIMIT 1").Scan(&tokenHash)
	if err != nil || tokenHash == rawToken || len(tokenHash) != 64 {
		t.Fatalf("expected sha256 token hash in DB, got %q, err=%v", tokenHash, err)
	}
	ok, err := VerifyAdminSession(appDB.DB, rawToken)
	if err != nil || !ok {
		t.Fatalf("session verify failed: ok=%v err=%v", ok, err)
	}
	_ = DeleteAdminSession(appDB.DB, rawToken)
	okAfter, _ := VerifyAdminSession(appDB.DB, rawToken)
	if okAfter {
		t.Fatalf("session should be deleted after logout")
	}
}

func TestBufferedShortLinkClickFlush(t *testing.T) {
	appDB, err := InitLocalForTest(":memory:")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = appDB.Close() }()
	appDB.SetCacheEnabledForTest(true)

	wrapped := appDB.WrapURLCached("https://go.example.com", "https://shopee.vn/product/9/10", "SP Click", "Shopee")
	parts := strings.Split(wrapped, "/")
	code := parts[len(parts)-1]

	appDB.RecordShortLinkClickBuffered(code)
	appDB.RecordShortLinkClickBuffered(code)
	appDB.RecordShortLinkClickBuffered(code)

	var beforeFlush int64
	if err := appDB.DB.QueryRow("SELECT clicks_count FROM short_links WHERE code = ?1", code).Scan(&beforeFlush); err != nil {
		t.Fatal(err)
	}
	if beforeFlush != 0 {
		t.Fatalf("expected 0 DB writes before flush, got %d", beforeFlush)
	}

	flushed := appDB.FlushShortLinkClicks()
	if flushed != 3 {
		t.Fatalf("expected flushed=3, got %d", flushed)
	}
	var afterFlush int64
	if err := appDB.DB.QueryRow("SELECT clicks_count FROM short_links WHERE code = ?1", code).Scan(&afterFlush); err != nil {
		t.Fatal(err)
	}
	if afterFlush != 3 {
		t.Fatalf("expected clicks_count=3 after flush, got %d", afterFlush)
	}
}

func TestLoginRateLimitPersistenceAndClickRestore(t *testing.T) {
	appDB, err := InitLocalForTest(":memory:")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = appDB.Close() }()

	// 1. Kiểm tra DB-backed login rate limit
	now := int64(20000)
	for i := uint32(1); i <= 5; i++ {
		n, err := RecordLoginFailure(appDB.DB, "9.8.7.6", now, 900)
		if err != nil || n != i {
			t.Fatalf("expected attempts=%d, got %d (err=%v)", i, n, err)
		}
	}
	gotN, gotTS, err := GetLoginRateLimit(appDB.DB, "9.8.7.6")
	if err != nil || gotN != 5 || gotTS != now {
		t.Fatalf("unexpected GetLoginRateLimit: n=%d ts=%d err=%v", gotN, gotTS, err)
	}
	// Quá cửa sổ 900s tự reset về 1
	resetN, err := RecordLoginFailure(appDB.DB, "9.8.7.6", now+1000, 900)
	if err != nil || resetN != 1 {
		t.Fatalf("expected reset to 1 after window expiry, got %d (err=%v)", resetN, err)
	}
	if err := ClearLoginRateLimit(appDB.DB, "9.8.7.6"); err != nil {
		t.Fatal(err)
	}
	clearedN, _, err := GetLoginRateLimit(appDB.DB, "9.8.7.6")
	if err != nil || clearedN != 0 {
		t.Fatalf("expected clearedN=0, got %d (err=%v)", clearedN, err)
	}

	// 2. Kiểm tra hoàn trả pending clicks khi flush lỗi DB
	appDB.SetCacheEnabledForTest(true)
	appDB.RecordShortLinkClickBuffered("ab12CD34")
	appDB.RecordShortLinkClickBuffered("ab12CD34")
	// Đóng kết nối DB để mô phỏng lỗi mạng khi FlushShortLinkClicks
	_ = appDB.Close()
	if flushed := appDB.FlushShortLinkClicks(); flushed != 0 {
		t.Fatalf("expected flushed=0 when DB closed, got %d", flushed)
	}
	appDB.cache.muClicks.Lock()
	restored := appDB.cache.pendingShortClicks["ab12CD34"]
	appDB.cache.muClicks.Unlock()
	if restored != 2 {
		t.Fatalf("expected 2 clicks restored into RAM buffer after DB flush error, got %d", restored)
	}
}

func TestSensitiveSettingsEncryptedAtRestAES256GCM(t *testing.T) {
	appDB, err := InitLocalForTest(":memory:")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = appDB.Close() }()

	SetSettingsEncryptionSeed("test-secret-master-key-2026")
	defer SetSettingsEncryptionSeed("unit-test-default-seed")

	const rawToken = "at_live_super_secret_key_999"
	if err := SetSetting(appDB.DB, "accesstrade_api_token", rawToken); err != nil {
		t.Fatalf("SetSetting encrypted failed: %v", err)
	}
	if err := SetSetting(appDB.DB, "campaign_shopee", "camp_public_123"); err != nil {
		t.Fatalf("SetSetting public failed: %v", err)
	}

	// Kiểm tra trực tiếp giá trị thô trong cột value của bảng settings: phải là enc:v1:..., KHÔNG chứa plaintext
	var rawStoredSecret, rawStoredPublic string
	if err := appDB.DB.QueryRow("SELECT value FROM settings WHERE key = 'accesstrade_api_token'").Scan(&rawStoredSecret); err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(rawStoredSecret, "enc:v1:") || strings.Contains(rawStoredSecret, "super_secret") {
		t.Fatalf("expected AES-256-GCM ciphertext in DB at-rest, got: %q", rawStoredSecret)
	}
	if err := appDB.DB.QueryRow("SELECT value FROM settings WHERE key = 'campaign_shopee'").Scan(&rawStoredPublic); err != nil {
		t.Fatal(err)
	}
	if rawStoredPublic != "camp_public_123" {
		t.Fatalf("non-sensitive setting should stay plain, got: %q", rawStoredPublic)
	}

	// Đọc qua GetSetting & GetSettingsMap tự động giải mã chính xác
	decPtr, err := GetSetting(appDB.DB, "accesstrade_api_token")
	if err != nil || decPtr == nil || *decPtr != rawToken {
		t.Fatalf("GetSetting decrypt mismatch: %v, err=%v", decPtr, err)
	}
	m, err := GetSettingsMap(appDB.DB)
	if err != nil || m["accesstrade_api_token"] != rawToken {
		t.Fatalf("GetSettingsMap decrypt mismatch: %q, err=%v", m["accesstrade_api_token"], err)
	}
}
