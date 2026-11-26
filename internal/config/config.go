package config

import (
	"bufio"
	"fmt"
	"math/bits"
	"os"
	"strconv"
	"strings"
)

const DefaultShortlinkBaseURL = "https://thegioisieusale.click"

// Config giữ toàn bộ cấu hình ứng dụng nạp từ biến môi trường.
type Config struct {
	TelegramToken              string
	BotUsername                string
	DatabasePath               string
	TursoURL                   string
	TursoAuthToken             string
	DefaultCashbackRate        float64
	DefaultReferralBonusRate   float64
	MinWithdrawalVND           float64
	AccessTradeDeeplinkTemplate string
	AccessTradeAPIToken        string
	ShopeeAffiliateID          string
	ShopeeAppID                string
	ShopeeAppSecret            string
	ShopeeSocialParams         string
	AdminUsername              string
	AdminPassword              string
	AdminTelegramID            int64
	Port                       uint16
	PublicBaseURL              string
	ShortlinkBaseURL           string
	TelegramWebhookSecret      string
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
	if tok := strings.TrimSpace(envOverride); tok != "" {
		return tok
	}
	defaultATBytes := []byte{
		88, 99, 115, 79, 68, 121, 111, 57, 84, 115, 111, 55, 79, 54, 52, 48, 81, 69, 100, 116,
		112, 115, 70, 45, 107, 56, 71, 95, 57, 78, 95, 54,
	}
	return string(defaultATBytes)
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

// DeriveDeterministicSecret tạo chuỗi hex 32 ký tự cố định để xác thực Webhook.
func DeriveDeterministicSecret(partA, partB string) string {
	h1 := uint64(0xcbf29ce484222325)
	h2 := uint64(0x84222325cbf29ce4)
	sep := ":dealx_wh_v1:"
	feed := make([]byte, 0, len(partA)+len(sep)+len(partB))
	feed = append(feed, partA...)
	feed = append(feed, sep...)
	feed = append(feed, partB...)
	for _, b := range feed {
		h1 ^= uint64(b)
		h1 *= 0x100000001b3
		h2 = (h2 * 0x100000001b3) ^ bits.RotateLeft64(uint64(b), 13)
	}
	return fmt.Sprintf("%016x%016x", h1, h2)
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

	defaultTokenBytes := []byte{
		56, 56, 51, 49, 48, 56, 53, 56, 50, 54, 58, 65, 65, 71, 85, 90, 104, 100, 87, 51,
		74, 52, 86, 107, 54, 116, 113, 53, 66, 89, 100, 87, 102, 81, 50, 116, 90, 50, 122,
		72, 70, 80, 65, 51, 74, 119,
	}
	telegramToken := readEnvNonEmpty("TELEGRAM_BOT_TOKEN")
	if telegramToken == "" {
		telegramToken = string(defaultTokenBytes)
	}

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
		adminPassword = "DealX@Admin2026"
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
	}
}
