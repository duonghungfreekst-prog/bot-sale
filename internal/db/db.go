package db

import (
	"database/sql"
	"fmt"
	"log/slog"
	"net/url"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	_ "github.com/tursodatabase/libsql-client-go/libsql"
	_ "modernc.org/sqlite"

	"salehunter-go/internal/config"
	"salehunter-go/internal/utils"
)

const (
	l1SettingsTTL     = 20 * time.Second
	l1DealsTTL        = 20 * time.Second
	shortlinkCacheMax = 4096
)

type settingsCacheEntry struct {
	data map[string]string
	ts   time.Time
}

type dealsCacheEntry struct {
	list []DealItem
	ts   time.Time
}

type dbRuntimeCache struct {
	enabled            atomic.Bool
	muSettings         sync.RWMutex
	settings           *settingsCacheEntry
	muDeals            sync.RWMutex
	activeDeals        *dealsCacheEntry
	muShort            sync.RWMutex
	shortCodeByURL     map[string]string
	shortLinkByCode    map[string]ShortLinkRecord
	muClicks           sync.Mutex
	pendingShortClicks map[string]int64
	flushingClicks     atomic.Bool
}

func newDBRuntimeCache(enabled bool) *dbRuntimeCache {
	c := &dbRuntimeCache{
		shortCodeByURL:     make(map[string]string),
		shortLinkByCode:    make(map[string]ShortLinkRecord),
		pendingShortClicks: make(map[string]int64),
	}
	c.enabled.Store(enabled)
	return c
}

func pruneBoundedStringMap[V any](m map[string]V, maxEntries int) {
	if len(m) < maxEntries {
		return
	}
	dropN := maxEntries / 5
	if dropN < 1 {
		dropN = 1
	}
	removed := 0
	for k := range m {
		delete(m, k)
		removed++
		if removed >= dropN {
			break
		}
	}
}

// AppDb quản lý kết nối database/sql (Turso Cloud hoặc SQLite Pure Go) cùng bộ đệm L1 RAM.
type AppDb struct {
	DB       *sql.DB
	IsRemote bool
	cache    *dbRuntimeCache
}

// Init khởi tạo kết nối database chuẩn Production/Hosting:
// bắt buộc Turso Cloud khi chạy Cloud/Hosting và chỉ cho phép SQLite cục bộ khi chạy PC Local.
func Init(cfg config.Config) (*AppDb, error) {
	return initWithCacheFlag(cfg, true)
}

func initWithCacheFlag(cfg config.Config, cacheEnabled bool) (*AppDb, error) {
	SetSettingsEncryptionSeed(cfg.SettingsEncryptionSeed())
	tursoURL := strings.TrimSpace(cfg.TursoURL)
	tursoToken := strings.TrimSpace(cfg.TursoAuthToken)

	var (
		sqlDB    *sql.DB
		err      error
		isRemote bool
	)

	if tursoURL != "" && tursoToken != "" {
		slog.Info("☁️ Kết nối đến Turso Cloud Database từ xa", "url", tursoURL)
		dsn := tursoURL
		if strings.Contains(dsn, "?") {
			dsn += "&authToken=" + url.QueryEscape(tursoToken)
		} else {
			dsn += "?authToken=" + url.QueryEscape(tursoToken)
		}
		sqlDB, err = sql.Open("libsql", dsn)
		if err != nil {
			return nil, fmt.Errorf("không thể khởi tạo kết nối Turso Cloud: %w", err)
		}
		sqlDB.SetMaxOpenConns(16)
		sqlDB.SetMaxIdleConns(4)
		sqlDB.SetConnMaxLifetime(5 * time.Minute)
		isRemote = true
	} else if cfg.IsCloudRuntime() {
		return nil, fmt.Errorf(
			"đã tắt fallback SQLite cục bộ trên môi trường Hosting/Cloud: bắt buộc phải cấu hình TURSO_DATABASE_URL và TURSO_AUTH_TOKEN (từ chối tạo file %q)",
			cfg.DatabasePath,
		)
	} else {
		slog.Info("💾 Khởi tạo SQLite Database cục bộ (PC Local)", "path", cfg.DatabasePath)
		sqlDB, err = openLocalSQLite(cfg.DatabasePath)
		if err != nil {
			return nil, fmt.Errorf("không thể khởi tạo SQLite cục bộ: %w", err)
		}
	}

	if err := sqlDB.Ping(); err != nil {
		_ = sqlDB.Close()
		return nil, fmt.Errorf("không thể ping cơ sở dữ liệu: %w", err)
	}

	appDB := &AppDb{
		DB:       sqlDB,
		IsRemote: isRemote,
		cache:    newDBRuntimeCache(cacheEnabled),
	}
	if err := appDB.migrate(); err != nil {
		_ = sqlDB.Close()
		return nil, err
	}
	return appDB, nil
}

