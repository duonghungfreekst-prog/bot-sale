package config

import (
	"bufio"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"strconv"
	"strings"
)

const (
	DefaultShortlinkBaseURL = "https://thegioisieusale.click"
	DefaultAdminPassword    = "DealX@Admin2026"
	MinCloudAdminPassLength = 12
)

// Config giữ toàn bộ cấu hình ứng dụng nạp từ biến môi trường.
type Config struct {
	TelegramToken               string
	BotUsername                 string
	DatabasePath                string
	TursoURL                    string
	TursoAuthToken              string
	DefaultCashbackRate         float64
	DefaultReferralBonusRate    float64
	MinWithdrawalVND            float64
	AccessTradeDeeplinkTemplate string
	AccessTradeAPIToken         string
	ShopeeAffiliateID           string
	ShopeeAppID                 string
	ShopeeAppSecret             string
	ShopeeSocialParams          string
	AdminUsername               string
	AdminPassword               string
	AdminTelegramID             int64
	Port                        uint16
	PublicBaseURL               string
	ShortlinkBaseURL            string
	TelegramWebhookSecret       string
	TrustCloudflareHeaders      bool
	AppSecretKey                string
}

func loadDotEnvIfExists() {
	f, err := os.Open(".env")
	if err != nil {
		return
	}
	defer f.Close()

	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		line = strings.TrimPrefix(line, "export ")
		k, v, ok := strings.Cut(line, "=")
		if !ok {
			continue
		}
		key := strings.TrimSpace(k)
		if key == "" {
			continue
		}
		if _, exists := os.LookupEnv(key); exists {
			continue
		}
		val := strings.TrimSpace(v)
		if len(val) >= 2 && ((val[0] == '"' && val[len(val)-1] == '"') || (val[0] == '\'' && val[len(val)-1] == '\'')) {
			val = val[1 : len(val)-1]
		}
		_ = os.Setenv(key, val)
	}
}

func readEnvNonEmpty(key string) string {
	return strings.TrimSpace(os.Getenv(key))
}

func resolveAccessTradeAPIToken(envOverride string) string {
	return strings.TrimSpace(envOverride)
}

// IsShortlinkDisabledValue kiểm tra giá trị có phải lệnh tắt bọc link hay không.
func IsShortlinkDisabledValue(raw string) bool {
	switch strings.ToLower(strings.TrimSpace(raw)) {
	case "off", "none", "false", "direct":
		return true
	default:
		return false
	}
}

func resolveShortlinkBaseURL(envOverride string) string {
	trimmed := strings.TrimRight(strings.TrimSpace(envOverride), "/")
	if trimmed != "" {
		if IsShortlinkDisabledValue(trimmed) {
			return ""
		}
		if strings.HasPrefix(trimmed, "https://") || strings.HasPrefix(trimmed, "http://") {
			return trimmed
		}
	}
	return DefaultShortlinkBaseURL
}

// ResolveEffectiveShortlinkBaseURL ưu tiên DB setting -> Config -> PublicBaseURL.
func ResolveEffectiveShortlinkBaseURL(dbSetting, configShortlink, publicBase string) string {
	if trimmed := strings.TrimRight(strings.TrimSpace(dbSetting), "/"); trimmed != "" {
		if IsShortlinkDisabledValue(trimmed) {
			return ""
		}
		if strings.HasPrefix(trimmed, "https://") || strings.HasPrefix(trimmed, "http://") {
			return trimmed
		}
	}
	if s := strings.TrimRight(strings.TrimSpace(configShortlink), "/"); strings.HasPrefix(s, "https://") || strings.HasPrefix(s, "http://") {
		return s
	}
	if s := strings.TrimRight(strings.TrimSpace(publicBase), "/"); strings.HasPrefix(s, "https://") || strings.HasPrefix(s, "http://") {
		return s
	}
	return ""
}

// ResolvePCRuntimeShortlinkBaseURL giải quyết base URL bọc link hiệu lực trên PC/Hosting.
func ResolvePCRuntimeShortlinkBaseURL(dbSetting, configShortlink, publicBase, quickTunnelURL string, hasNamedTunnel bool) string {
	effective := ResolveEffectiveShortlinkBaseURL(dbSetting, configShortlink, publicBase)
	if effective == "" {
		return ""
	}
	if strings.TrimSpace(publicBase) == "" && !hasNamedTunnel && strings.EqualFold(effective, DefaultShortlinkBaseURL) {
		qt := strings.TrimRight(strings.TrimSpace(quickTunnelURL), "/")
		if strings.HasPrefix(qt, "https://") || strings.HasPrefix(qt, "http://") {
			return qt
		}
		return ""
	}
	return effective
}

