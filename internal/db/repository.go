package db

import (
	"crypto/rand"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"errors"
	"fmt"
	"log/slog"
	"math"
	"math/big"
	"strings"
	"time"

	"github.com/google/uuid"

	"salehunter-go/internal/utils"
)

const shortCodeAlphabet = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789"

func sha256HexLower(b []byte) string {
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

func generateShortCode() string {
	var b strings.Builder
	b.Grow(8)
	maxIdx := big.NewInt(int64(len(shortCodeAlphabet)))
	for i := 0; i < 8; i++ {
		idx := 0
		if n, err := rand.Int(rand.Reader, maxIdx); err == nil {
			idx = int(n.Int64())
		}
		b.WriteByte(shortCodeAlphabet[idx])
	}
	return b.String()
}

type rowScanner interface {
	Scan(dest ...any) error
}

func scanDealItem(s rowScanner) (DealItem, error) {
	var (
		d          DealItem
		couponNull sql.NullString
		expireNull sql.NullString
		activeInt  int64
		exclInt    int64
	)
	err := s.Scan(
		&d.ID,
		&d.Platform,
		&d.Title,
		&couponNull,
		&d.DiscountDesc,
		&d.MinSpend,
		&d.DealURL,
		&expireNull,
		&activeInt,
		&exclInt,
	)
	if err != nil {
		return d, err
	}
	if couponNull.Valid {
		v := couponNull.String
		d.CouponCode = &v
	}
	if expireNull.Valid {
		v := expireNull.String
		d.ExpireAt = &v
	}
	d.IsActive = activeInt != 0
	d.IsExclusive = exclInt != 0
	return d, nil
}

func scanUser(s rowScanner) (*User, error) {
	var (
		u        User
		userNull sql.NullString
		refNull  sql.NullInt64
	)
	err := s.Scan(
		&u.TelegramID,
		&userNull,
		&u.FullName,
		&u.ReferralCode,
		&refNull,
		&u.BalanceAvailable,
		&u.BalancePending,
		&u.BalanceWithdrawn,
		&u.CreatedAt,
	)
	if err != nil {
		return nil, err
	}
	if userNull.Valid {
		v := userNull.String
		u.Username = &v
	}
	if refNull.Valid {
		v := refNull.Int64
		u.ReferredBy = &v
	}
	return &u, nil
}

func ptrStrEqual(a, b *string) bool {
	if a == nil && b == nil {
		return true
	}
	if a == nil || b == nil {
		return false
	}
	return *a == *b
}

// GetOrCreateUser lấy thông tin user hiện có hoặc tạo user mới, trả về (User, isNew).
func GetOrCreateUser(db *sql.DB, telegramID int64, username *string, fullName string, referredByCode *string) (*User, bool, error) {
	existing, err := GetUserByTelegramID(db, telegramID)
	if err != nil {
		return nil, false, err
	}
	if existing != nil {
		if !ptrStrEqual(existing.Username, username) || existing.FullName != fullName {
			var uArg any
			if username != nil {
				uArg = *username
			}
			_, _ = db.Exec(
				"UPDATE users SET username = ?1, full_name = ?2 WHERE telegram_id = ?3",
				uArg, fullName, telegramID,
			)
			existing.Username = username
			existing.FullName = fullName
		}
		return existing, false, nil
	}

	var referrerID *int64
	if referredByCode != nil && strings.TrimSpace(*referredByCode) != "" {
		cleanCode := strings.ToUpper(strings.TrimSpace(*referredByCode))
		if refUser, err := GetUserByRefCode(db, cleanCode); err == nil && refUser != nil && refUser.TelegramID != telegramID {
			id := refUser.TelegramID
			referrerID = &id
		}
	}

	refCode := utils.GenerateReferralCode()
	for i := 0; i < 5; i++ {
		if dup, err := GetUserByRefCode(db, refCode); err == nil && dup == nil {
			break
		}
		refCode = utils.GenerateReferralCode()
	}

	now := time.Now().UTC().Format(time.RFC3339)
	var uArg any
	if username != nil {
		uArg = *username
	}
	var refArg any
	if referrerID != nil {
		refArg = *referrerID
	}

	if _, err := db.Exec(
		`INSERT INTO users (telegram_id, username, full_name, referral_code, referred_by, balance_available, balance_pending, balance_withdrawn, created_at)
		 VALUES (?1, ?2, ?3, ?4, ?5, 0.0, 0.0, 0.0, ?6)`,
		telegramID, uArg, fullName, refCode, refArg, now,
	); err != nil {
		return nil, false, fmt.Errorf("không thể tạo người dùng mới: %w", err)
	}

	slog.Info("Đã đăng ký người dùng mới", "telegram_id", telegramID, "ref_code", refCode)
	return &User{
		TelegramID:   telegramID,
		Username:     username,
		FullName:     fullName,
		ReferralCode: refCode,
		ReferredBy:   referrerID,
		CreatedAt:    now,
	}, true, nil
}

// GetUserByTelegramID tìm user theo Telegram ID.
func GetUserByTelegramID(db *sql.DB, telegramID int64) (*User, error) {
	row := db.QueryRow(
		"SELECT telegram_id, username, full_name, referral_code, referred_by, balance_available, balance_pending, balance_withdrawn, created_at FROM users WHERE telegram_id = ?1",
		telegramID,
	)
	u, err := scanUser(row)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return u, nil
}

// GetUserByRefCode tìm user theo mã giới thiệu.
func GetUserByRefCode(db *sql.DB, refCode string) (*User, error) {
	row := db.QueryRow(
		"SELECT telegram_id, username, full_name, referral_code, referred_by, balance_available, balance_pending, balance_withdrawn, created_at FROM users WHERE referral_code = ?1",
		refCode,
	)
	u, err := scanUser(row)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return u, nil
}

// CountReferrals đếm số lượt F1 đã giới thiệu.
func CountReferrals(db *sql.DB, telegramID int64) (int64, error) {
	var cnt int64
	if err := db.QueryRow("SELECT COUNT(*) FROM users WHERE referred_by = ?1", telegramID).Scan(&cnt); err != nil {
		return 0, err
	}
	return cnt, nil
}

// GetWalletSummary lấy tổng hợp số dư và thống kê ví.
func GetWalletSummary(db *sql.DB, telegramID int64) (*WalletSummary, error) {
	user, err := GetUserByTelegramID(db, telegramID)
	if err != nil {
		return nil, err
	}
	if user == nil {
		return nil, errors.New("không tìm thấy người dùng")
	}
	totalRefs, err := CountReferrals(db, telegramID)
	if err != nil {
		return nil, err
	}
	var totalRefBonus float64
	_ = db.QueryRow(
		"SELECT COALESCE(SUM(referrer_bonus), 0.0) FROM cashback_orders WHERE referrer_id = ?1 AND status = 'APPROVED'",
		telegramID,
	).Scan(&totalRefBonus)

	return &WalletSummary{
		BalanceAvailable: user.BalanceAvailable,
		BalancePending:   user.BalancePending,
		BalanceWithdrawn: user.BalanceWithdrawn,
		TotalEarned:      user.BalanceAvailable + user.BalanceWithdrawn,
		ReferralCode:     user.ReferralCode,
		TotalReferrals:   totalRefs,
		TotalRefBonus:    totalRefBonus,
	}, nil
}

// RecordClick ghi nhận 1 lượt khách hàng dán link.
func RecordClick(db *sql.DB, c ClickRecord) error {
	_, err := db.Exec(
		`INSERT INTO clicks (id, user_id, platform, original_url, clean_url, affiliate_url, sub_id, created_at)
		 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)`,
		c.ID, c.UserID, c.Platform, c.OriginalURL, c.CleanURL, c.AffiliateURL, c.SubID, c.CreatedAt,
	)
	return err
}

// GetRecentClicks lấy lịch sử dán link gần nhất của 1 user.
func GetRecentClicks(db *sql.DB, userID int64, limit int64) ([]ClickRecord, error) {
	rows, err := db.Query(
		`SELECT id, user_id, platform, original_url, clean_url, affiliate_url, sub_id, created_at
		 FROM clicks WHERE user_id = ?1 ORDER BY created_at DESC LIMIT ?2`,
		userID, limit,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var list []ClickRecord
	for rows.Next() {
		var c ClickRecord
		if err := rows.Scan(&c.ID, &c.UserID, &c.Platform, &c.OriginalURL, &c.CleanURL, &c.AffiliateURL, &c.SubID, &c.CreatedAt); err != nil {
			return nil, err
		}
		list = append(list, c)
	}
	return list, rows.Err()
}

// RecordCashbackOrder lưu đơn hoàn tiền mới và cộng balance_pending.
func RecordCashbackOrder(db *sql.DB, o CashbackOrder) error {
	var refArg any
	if o.ReferrerID != nil {
		refArg = *o.ReferrerID
	}
	if _, err := db.Exec(
		`INSERT INTO cashback_orders (id, user_id, order_id, platform, order_amount, total_commission, user_cashback, referrer_bonus, referrer_id, status, created_at, updated_at)
		 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)`,
		o.ID, o.UserID, o.OrderID, o.Platform, o.OrderAmount, o.TotalCommission, o.UserCashback, o.ReferrerBonus, refArg, o.Status, o.CreatedAt, o.UpdatedAt,
	); err != nil {
		return err
	}
	_, err := db.Exec("UPDATE users SET balance_pending = balance_pending + ?1 WHERE telegram_id = ?2", o.UserCashback, o.UserID)
	return err
}

// ApproveCashbackOrder duyệt đơn hoàn tiền (idempotent).
func ApproveCashbackOrder(db *sql.DB, orderID string) error {
	var (
		id, status   string
		userID       int64
		userCashback float64
		refBonus     float64
		refNull      sql.NullInt64
	)
	err := db.QueryRow(
		"SELECT id, user_id, user_cashback, referrer_bonus, referrer_id, status FROM cashback_orders WHERE order_id = ?1",
		orderID,
	).Scan(&id, &userID, &userCashback, &refBonus, &refNull, &status)
	if errors.Is(err, sql.ErrNoRows) {
		return fmt.Errorf("không tìm thấy đơn hàng %s", orderID)
	}
	if err != nil {
		return err
	}
	if status == "APPROVED" {
		return nil
	}

	now := time.Now().UTC().Format(time.RFC3339)
	if _, err := db.Exec("UPDATE cashback_orders SET status = 'APPROVED', updated_at = ?1 WHERE order_id = ?2", now, orderID); err != nil {
		return err
	}
	if _, err := db.Exec(
		"UPDATE users SET balance_pending = MAX(0.0, balance_pending - ?1), balance_available = balance_available + ?1 WHERE telegram_id = ?2",
		userCashback, userID,
	); err != nil {
		return err
	}
	if refNull.Valid && refBonus > 0.0 {
		_, err = db.Exec("UPDATE users SET balance_available = balance_available + ?1 WHERE telegram_id = ?2", refBonus, refNull.Int64)
		return err
	}
	return nil
}

// CreateWithdrawal tạo lệnh rút tiền.
func CreateWithdrawal(db *sql.DB, userID int64, amount float64, bankName, accNum, accName string, minLimit float64) (*Withdrawal, error) {
	if amount < minLimit {
		return nil, fmt.Errorf("số tiền rút tối thiểu là %s", utils.FormatVND(minLimit))
	}
	user, err := GetUserByTelegramID(db, userID)
	if err != nil {
		return nil, err
	}
	if user == nil {
		return nil, errors.New("không tìm thấy người dùng")
	}
	if user.BalanceAvailable < amount {
		return nil, fmt.Errorf("số dư khả dụng (%s) không đủ để rút", utils.FormatVND(user.BalanceAvailable))
	}
	wdID := "WD-" + strings.ReplaceAll(uuid.NewString(), "-", "")
	now := time.Now().UTC().Format(time.RFC3339)

	if _, err := db.Exec(
		"UPDATE users SET balance_available = balance_available - ?1, balance_withdrawn = balance_withdrawn + ?1 WHERE telegram_id = ?2",
		amount, userID,
	); err != nil {
		return nil, err
	}
	if _, err := db.Exec(
		`INSERT INTO withdrawals (id, user_id, amount, bank_name, account_number, account_name, status, created_at)
		 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'PENDING', ?7)`,
		wdID, userID, amount, bankName, accNum, accName, now,
	); err != nil {
		return nil, err
	}
	return &Withdrawal{
		ID:            wdID,
		UserID:        userID,
		Amount:        amount,
		BankName:      bankName,
		AccountNumber: accNum,
		AccountName:   accName,
		Status:        "PENDING",
		CreatedAt:     now,
	}, nil
}

// GetRecentWithdrawals lấy các yêu cầu rút tiền gần đây.
func GetRecentWithdrawals(db *sql.DB, userID int64, limit int64) ([]Withdrawal, error) {
	rows, err := db.Query(
		`SELECT id, user_id, amount, bank_name, account_number, account_name, status, created_at
		 FROM withdrawals WHERE user_id = ?1 ORDER BY created_at DESC LIMIT ?2`,
		userID, limit,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var list []Withdrawal
	for rows.Next() {
		var w Withdrawal
		if err := rows.Scan(&w.ID, &w.UserID, &w.Amount, &w.BankName, &w.AccountNumber, &w.AccountName, &w.Status, &w.CreatedAt); err != nil {
			return nil, err
		}
		list = append(list, w)
	}
	return list, rows.Err()
}

// GetActiveDeals lấy danh sách deal đang bật (lọc theo sàn nếu có), ưu tiên độc quyền.
func GetActiveDeals(db *sql.DB, platformFilter string) ([]DealItem, error) {
	var (
		rows *sql.Rows
		err  error
	)
	if strings.TrimSpace(platformFilter) != "" {
		rows, err = db.Query(
			`SELECT id, platform, title, coupon_code, discount_desc, min_spend, deal_url, expire_at, is_active, is_exclusive
			 FROM deals WHERE is_active = 1 AND LOWER(platform) = LOWER(?1)
			 ORDER BY is_exclusive DESC, min_spend ASC, id DESC`,
			platformFilter,
		)
	} else {
		rows, err = db.Query(
			`SELECT id, platform, title, coupon_code, discount_desc, min_spend, deal_url, expire_at, is_active, is_exclusive
			 FROM deals WHERE is_active = 1
			 ORDER BY is_exclusive DESC, platform, min_spend ASC, id DESC`,
		)
	}
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var list []DealItem
	for rows.Next() {
		d, err := scanDealItem(rows)
		if err != nil {
			return nil, err
		}
		list = append(list, d)
	}
	return list, rows.Err()
}

// GetRecentCashbackOrders lấy các đơn hoàn tiền của user.
func GetRecentCashbackOrders(db *sql.DB, userID int64, limit int64) ([]CashbackOrder, error) {
	rows, err := db.Query(
		`SELECT id, user_id, order_id, platform, order_amount, total_commission, user_cashback, referrer_bonus, referrer_id, status, created_at, updated_at
		 FROM cashback_orders WHERE user_id = ?1 ORDER BY created_at DESC LIMIT ?2`,
		userID, limit,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var list []CashbackOrder
	for rows.Next() {
		var (
			o       CashbackOrder
			refNull sql.NullInt64
		)
		if err := rows.Scan(
			&o.ID, &o.UserID, &o.OrderID, &o.Platform, &o.OrderAmount, &o.TotalCommission,
			&o.UserCashback, &o.ReferrerBonus, &refNull, &o.Status, &o.CreatedAt, &o.UpdatedAt,
		); err != nil {
			return nil, err
		}
		if refNull.Valid {
			v := refNull.Int64
			o.ReferrerID = &v
		}
		list = append(list, o)
	}
	return list, rows.Err()
}

// GetAdminDashboardStats gộp thống kê tổng quan thành 2 truy vấn nhẹ trên Turso.
func GetAdminDashboardStats(db *sql.DB) (AdminStats, error) {
	var stats AdminStats
	_ = db.QueryRow("SELECT (SELECT COUNT(*) FROM users), (SELECT COUNT(*) FROM deals)").
		Scan(&stats.TotalUsers, &stats.TotalDeals)

	_ = db.QueryRow(
		`SELECT COUNT(*),
				COALESCE(SUM(CASE WHEN LOWER(platform) = 'shopee' THEN 1 ELSE 0 END), 0),
				COALESCE(SUM(CASE WHEN LOWER(platform) = 'lazada' THEN 1 ELSE 0 END), 0),
				COALESCE(SUM(CASE WHEN LOWER(platform) = 'tiki' THEN 1 ELSE 0 END), 0),
				COALESCE(SUM(CASE WHEN LOWER(platform) LIKE '%tiktok%' THEN 1 ELSE 0 END), 0)
		 FROM clicks`,
	).Scan(&stats.TotalClicks, &stats.ShopeeClicks, &stats.LazadaClicks, &stats.TikiClicks, &stats.TikTokClicks)

	return stats, nil
}

// GetAllDealsForAdmin lấy tối đa 200 deal gần nhất cho trang quản trị.
func GetAllDealsForAdmin(db *sql.DB) ([]DealItem, error) {
	rows, err := db.Query(
		`SELECT id, platform, title, coupon_code, discount_desc, min_spend, deal_url, expire_at, is_active, is_exclusive
		 FROM deals ORDER BY is_exclusive DESC, id DESC LIMIT 200`,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var list []DealItem
	for rows.Next() {
		d, err := scanDealItem(rows)
		if err != nil {
			return nil, err
		}
		list = append(list, d)
	}
	return list, rows.Err()
}

// AddDeal thêm mới hoặc cập nhật deal nếu trùng (platform, coupon_code); trả true nếu thêm mới.
func AddDeal(db *sql.DB, input NewDealInput) (bool, error) {
	now := time.Now().UTC().Format(time.RFC3339)
	exclFlag := int64(0)
	if input.IsExclusive {
		exclFlag = 1
	}

	var couponArg any
	if input.CouponCode != nil {
		couponArg = *input.CouponCode
		trimmedCode := strings.TrimSpace(*input.CouponCode)
		if trimmedCode != "" {
			var dealID int64
			err := db.QueryRow(
				"SELECT id FROM deals WHERE LOWER(platform) = LOWER(?1) AND UPPER(coupon_code) = UPPER(?2) LIMIT 1",
				input.Platform, trimmedCode,
			).Scan(&dealID)
			if err == nil {
				var expArg any
				if input.ExpireAt != nil {
					expArg = *input.ExpireAt
				}
				_, err = db.Exec(
					"UPDATE deals SET title = ?1, discount_desc = ?2, min_spend = ?3, deal_url = ?4, expire_at = ?5, is_active = 1, is_exclusive = ?6 WHERE id = ?7",
					input.Title, input.DiscountDesc, input.MinSpend, input.DealURL, expArg, exclFlag, dealID,
				)
				return false, err
			}
		}
	}

	var expArg any
	if input.ExpireAt != nil {
		expArg = *input.ExpireAt
	}
	_, err := db.Exec(
		`INSERT INTO deals (platform, title, coupon_code, discount_desc, min_spend, deal_url, expire_at, is_active, is_exclusive, created_at)
		 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9)`,
		input.Platform, input.Title, couponArg, input.DiscountDesc, input.MinSpend, input.DealURL, expArg, exclFlag, now,
	)
	return true, err
}

// AddDealsBatch nhập hàng loạt deals tối ưu cho Turso: lọc trước deals không đổi và gộp trong 1 transaction.
func AddDealsBatch(db *sql.DB, items []NewDealInput) (int, int, error) {
	if len(items) == 0 {
		return 0, 0, nil
	}

	type dealKey struct {
		plat string
		code string
	}
	existingByCode := make(map[dealKey]*DealItem)
	rows, err := db.Query("SELECT id, platform, title, coupon_code, discount_desc, min_spend, deal_url, expire_at, is_active, is_exclusive FROM deals")
	if err != nil {
		return 0, 0, err
	}
	for rows.Next() {
		d, scanErr := scanDealItem(rows)
		if scanErr == nil && d.CouponCode != nil {
			trimmed := strings.TrimSpace(*d.CouponCode)
			if trimmed != "" {
				cp := d
				existingByCode[dealKey{plat: strings.ToLower(d.Platform), code: strings.ToUpper(trimmed)}] = &cp
			}
		}
	}
	_ = rows.Close()

	type pendingUpd struct {
		id       int64
		item     NewDealInput
		exclFlag int64
	}
	var (
		inserted       int
		updated        int
		pendingUpdates []pendingUpd
		pendingInserts []NewDealInput
	)
	now := time.Now().UTC().Format(time.RFC3339)

	for _, item := range items {
		exclFlag := int64(0)
		if item.IsExclusive {
			exclFlag = 1
		}
		trimmedCode := ""
		if item.CouponCode != nil {
			trimmedCode = strings.TrimSpace(*item.CouponCode)
		}
		if trimmedCode != "" {
			k := dealKey{plat: strings.ToLower(item.Platform), code: strings.ToUpper(trimmedCode)}
			if ex, ok := existingByCode[k]; ok {
				unchanged := ex.IsActive &&
					ex.IsExclusive == item.IsExclusive &&
					ex.Title == item.Title &&
					ex.DiscountDesc == item.DiscountDesc &&
					ex.DealURL == item.DealURL &&
					math.Abs(ex.MinSpend-item.MinSpend) < 0.01 &&
					ptrStrEqual(ex.ExpireAt, item.ExpireAt)
				updated++
				if !unchanged {
					ex.Title = item.Title
					ex.DiscountDesc = item.DiscountDesc
					ex.MinSpend = item.MinSpend
					ex.DealURL = item.DealURL
					ex.ExpireAt = item.ExpireAt
					ex.IsActive = true
					ex.IsExclusive = item.IsExclusive
					pendingUpdates = append(pendingUpdates, pendingUpd{id: ex.ID, item: item, exclFlag: exclFlag})
				}
				continue
			}
		}
		inserted++
		pendingInserts = append(pendingInserts, item)
	}

	if len(pendingUpdates) == 0 && len(pendingInserts) == 0 {
		return inserted, updated, nil
	}

	tx, err := db.Begin()
	if err != nil {
		return 0, 0, err
	}
	defer func() { _ = tx.Rollback() }()

	for _, u := range pendingUpdates {
		var expArg any
		if u.item.ExpireAt != nil {
			expArg = *u.item.ExpireAt
		}
		if _, err := tx.Exec(
			"UPDATE deals SET title = ?1, discount_desc = ?2, min_spend = ?3, deal_url = ?4, expire_at = ?5, is_active = 1, is_exclusive = ?6 WHERE id = ?7",
			u.item.Title, u.item.DiscountDesc, u.item.MinSpend, u.item.DealURL, expArg, u.exclFlag, u.id,
		); err != nil {
			return 0, 0, err
		}
	}

	for _, item := range pendingInserts {
		exclFlag := int64(0)
		if item.IsExclusive {
			exclFlag = 1
		}
		var couponArg any
		if item.CouponCode != nil {
			couponArg = *item.CouponCode
		}
		var expArg any
		if item.ExpireAt != nil {
			expArg = *item.ExpireAt
		}
		if _, err := tx.Exec(
			`INSERT INTO deals (platform, title, coupon_code, discount_desc, min_spend, deal_url, expire_at, is_active, is_exclusive, created_at)
			 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9)`,
			item.Platform, item.Title, couponArg, item.DiscountDesc, item.MinSpend, item.DealURL, expArg, exclFlag, now,
		); err != nil {
			return 0, 0, err
		}
	}

	if err := tx.Commit(); err != nil {
		return 0, 0, err
	}
	return inserted, updated, nil
}

// ToggleDeal đảo trạng thái bật/tắt deal.
func ToggleDeal(db *sql.DB, dealID int64) error {
	_, err := db.Exec("UPDATE deals SET is_active = CASE WHEN is_active = 1 THEN 0 ELSE 1 END WHERE id = ?1", dealID)
	return err
}

// DeleteDeal xóa deal theo ID.
func DeleteDeal(db *sql.DB, dealID int64) error {
	_, err := db.Exec("DELETE FROM deals WHERE id = ?1", dealID)
	return err
}

// GetRecentClicksForAdmin lấy danh sách lượt dán link gần nhất.
func GetRecentClicksForAdmin(db *sql.DB, limit int64) ([]ClickRecord, error) {
	rows, err := db.Query(
		"SELECT id, user_id, platform, original_url, clean_url, affiliate_url, sub_id, created_at FROM clicks ORDER BY created_at DESC LIMIT ?1",
		limit,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var list []ClickRecord
	for rows.Next() {
		var c ClickRecord
		if err := rows.Scan(&c.ID, &c.UserID, &c.Platform, &c.OriginalURL, &c.CleanURL, &c.AffiliateURL, &c.SubID, &c.CreatedAt); err != nil {
			return nil, err
		}
		list = append(list, c)
	}
	return list, rows.Err()
}

// GetSetting đọc giá trị 1 key từ bảng settings.
func GetSetting(db *sql.DB, key string) (*string, error) {
	var val string
	err := db.QueryRow("SELECT value FROM settings WHERE key = ?1", key).Scan(&val)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return &val, nil
}

// GetSettingsMap đọc toàn bộ cấu hình nhẹ trong 1 query (bỏ qua market_vouchers_cache_json).
func GetSettingsMap(db *sql.DB) (map[string]string, error) {
	rows, err := db.Query("SELECT key, value FROM settings WHERE key != 'market_vouchers_cache_json'")
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	res := make(map[string]string)
	for rows.Next() {
		var k, v string
		if err := rows.Scan(&k, &v); err != nil {
			return nil, err
		}
		res[k] = v
	}
	return res, rows.Err()
}

// SetSetting lưu/ghi đè 1 key vào bảng settings.
func SetSetting(db *sql.DB, key, value string) error {
	_, err := db.Exec(
		"INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
		key, value,
	)
	return err
}

// CleanupExpiredAdminSessions xóa các phiên đăng nhập Admin đã hết hạn.
func CleanupExpiredAdminSessions(db *sql.DB, nowTS int64) (int64, error) {
	res, err := db.Exec("DELETE FROM admin_sessions WHERE expires_at <= ?1", nowTS)
	if err != nil {
		return 0, err
	}
	aff, _ := res.RowsAffected()
	return aff, nil
}

// IsValidShortCode kiểm tra mã bọc link 6-12 ký tự ASCII chữ/số.
func IsValidShortCode(code string) bool {
	if len(code) < 6 || len(code) > 12 {
		return false
	}
	for i := 0; i < len(code); i++ {
		b := code[i]
		if !((b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || (b >= '0' && b <= '9')) {
			return false
		}
	}
	return true
}

// GetShortCodesByTargets tra cứu hàng loạt short code theo chunk 40 URL.
func GetShortCodesByTargets(db *sql.DB, targets []string) (map[string]string, error) {
	found := make(map[string]string)
	if len(targets) == 0 {
		return found, nil
	}
	const chunkSize = 40
	for start := 0; start < len(targets); start += chunkSize {
		end := start + chunkSize
		if end > len(targets) {
			end = len(targets)
		}
		chunk := targets[start:end]
		placeholders := make([]string, len(chunk))
		args := make([]any, len(chunk))
		for i, u := range chunk {
			placeholders[i] = fmt.Sprintf("?%d", i+1)
			args[i] = u
		}
		q := fmt.Sprintf("SELECT target_url, code FROM short_links WHERE target_url IN (%s)", strings.Join(placeholders, ","))
		rows, err := db.Query(q, args...)
		if err != nil {
			return found, err
		}
		for rows.Next() {
			var targetURL, code string
			if err := rows.Scan(&targetURL, &code); err == nil {
				if _, exists := found[targetURL]; !exists {
					found[targetURL] = code
				}
			}
		}
		_ = rows.Close()
	}
	return found, nil
}

// CreateOrGetShortLink tạo mới hoặc tái sử dụng mã 8 ký tự cho target_url.
func CreateOrGetShortLink(db *sql.DB, targetURL, title, platform string) (string, error) {
	var existing string
	if err := db.QueryRow("SELECT code FROM short_links WHERE target_url = ?1 LIMIT 1", targetURL).Scan(&existing); err == nil && existing != "" {
		return existing, nil
	}

	now := time.Now().UTC().Format(time.RFC3339)
	for i := 0; i < 5; i++ {
		code := generateShortCode()
		if _, err := db.Exec(
			"INSERT INTO short_links (code, target_url, title, platform, clicks_count, created_at) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
			code, targetURL, title, platform, now,
		); err == nil {
			return code, nil
		}
	}
	return "", errors.New("không thể tạo mã shortlink duy nhất sau 5 lần thử")
}

// ResolveShortLink tra cứu thông tin link gốc từ mã shortlink.
func ResolveShortLink(db *sql.DB, code string) (*ShortLinkRecord, error) {
	var rec ShortLinkRecord
	err := db.QueryRow(
		"SELECT code, target_url, title, platform FROM short_links WHERE code = ?1 LIMIT 1",
		code,
	).Scan(&rec.Code, &rec.TargetURL, &rec.Title, &rec.Platform)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return &rec, nil
}

// IncrementShortLinkClick tăng bộ đếm clicks_count của shortlink.
func IncrementShortLinkClick(db *sql.DB, code string) error {
	_, err := db.Exec("UPDATE short_links SET clicks_count = clicks_count + 1 WHERE code = ?1", code)
	return err
}

// IncrementShortLinkClicksBatch cộng dồn clicks_count cho nhiều shortlink trong 1 transaction duy nhất.
func IncrementShortLinkClicksBatch(db *sql.DB, counts map[string]int64) error {
	if len(counts) == 0 {
		return nil
	}
	tx, err := db.Begin()
	if err != nil {
		return err
	}
	stmt, err := tx.Prepare("UPDATE short_links SET clicks_count = clicks_count + ?1 WHERE code = ?2")
	if err != nil {
		_ = tx.Rollback()
		return err
	}
	defer func() { _ = stmt.Close() }()

	for code, delta := range counts {
		if delta <= 0 || !IsValidShortCode(code) {
			continue
		}
		if _, err := stmt.Exec(delta, code); err != nil {
			_ = tx.Rollback()
			return err
		}
	}
	return tx.Commit()
}

// WrapURLIfPublic bọc URL nếu baseURLOpt là http(s):// hợp lệ (dùng trực tiếp DB không cache).
func WrapURLIfPublic(db *sql.DB, baseURLOpt, targetURL, title, platform string) string {
	trimmed := strings.TrimSpace(targetURL)
	base := strings.TrimRight(strings.TrimSpace(baseURLOpt), "/")
	if !strings.HasPrefix(base, "https://") && !strings.HasPrefix(base, "http://") {
		return trimmed
	}
	if trimmed == "" || strings.HasPrefix(trimmed, base) {
		return trimmed
	}
	code, err := CreateOrGetShortLink(db, trimmed, strings.TrimSpace(title), strings.TrimSpace(platform))
	if err != nil {
		return trimmed
	}
	return base + "/" + code
}

// CreateAdminSession lưu hash SHA-256 của session token cùng thời hạn hết hiệu lực.
func CreateAdminSession(db *sql.DB, rawToken string, ttlSecs int64) error {
	nowTS := time.Now().UTC().Unix()
	if ttlSecs < 60 {
		ttlSecs = 60
	}
	expiresAt := nowTS + ttlSecs
	tokenHash := sha256HexLower([]byte(rawToken))
	createdAt := time.Now().UTC().Format(time.RFC3339)

	_, _ = db.Exec("DELETE FROM admin_sessions WHERE expires_at <= ?1", nowTS)
	_, _ = db.Exec("DELETE FROM admin_sessions WHERE token_hash NOT IN (SELECT token_hash FROM admin_sessions ORDER BY expires_at DESC LIMIT 99)")
	_, err := db.Exec(
		`INSERT INTO admin_sessions (token_hash, expires_at, created_at)
		 VALUES (?1, ?2, ?3)
		 ON CONFLICT(token_hash) DO UPDATE SET expires_at = ?2`,
		tokenHash, expiresAt, createdAt,
	)
	return err
}

// VerifyAdminSession xác thực session token chưa hết hạn trong DB.
func VerifyAdminSession(db *sql.DB, rawToken string) (bool, error) {
	nowTS := time.Now().UTC().Unix()
	tokenHash := sha256HexLower([]byte(rawToken))
	var expiresAt int64
	err := db.QueryRow("SELECT expires_at FROM admin_sessions WHERE token_hash = ?1 LIMIT 1", tokenHash).Scan(&expiresAt)
	if errors.Is(err, sql.ErrNoRows) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	return expiresAt > nowTS, nil
}

// DeleteAdminSession xóa session khỏi DB khi đăng xuất.
func DeleteAdminSession(db *sql.DB, rawToken string) error {
	tokenHash := sha256HexLower([]byte(rawToken))
	_, err := db.Exec("DELETE FROM admin_sessions WHERE token_hash = ?1", tokenHash)
	return err
}

// UpsertTrackedOrder lưu hoặc cập nhật đơn hàng đối soát; trả true nếu đơn mới hoàn toàn.
func UpsertTrackedOrder(db *sql.DB, order TrackedOrder) (bool, error) {
	var oldStatus string
	err := db.QueryRow("SELECT status FROM tracked_orders WHERE order_key = ?1 LIMIT 1", order.OrderKey).Scan(&oldStatus)
	if err == nil {
		_, err = db.Exec(
			"UPDATE tracked_orders SET status = ?1, order_amount = ?2, commission = ?3 WHERE order_key = ?4",
			order.Status, order.OrderAmount, order.Commission, order.OrderKey,
		)
		return false, err
	}
	_, err = db.Exec(
		`INSERT INTO tracked_orders (order_key, platform, order_id, status, order_amount, commission, sub_id, order_time, notified_at)
		 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)`,
		order.OrderKey, order.Platform, order.OrderID, order.Status, order.OrderAmount, order.Commission, order.SubID, order.OrderTime, order.NotifiedAt,
	)
	return true, err
}

// GetRecentTrackedOrders lấy danh sách đơn hàng đã đối soát gần đây.
func GetRecentTrackedOrders(db *sql.DB, limit int64) ([]TrackedOrder, error) {
	rows, err := db.Query(
		`SELECT order_key, platform, order_id, status, order_amount, commission, sub_id, order_time, notified_at
		 FROM tracked_orders ORDER BY notified_at DESC LIMIT ?1`,
		limit,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var list []TrackedOrder
	for rows.Next() {
		var o TrackedOrder
		if err := rows.Scan(&o.OrderKey, &o.Platform, &o.OrderID, &o.Status, &o.OrderAmount, &o.Commission, &o.SubID, &o.OrderTime, &o.NotifiedAt); err != nil {
			return nil, err
		}
		list = append(list, o)
	}
	return list, rows.Err()
}

// GetLoginRateLimit đọc trạng thái brute-force bền vững của 1 IP từ DB (đồng bộ qua restart & multi-instance).
func GetLoginRateLimit(db *sql.DB, ip string) (uint32, int64, error) {
	var attempts int64
	var lastAttempt int64
	err := db.QueryRow(
		"SELECT attempts, last_attempt FROM login_rate_limits WHERE ip = ?1 LIMIT 1",
		ip,
	).Scan(&attempts, &lastAttempt)
	if errors.Is(err, sql.ErrNoRows) {
		return 0, 0, nil
	}
	if err != nil {
		return 0, 0, err
	}
	if attempts < 0 {
		attempts = 0
	}
	return uint32(attempts), lastAttempt, nil
}

// RecordLoginFailure tăng bộ đếm đăng nhập sai cho IP theo cửa sổ windowSecs (atomic UPSERT).
func RecordLoginFailure(db *sql.DB, ip string, nowTS, windowSecs int64) (uint32, error) {
	_, err := db.Exec(
		`INSERT INTO login_rate_limits (ip, attempts, last_attempt)
		 VALUES (?1, 1, ?2)
		 ON CONFLICT(ip) DO UPDATE SET
		   attempts = CASE WHEN (?2 - login_rate_limits.last_attempt) >= ?3 THEN 1 ELSE login_rate_limits.attempts + 1 END,
		   last_attempt = ?2`,
		ip, nowTS, windowSecs,
	)
	if err != nil {
		return 0, err
	}
	attempts, _, err := GetLoginRateLimit(db, ip)
	return attempts, err
}

// ClearLoginRateLimit xóa trạng thái khóa của IP sau khi đăng nhập thành công.
func ClearLoginRateLimit(db *sql.DB, ip string) error {
	_, err := db.Exec("DELETE FROM login_rate_limits WHERE ip = ?1", ip)
	return err
}

// CleanupExpiredLoginRateLimits dọn các bản ghi khóa đăng nhập đã hết cửa sổ theo dõi.
func CleanupExpiredLoginRateLimits(db *sql.DB, cutoffTS int64) (int64, error) {
	res, err := db.Exec("DELETE FROM login_rate_limits WHERE last_attempt <= ?1", cutoffTS)
	if err != nil {
		return 0, err
	}
	return res.RowsAffected()
}