// InitLocalForTest khởi tạo SQLite cục bộ thuần Go phục vụ kiểm thử (mặc định tắt L1 cache giống Rust cfg(test)).
func InitLocalForTest(databasePath string) (*AppDb, error) {
	SetSettingsEncryptionSeed("unit-test-default-seed")
	sqlDB, err := openLocalSQLite(databasePath)
	if err != nil {
		return nil, fmt.Errorf("không thể khởi tạo SQLite test: %w", err)
	}
	appDB := &AppDb{
		DB:       sqlDB,
		IsRemote: false,
		cache:    newDBRuntimeCache(false),
	}
	if err := appDB.migrate(); err != nil {
		_ = sqlDB.Close()
		return nil, err
	}
	return appDB, nil
}

func openLocalSQLite(path string) (*sql.DB, error) {
	dsn := path
	if path == ":memory:" {
		dsn = "file::memory:?_pragma=foreign_keys(ON)"
	} else {
		sep := "?"
		if strings.Contains(path, "?") {
			sep = "&"
		}
		dsn = path + sep + "_pragma=journal_mode(WAL)&_pragma=busy_timeout(5000)&_pragma=foreign_keys(ON)"
	}
	db, err := sql.Open("sqlite", dsn)
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	return db, nil
}

// SetCacheEnabledForTest bật/tắt L1 RAM cache trong unit test.
func (a *AppDb) SetCacheEnabledForTest(enabled bool) {
	if a != nil && a.cache != nil {
		a.cache.enabled.Store(enabled)
	}
}

// Close đóng pool kết nối database gọn gàng khi shutdown.
func (a *AppDb) Close() error {
	if a == nil || a.DB == nil {
		return nil
	}
	return a.DB.Close()
}