func resolvePublicBaseURL() string {
	for _, k := range []string{"WEBHOOK_BASE_URL", "PUBLIC_BASE_URL", "RENDER_EXTERNAL_URL"} {
		if raw := readEnvNonEmpty(k); raw != "" {
			norm := strings.TrimRight(raw, "/")
			if strings.HasPrefix(norm, "https://") {
				return norm
			}
		}
	}
	if domain := readEnvNonEmpty("RAILWAY_PUBLIC_DOMAIN"); domain != "" {
		trimmed := strings.TrimRight(strings.TrimSpace(domain), "/")
		if strings.HasPrefix(trimmed, "https://") {
			return trimmed
		}
		if trimmed != "" && !strings.HasPrefix(trimmed, "http://") {
			return "https://" + trimmed
		}
	}
	return ""
}

func hasCloudPlatformEnv() bool {
	return readEnvNonEmpty("RENDER") != "" ||
		readEnvNonEmpty("RAILWAY_ENVIRONMENT") != "" ||
		readEnvNonEmpty("FLY_APP_NAME") != ""
}

// IsCloudRuntime trả về true khi đang chạy trên Hosting/Cloud thay vì PC Local.
func (c *Config) IsCloudRuntime() bool {
	return strings.TrimSpace(c.PublicBaseURL) != "" || hasCloudPlatformEnv()
}

