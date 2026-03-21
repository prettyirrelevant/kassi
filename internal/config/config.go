package config

import (
	"fmt"
	"strings"
	"time"

	validation "github.com/go-ozzo/ozzo-validation/v4"
	"github.com/knadh/koanf/providers/env"
	"github.com/knadh/koanf/v2"
)

type Config struct {
	DatabaseURL string        `koanf:"database_url"`
	RedisURL    string        `koanf:"redis_url"`
	Port        string        `koanf:"port"`
	Deployment  string        `koanf:"deployment"`
	JWTSecret   string        `koanf:"session_jwt_secret"`
	JWTExpiry   time.Duration `koanf:"session_jwt_expiry"`

	InfisicalClientID     string `koanf:"infisical_client_id"`
	InfisicalClientSecret string `koanf:"infisical_client_secret"`
	InfisicalProjectID    string `koanf:"infisical_project_id"`

	QuoteLockDuration time.Duration `koanf:"quote_lock_duration"`
	PriceCacheTTL     time.Duration `koanf:"price_cache_ttl"`

	FeeBPS             uint64 `koanf:"fee_bps"`
	FeeMaxCents        uint64 `koanf:"fee_max_cents"`
	FeeRecipientEVM    string `koanf:"fee_recipient_evm"`
	FeeRecipientSolana string `koanf:"fee_recipient_solana"`

	AlchemyWebhookSigningKey string `koanf:"alchemy_webhook_signing_key"`
	AlchemyAuthToken         string `koanf:"alchemy_auth_token"`
	AlchemyWebhookPath       string `koanf:"alchemy_webhook_path"`

	TelegramBotToken string `koanf:"telegram_bot_token"`
	TelegramChatID   string `koanf:"telegram_chat_id"`
}

func Load() (*Config, error) {
	k := koanf.New(".")

	if err := k.Load(env.Provider("", ".", func(s string) string {
		return strings.ToLower(s)
	}), nil); err != nil {
		return nil, fmt.Errorf("loading env vars: %w", err)
	}

	cfg := &Config{}
	if err := k.Unmarshal("", cfg); err != nil {
		return nil, fmt.Errorf("unmarshalling config: %w", err)
	}

	if err := cfg.Validate(); err != nil {
		return nil, fmt.Errorf("validating config: %w", err)
	}

	return cfg, nil
}

func (c *Config) Validate() error {
	return validation.ValidateStruct(c,
		validation.Field(&c.DatabaseURL, validation.Required),
		validation.Field(&c.RedisURL, validation.Required),
		validation.Field(&c.Port, validation.Required),
		validation.Field(&c.Deployment, validation.Required, validation.In("test", "live")),
		validation.Field(&c.JWTSecret, validation.Required),
		validation.Field(&c.JWTExpiry, validation.Required),
		validation.Field(&c.InfisicalClientID, validation.Required),
		validation.Field(&c.InfisicalClientSecret, validation.Required),
		validation.Field(&c.InfisicalProjectID, validation.Required),
		validation.Field(&c.QuoteLockDuration, validation.Required),
		validation.Field(&c.PriceCacheTTL, validation.Required),
	)
}

func (c *Config) PublicKeyPrefix() string {
	return c.Deployment + "_pub_"
}

func (c *Config) SecretKeyPrefix() string {
	return c.Deployment + "_sec_"
}