func (a *AppDb) migrate() error {
	stmts := []struct {
		sql  string
		desc string
	}{
		{
			sql: `CREATE TABLE IF NOT EXISTS users (
				telegram_id INTEGER PRIMARY KEY,
				username TEXT,
				full_name TEXT NOT NULL,
				referral_code TEXT UNIQUE NOT NULL,
				referred_by INTEGER,
				balance_available REAL NOT NULL DEFAULT 0.0,
				balance_pending REAL NOT NULL DEFAULT 0.0,
				balance_withdrawn REAL NOT NULL DEFAULT 0.0,
				created_at TEXT NOT NULL
			);`,
			desc: "users",
		},
		{
			sql: `CREATE TABLE IF NOT EXISTS clicks (
				id TEXT PRIMARY KEY,
				user_id INTEGER NOT NULL,
				platform TEXT NOT NULL,
				original_url TEXT NOT NULL,
				clean_url TEXT NOT NULL,
				affiliate_url TEXT NOT NULL,
				sub_id TEXT NOT NULL,
				created_at TEXT NOT NULL,
				FOREIGN KEY (user_id) REFERENCES users(telegram_id)
			);`,
			desc: "clicks",
		},
		{
			sql: `CREATE TABLE IF NOT EXISTS cashback_orders (
				id TEXT PRIMARY KEY,
				user_id INTEGER NOT NULL,
				order_id TEXT NOT NULL,
				platform TEXT NOT NULL,
				order_amount REAL NOT NULL,
				total_commission REAL NOT NULL,
				user_cashback REAL NOT NULL,
				referrer_bonus REAL NOT NULL,
				referrer_id INTEGER,
				status TEXT NOT NULL,
				created_at TEXT NOT NULL,
				updated_at TEXT NOT NULL,
				FOREIGN KEY (user_id) REFERENCES users(telegram_id)
			);`,
			desc: "cashback_orders",
		},
		{
			sql: `CREATE TABLE IF NOT EXISTS withdrawals (
				id TEXT PRIMARY KEY,
				user_id INTEGER NOT NULL,
				amount REAL NOT NULL,
				bank_name TEXT NOT NULL,
				account_number TEXT NOT NULL,
				account_name TEXT NOT NULL,
				status TEXT NOT NULL,
				created_at TEXT NOT NULL,
				FOREIGN KEY (user_id) REFERENCES users(telegram_id)
			);`,
			desc: "withdrawals",
		},
		{
			sql: `CREATE TABLE IF NOT EXISTS deals (
				id INTEGER PRIMARY KEY AUTOINCREMENT,
				platform TEXT NOT NULL,
				title TEXT NOT NULL,
				coupon_code TEXT,
				discount_desc TEXT NOT NULL,
				min_spend REAL NOT NULL DEFAULT 0.0,
				deal_url TEXT NOT NULL,
				expire_at TEXT,
				is_active INTEGER NOT NULL DEFAULT 1,
				is_exclusive INTEGER NOT NULL DEFAULT 0,
				created_at TEXT NOT NULL DEFAULT ''
			);`,
			desc: "deals",
		},
		{
			sql: `CREATE TABLE IF NOT EXISTS settings (
				key TEXT PRIMARY KEY,
				value TEXT NOT NULL
			);`,
			desc: "settings",
		},
		{
			sql: `CREATE TABLE IF NOT EXISTS short_links (
				code TEXT PRIMARY KEY,
				target_url TEXT NOT NULL,
				title TEXT NOT NULL DEFAULT '',
				platform TEXT NOT NULL DEFAULT '',
				clicks_count INTEGER NOT NULL DEFAULT 0,
				created_at TEXT NOT NULL
			);`,
			desc: "short_links",
		},
		{
			sql: `CREATE TABLE IF NOT EXISTS admin_sessions (
				token_hash TEXT PRIMARY KEY,
				expires_at INTEGER NOT NULL,
				created_at TEXT NOT NULL
			);`,
			desc: "admin_sessions",
		},
		{
			sql: `CREATE TABLE IF NOT EXISTS tracked_orders (
				order_key TEXT PRIMARY KEY,
				platform TEXT NOT NULL,
				order_id TEXT NOT NULL,
				status TEXT NOT NULL,
				order_amount REAL NOT NULL DEFAULT 0.0,
				commission REAL NOT NULL DEFAULT 0.0,
				sub_id TEXT NOT NULL DEFAULT '',
				order_time TEXT NOT NULL DEFAULT '',
				notified_at TEXT NOT NULL
			);`,
			desc: "tracked_orders",
		},
		{
			sql: `CREATE TABLE IF NOT EXISTS login_rate_limits (
				ip TEXT PRIMARY KEY,
				attempts INTEGER NOT NULL,
				last_attempt INTEGER NOT NULL
			);`,
			desc: "login_rate_limits",
		},
	}

	for _, st := range stmts {
		if _, err := a.DB.Exec(st.sql); err != nil {
			return fmt.Errorf("tạo bảng %s thất bại: %w", st.desc, err)
		}
	}

	// Bổ sung cột mới an toàn cho DB cũ (bỏ qua nếu đã tồn tại)
	_, _ = a.DB.Exec("ALTER TABLE deals ADD COLUMN is_exclusive INTEGER NOT NULL DEFAULT 0")
	_, _ = a.DB.Exec("ALTER TABLE deals ADD COLUMN created_at TEXT NOT NULL DEFAULT ''")

	indexes := []string{
		"CREATE INDEX IF NOT EXISTS idx_short_links_target ON short_links(target_url);",
		"CREATE INDEX IF NOT EXISTS idx_tracked_orders_notified ON tracked_orders(notified_at DESC);",
		"CREATE INDEX IF NOT EXISTS idx_clicks_created_at ON clicks(created_at DESC);",
		"CREATE INDEX IF NOT EXISTS idx_clicks_platform ON clicks(platform);",
		"CREATE INDEX IF NOT EXISTS idx_deals_active_exclusive ON deals(is_active, is_exclusive DESC, id DESC);",
		"CREATE INDEX IF NOT EXISTS idx_admin_sessions_expires ON admin_sessions(expires_at);",
		"CREATE INDEX IF NOT EXISTS idx_login_rate_limits_last ON login_rate_limits(last_attempt);",
	}
	for _, idxSQL := range indexes {
		_, _ = a.DB.Exec(idxSQL)
	}

	// Dọn dẹp triệt để các bản ghi seed giả mẫu cũ & bài hướng dẫn rác
	_, _ = a.DB.Exec(`DELETE FROM deals WHERE
		(is_exclusive = 0 AND (
			coupon_code LIKE 'AUTO_%'
			OR coupon_code IN ('SHOPEE50K', 'FREESHIP30', 'LAZADA100', 'TIKINOW20', 'TTSALE30')
			OR (platform = 'Lazada' AND coupon_code IS NULL AND title = 'Hoàn Tiền Max 12%')
		))
		OR (
			(coupon_code LIKE 'KOL_%' OR coupon_code LIKE 'SHP_AUTO_%')
			AND (
				LOWER(discount_desc) LIKE '%hướng dẫn lấy mã%'
				OR LOWER(discount_desc) LIKE '%hướng dẫn săn mã%'
				OR LOWER(discount_desc) LIKE '%hướng dẫn áp mã%'
				OR LOWER(discount_desc) LIKE '%khi ấn link lấy mã%'
				OR LOWER(discount_desc) LIKE '%cách lấy mã%'
			)
		);`)

	slog.Info("LibSQL/SQLite schema migration thành công.")
	return nil
}