func isValidTelegramSecret(secret string) bool {
	if secret == "" || len(secret) > 256 {
		return false
	}
	for i := 0; i < len(secret); i++ {
		b := secret[i]
		isValid := (b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || (b >= '0' && b <= '9') || b == '_' || b == '-'
		if !isValid {
			return false
		}
	}
	return true
}

// ValidateCloudSecurity áp dụng chính sách Fail-Closed cho môi trường Cloud/Hosting:
// từ chối khởi động nếu quên đặt ADMIN_PASSWORD, vẫn dùng mật khẩu mặc định hoặc mật khẩu ngắn hơn 12 ký tự.
func (c *Config) ValidateCloudSecurity() error {
	if !c.IsCloudRuntime() {
		return nil
	}
	pass := strings.TrimSpace(c.AdminPassword)
	if pass == "" || pass == DefaultAdminPassword {
		return errors.New("chế độ Cloud/Hosting từ chối khởi động với ADMIN_PASSWORD mặc định — hãy cấu hình biến môi trường ADMIN_PASSWORD mạnh (>= 12 ký tự)")
	}
	if len(pass) < MinCloudAdminPassLength {
		return fmt.Errorf("chế độ Cloud/Hosting yêu cầu ADMIN_PASSWORD dài tối thiểu %d ký tự (hiện tại %d ký tự)", MinCloudAdminPassLength, len(pass))
	}
	return nil
}

// SettingsEncryptionSeed trả về nguồn khóa mã hóa AES-256-GCM cho secrets trong bảng settings:
// ưu tiên APP_SECRET_KEY (để giữ khóa cố định kể cả khi đổi mật khẩu Admin), nếu trống dùng AdminPassword.
func (c *Config) SettingsEncryptionSeed() string {
	if s := strings.TrimSpace(c.AppSecretKey); s != "" {
		return s
	}
	return c.AdminPassword
}

func readEnvTruthy(key string) bool {
	switch strings.ToLower(readEnvNonEmpty(key)) {
	case "1", "true", "yes", "on":
		return true
	default:
		return false
	}
}

// DeriveDeterministicSecret tạo chuỗi hex 32 ký tự chuẩn mật mã SHA-256 cố định để xác thực Webhook.
func DeriveDeterministicSecret(partA, partB string) string {
	h := sha256.New()
	_, _ = h.Write([]byte("dealx_wh_v2:"))
	_, _ = h.Write([]byte(partA))
	_, _ = h.Write([]byte{0x00})
	_, _ = h.Write([]byte(partB))
	sum := h.Sum(nil)
	return hex.EncodeToString(sum[:16])
}

func resolveWebhookSecret(telegramToken, adminPassword string) string {
	if envSecret := readEnvNonEmpty("TELEGRAM_WEBHOOK_SECRET"); isValidTelegramSecret(envSecret) {
		return envSecret
	}
	return DeriveDeterministicSecret(telegramToken, adminPassword)
}

// FromEnv nạp toàn bộ cấu hình ứng dụng từ .env và biến môi trường.
func FromEnv() Config {
	loadDotEnvIfExists()

	telegramToken := readEnvNonEmpty("TELEGRAM_BOT_TOKEN")

	botUsername := readEnvNonEmpty("BOT_USERNAME")
	if botUsername == "" {
		botUsername = "vvipass_vip_bot"
	}

	databasePath := readEnvNonEmpty("DATABASE_PATH")
	if databasePath == "" {
		databasePath = "salehunter.db"
	}

	cashbackRate := 70.0
	if raw, err := strconv.ParseFloat(os.Getenv("CASHBACK_RATE"), 64); err == nil {
		cashbackRate = raw
	}
	refBonusRate := 10.0
	if raw, err := strconv.ParseFloat(os.Getenv("REFERRAL_BONUS_RATE"), 64); err == nil {
		refBonusRate = raw
	}
	minWithdraw := 50000.0
	if raw, err := strconv.ParseFloat(os.Getenv("MIN_WITHDRAWAL_VND"), 64); err == nil {
		minWithdraw = raw
	}

	adminUsername := readEnvNonEmpty("ADMIN_USERNAME")
	if adminUsername == "" {
		adminUsername = "admin"
	}
	adminPassword := readEnvNonEmpty("ADMIN_PASSWORD")
	if adminPassword == "" {
		adminPassword = DefaultAdminPassword
	}

	var adminTelegramID int64
	if raw := readEnvNonEmpty("ADMIN_TELEGRAM_ID"); raw != "" {
		if v, err := strconv.ParseInt(raw, 10, 64); err == nil && v != 0 {
			adminTelegramID = v
		}
	}

	port := uint16(10000)
	if raw, err := strconv.ParseUint(os.Getenv("PORT"), 10, 16); err == nil && raw > 0 {
		port = uint16(raw)
	}

	publicBase := resolvePublicBaseURL()
	shortlinkBase := resolveShortlinkBaseURL(readEnvNonEmpty("SHORTLINK_BASE_URL"))
	webhookSecret := resolveWebhookSecret(telegramToken, adminPassword)

	return Config{
		TelegramToken:               telegramToken,
		BotUsername:                 botUsername,
		DatabasePath:                databasePath,
		TursoURL:                    readEnvNonEmpty("TURSO_DATABASE_URL"),
		TursoAuthToken:              readEnvNonEmpty("TURSO_AUTH_TOKEN"),
		DefaultCashbackRate:         cashbackRate,
		DefaultReferralBonusRate:    refBonusRate,
		MinWithdrawalVND:            minWithdraw,
		AccessTradeDeeplinkTemplate: readEnvNonEmpty("ACCESSTRADE_DEEPLINK_TEMPLATE"),
		AccessTradeAPIToken:         resolveAccessTradeAPIToken(readEnvNonEmpty("ACCESSTRADE_API_TOKEN")),
		ShopeeAffiliateID:           readEnvNonEmpty("SHOPEE_AFFILIATE_ID"),
		ShopeeAppID:                 readEnvNonEmpty("SHOPEE_APP_ID"),
		ShopeeAppSecret:             readEnvNonEmpty("SHOPEE_APP_SECRET"),
		ShopeeSocialParams:          readEnvNonEmpty("SHOPEE_SOCIAL_PARAMS"),
		AdminUsername:               adminUsername,
		AdminPassword:               adminPassword,
		AdminTelegramID:             adminTelegramID,
		Port:                        port,
		PublicBaseURL:               publicBase,
		ShortlinkBaseURL:            shortlinkBase,
		TelegramWebhookSecret:       webhookSecret,
		TrustCloudflareHeaders:      readEnvTruthy("TRUST_CLOUDFLARE_HEADERS"),
		AppSecretKey:                readEnvNonEmpty("APP_SECRET_KEY"),
	}
}
