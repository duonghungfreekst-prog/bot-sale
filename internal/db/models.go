package db

// User đại diện bảng users.
type User struct {
	TelegramID       int64
	Username         *string
	FullName         string
	ReferralCode     string
	ReferredBy       *int64
	BalanceAvailable float64
	BalancePending   float64
	BalanceWithdrawn float64
	CreatedAt        string
}

// ClickRecord lưu thông tin mỗi lượt khách hàng dán link.
type ClickRecord struct {
	ID           string
	UserID       int64
	Platform     string
	OriginalURL  string
	CleanURL     string
	AffiliateURL string
	SubID        string
	CreatedAt    string
}

// CashbackOrder lưu giao dịch hoàn tiền nội bộ.
type CashbackOrder struct {
	ID              string
	UserID          int64
	OrderID         string
	Platform        string
	OrderAmount     float64
	TotalCommission float64
	UserCashback    float64
	ReferrerBonus   float64
	ReferrerID      *int64
	Status          string
	CreatedAt       string
	UpdatedAt       string
}

// Withdrawal lưu yêu cầu rút tiền.
type Withdrawal struct {
	ID            string
	UserID        int64
	Amount        float64
	BankName      string
	AccountNumber string
	AccountName   string
	Status        string
	CreatedAt     string
}

// DealItem lưu thông tin voucher / deal trên bảng deals.
type DealItem struct {
	ID           int64
	Platform     string
	Title        string
	CouponCode   *string
	DiscountDesc string
	MinSpend     float64
	DealURL      string
	ExpireAt     *string
	IsActive     bool
	IsExclusive  bool
}

// NewDealInput dữ liệu thêm hoặc cập nhật deal.
type NewDealInput struct {
	Platform     string
	Title        string
	CouponCode   *string
	DiscountDesc string
	MinSpend     float64
	DealURL      string
	ExpireAt     *string
	IsExclusive  bool
}

// WalletSummary tóm tắt ví hoàn tiền của user.
type WalletSummary struct {
	BalanceAvailable float64
	BalancePending   float64
	BalanceWithdrawn float64
	TotalEarned      float64
	ReferralCode     string
	TotalReferrals   int64
	TotalRefBonus    float64
}

// AdminStats số liệu tổng quan cho Dashboard.
type AdminStats struct {
	TotalUsers   int64
	TotalClicks  int64
	TotalDeals   int64
	ShopeeClicks int64
	LazadaClicks int64
	TikiClicks   int64
	TikTokClicks int64
}

// ShortLinkRecord lưu mã bọc link rút gọn 8 ký tự.
type ShortLinkRecord struct {
	Code      string
	TargetURL string
	Title     string
	Platform  string
}

// TrackedOrder lưu đơn hàng đối soát từ AccessTrade / Shopee Open API.
type TrackedOrder struct {
	OrderKey    string
	Platform    string
	OrderID     string
	Status      string
	OrderAmount float64
	Commission  float64
	SubID       string
	OrderTime   string
	NotifiedAt  string
}