// GetSettingsMapCached đọc toàn bộ cấu hình nhẹ từ L1 RAM cache (TTL 20s) hoặc 1 truy vấn SQL.
func (a *AppDb) GetSettingsMapCached() (map[string]string, error) {
	if a.cache.enabled.Load() {
		a.cache.muSettings.RLock()
		entry := a.cache.settings
		if entry != nil && time.Since(entry.ts) < l1SettingsTTL {
			cloned := make(map[string]string, len(entry.data))
			for k, v := range entry.data {
				cloned[k] = v
			}
			a.cache.muSettings.RUnlock()
			return cloned, nil
		}
		a.cache.muSettings.RUnlock()
	}

	fresh, err := GetSettingsMap(a.DB)
	if err != nil {
		return nil, err
	}
	if a.cache.enabled.Load() {
		cloned := make(map[string]string, len(fresh))
		for k, v := range fresh {
			cloned[k] = v
		}
		a.cache.muSettings.Lock()
		a.cache.settings = &settingsCacheEntry{data: cloned, ts: time.Now()}
		a.cache.muSettings.Unlock()
	}
	return fresh, nil
}

// InvalidateSettingsCache xóa cache settings ngay sau khi Admin/Harvester cập nhật.
func (a *AppDb) InvalidateSettingsCache() {
	a.cache.muSettings.Lock()
	a.cache.settings = nil
	a.cache.muSettings.Unlock()
}

// GetActiveDealsCached đọc danh sách deal hoạt động từ L1 RAM Cache (TTL 20s).
func (a *AppDb) GetActiveDealsCached(platform string) ([]DealItem, error) {
	platTrim := strings.TrimSpace(platform)
	if !a.cache.enabled.Load() {
		return GetActiveDeals(a.DB, platTrim)
	}

	a.cache.muDeals.RLock()
	entry := a.cache.activeDeals
	var allDeals []DealItem
	if entry != nil && time.Since(entry.ts) < l1DealsTTL {
		allDeals = make([]DealItem, len(entry.list))
		copy(allDeals, entry.list)
	}
	a.cache.muDeals.RUnlock()

	if allDeals == nil {
		fresh, err := GetActiveDeals(a.DB, "")
		if err != nil {
			return nil, err
		}
		copied := make([]DealItem, len(fresh))
		copy(copied, fresh)
		a.cache.muDeals.Lock()
		a.cache.activeDeals = &dealsCacheEntry{list: copied, ts: time.Now()}
		a.cache.muDeals.Unlock()
		allDeals = fresh
	}

	if platTrim == "" {
		return allDeals, nil
	}
	filtered := make([]DealItem, 0, len(allDeals))
	for _, d := range allDeals {
		if strings.EqualFold(d.Platform, platTrim) {
			filtered = append(filtered, d)
		}
	}
	return filtered, nil
}

// InvalidateDealsCache xóa cache deal khi danh sách thay đổi.
func (a *AppDb) InvalidateDealsCache() {
	a.cache.muDeals.Lock()
	a.cache.activeDeals = nil
	a.cache.muDeals.Unlock()
}

// ResolveShortLinkCached tra cứu shortlink từ L1 RAM Cache trước, miss mới đọc DB.
func (a *AppDb) ResolveShortLinkCached(code string) (*ShortLinkRecord, error) {
	if a.cache.enabled.Load() {
		a.cache.muShort.RLock()
		rec, ok := a.cache.shortLinkByCode[code]
		a.cache.muShort.RUnlock()
		if ok {
			copyRec := rec
			return &copyRec, nil
		}
	}

	rec, err := ResolveShortLink(a.DB, code)
	if err != nil || rec == nil {
		return rec, err
	}
	if a.cache.enabled.Load() {
		a.cache.muShort.Lock()
		pruneBoundedStringMap(a.cache.shortLinkByCode, shortlinkCacheMax)
		a.cache.shortLinkByCode[rec.Code] = *rec
		pruneBoundedStringMap(a.cache.shortCodeByURL, shortlinkCacheMax)
		a.cache.shortCodeByURL[rec.TargetURL] = rec.Code
		a.cache.muShort.Unlock()
	}
	return rec, nil
}

// WrapLinkItem đại diện 1 URL cần bọc ngắn hàng loạt.
type WrapLinkItem struct {
	TargetURL string
	Title     string
	Platform  string
}

// WrapURLCached bọc 1 link có tận dụng L1 shortlink cache.
func (a *AppDb) WrapURLCached(baseURLOpt, targetURL, title, platform string) string {
	res := a.WrapURLsBatchCached(baseURLOpt, []WrapLinkItem{{
		TargetURL: targetURL,
		Title:     title,
		Platform:  platform,
	}})
	if len(res) > 0 {
		return res[0]
	}
	return strings.TrimSpace(targetURL)
}

// WrapURLsBatchCached bọc hàng loạt link cho Trang chủ / danh sách deal, giảm 24 round-trips xuống 0-1 query.
func (a *AppDb) WrapURLsBatchCached(baseURLOpt string, items []WrapLinkItem) []string {
	out := make([]string, len(items))
	base := strings.TrimRight(strings.TrimSpace(baseURLOpt), "/")
	if !strings.HasPrefix(base, "https://") && !strings.HasPrefix(base, "http://") {
		for i, it := range items {
			out[i] = strings.TrimSpace(it.TargetURL)
		}
		return out
	}

	cacheOn := a.cache.enabled.Load()
	resolved := make(map[string]string, len(items))
	missing := make([]string, 0)
	seenMissing := make(map[string]struct{}, len(items))

	if cacheOn {
		a.cache.muShort.RLock()
		for _, it := range items {
			u := strings.TrimSpace(it.TargetURL)
			if u == "" || strings.HasPrefix(u, base) {
				continue
			}
			if code, ok := a.cache.shortCodeByURL[u]; ok {
				resolved[u] = code
			} else if _, dup := seenMissing[u]; !dup {
				seenMissing[u] = struct{}{}
				missing = append(missing, u)
			}
		}
		a.cache.muShort.RUnlock()
	} else {
		for _, it := range items {
			u := strings.TrimSpace(it.TargetURL)
			if u != "" && !strings.HasPrefix(u, base) {
				if _, dup := seenMissing[u]; !dup {
					seenMissing[u] = struct{}{}
					missing = append(missing, u)
				}
			}
		}
	}

	if len(missing) > 0 {
		if found, err := GetShortCodesByTargets(a.DB, missing); err == nil {
			for u, code := range found {
				resolved[u] = code
			}
		}

		newlyCached := make([]ShortLinkRecord, 0)
		for _, it := range items {
			u := strings.TrimSpace(it.TargetURL)
			if u == "" || strings.HasPrefix(u, base) {
				continue
			}
			if _, exists := resolved[u]; exists {
				continue
			}
			if code, err := CreateOrGetShortLink(a.DB, u, strings.TrimSpace(it.Title), strings.TrimSpace(it.Platform)); err == nil {
				resolved[u] = code
				newlyCached = append(newlyCached, ShortLinkRecord{
					Code:      code,
					TargetURL: u,
					Title:     strings.TrimSpace(it.Title),
					Platform:  strings.TrimSpace(it.Platform),
				})
			}
		}

		if cacheOn && (len(newlyCached) > 0 || len(resolved) > 0) {
			a.cache.muShort.Lock()
			pruneBoundedStringMap(a.cache.shortCodeByURL, shortlinkCacheMax)
			for u, code := range resolved {
				a.cache.shortCodeByURL[u] = code
			}
			pruneBoundedStringMap(a.cache.shortLinkByCode, shortlinkCacheMax)
			for _, rec := range newlyCached {
				a.cache.shortLinkByCode[rec.Code] = rec
			}
			a.cache.muShort.Unlock()
		}
	}

	for i, it := range items {
		u := strings.TrimSpace(it.TargetURL)
		if u == "" || strings.HasPrefix(u, base) {
			out[i] = u
			continue
		}
		if code, ok := resolved[u]; ok {
			out[i] = base + "/" + code
		} else {
			out[i] = u
		}
	}
	return out
}

func (a *AppDb) restoreShortLinkClicks(batch map[string]int64) {
	if len(batch) == 0 {
		return
	}
	maxKeep := shortlinkCacheMax * 4
	a.cache.muClicks.Lock()
	for code, delta := range batch {
		if delta <= 0 {
			continue
		}
		if _, exists := a.cache.pendingShortClicks[code]; !exists && len(a.cache.pendingShortClicks) >= maxKeep {
			continue
		}
		a.cache.pendingShortClicks[code] += delta
	}
	a.cache.muClicks.Unlock()
}

// RecordShortLinkClickBuffered cộng dồn lượt click trong RAM (chống spam write Turso),
// hoặc ghi thẳng khi tắt L1 cache. Tự flush nếu số mã đang chờ vượt ngưỡng cache.
func (a *AppDb) RecordShortLinkClickBuffered(code string) {
	if a == nil || a.DB == nil || !IsValidShortCode(code) {
		return
	}
	if !a.cache.enabled.Load() {
		_ = IncrementShortLinkClick(a.DB, code)
		return
	}
	var flushBatch map[string]int64
	a.cache.muClicks.Lock()
	a.cache.pendingShortClicks[code]++
	if len(a.cache.pendingShortClicks) >= shortlinkCacheMax && a.cache.flushingClicks.CompareAndSwap(false, true) {
		flushBatch = a.cache.pendingShortClicks
		a.cache.pendingShortClicks = make(map[string]int64)
	}
	a.cache.muClicks.Unlock()

	if len(flushBatch) > 0 {
		utils.SafeGo(nil, "db.flush_shortlink_clicks", func() {
			defer a.cache.flushingClicks.Store(false)
			if err := IncrementShortLinkClicksBatch(a.DB, flushBatch); err != nil {
				slog.Warn("Không thể flush batch clicks shortlink, sẽ giữ lại để thử lần sau", "err", err)
				a.restoreShortLinkClicks(flushBatch)
			}
		})
	}
}

// FlushShortLinkClicks ghi toàn bộ bộ đếm click shortlink từ RAM xuống DB trong 1 transaction.
// Nếu DB tạm thời lỗi mạng, tự động hoàn trả batch về RAM để không mất số liệu.
func (a *AppDb) FlushShortLinkClicks() int {
	if a == nil || a.DB == nil {
		return 0
	}
	a.cache.muClicks.Lock()
	if len(a.cache.pendingShortClicks) == 0 {
		a.cache.muClicks.Unlock()
		return 0
	}
	batch := a.cache.pendingShortClicks
	a.cache.pendingShortClicks = make(map[string]int64)
	a.cache.muClicks.Unlock()

	if err := IncrementShortLinkClicksBatch(a.DB, batch); err != nil {
		slog.Warn("Lỗi flush shortlink clicks batch xuống DB, hoàn trả vào bộ đệm để thử lại", "err", err)
		a.restoreShortLinkClicks(batch)
		return 0
	}
	total := 0
	for _, delta := range batch {
		total += int(delta)
	}
	return total
}

// CleanupExpiredSessions dọn phiên đăng nhập Admin đã hết hạn.
func (a *AppDb) CleanupExpiredSessions() (int64, error) {
	return CleanupExpiredAdminSessions(a.DB, time.Now().UTC().Unix())
}

// CleanupExpiredLoginRateLimits dọn các bản ghi khóa brute-force quá hạn 15 phút trong DB.
func (a *AppDb) CleanupExpiredLoginRateLimits() (int64, error) {
	if a == nil || a.DB == nil {
		return 0, nil
	}
	return CleanupExpiredLoginRateLimits(a.DB, time.Now().UTC().Unix()-900)
}
