package signer

import (
	"context"
	"encoding/base64"
	"fmt"
	"sync"
	"sync/atomic"
	"time"

	"github.com/imroc/req/v3"
	"golang.org/x/sync/singleflight"
)

type cachedToken struct {
	value     string
	expiresAt time.Time
}

type InfisicalKMS struct {
	clientID     string
	clientSecret string
	projectID    string
	client       *req.Client
	token        atomic.Value   // *cachedToken
	tokenFlight  singleflight.Group
	keys         sync.Map       // name -> keyId (string)
}

func NewInfisicalKMS(clientID, clientSecret, projectID string) *InfisicalKMS {
	kms := &InfisicalKMS{
		clientID:     clientID,
		clientSecret: clientSecret,
		projectID:    projectID,
	}

	kms.client = req.C().
		SetBaseURL("https://app.infisical.com/api").
		SetTimeout(10 * time.Second).
		SetUserAgent("kassi").
		OnBeforeRequest(kms.ensureAuth)

	return kms
}

func (k *InfisicalKMS) ensureAuth(_ *req.Client, r *req.Request) error {
	if r.RawURL == "/v1/auth/universal-auth/login" {
		return nil
	}

	if v, ok := k.token.Load().(*cachedToken); ok && time.Now().Before(v.expiresAt) {
		r.SetBearerAuthToken(v.value)
		return nil
	}

	result, err, _ := k.tokenFlight.Do("token", func() (any, error) {
		if v, ok := k.token.Load().(*cachedToken); ok && time.Now().Before(v.expiresAt) {
			return v.value, nil
		}

		var authResp struct {
			AccessToken string `json:"accessToken"` //nolint:gosec
			ExpiresIn   int64  `json:"expiresIn"`
		}

		resp, err := k.client.R().
			SetContext(r.Context()).
			SetBodyJsonMarshal(map[string]string{
				"clientId":     k.clientID,
				"clientSecret": k.clientSecret,
			}).
			SetSuccessResult(&authResp).
			Post("/v1/auth/universal-auth/login")
		if err != nil {
			return nil, fmt.Errorf("authenticating with infisical: %w", err)
		}
		if !resp.IsSuccessState() {
			return nil, fmt.Errorf("infisical auth failed (status %d): %s", resp.StatusCode, resp.String())
		}

		k.token.Store(&cachedToken{
			value:     authResp.AccessToken,
			expiresAt: time.Now().Add(time.Duration(authResp.ExpiresIn)*time.Second - 30*time.Second),
		})

		return authResp.AccessToken, nil
	})
	if err != nil {
		return err
	}

	r.SetBearerAuthToken(result.(string))
	return nil
}

func (k *InfisicalKMS) resolveKeyID(ctx context.Context, name string) (string, error) {
	if id, ok := k.keys.Load(name); ok {
		return id.(string), nil
	}

	var result struct {
		Key struct {
			ID string `json:"id"`
		} `json:"key"`
	}

	resp, err := k.client.R().
		SetContext(ctx).
		SetPathParam("keyName", name).
		SetQueryParam("projectId", k.projectID).
		SetSuccessResult(&result).
		Get("/v1/kms/keys/key-name/{keyName}")
	if err != nil {
		return "", fmt.Errorf("resolving KMS key %s: %w", name, err)
	}
	if !resp.IsSuccessState() {
		return "", fmt.Errorf("infisical get key failed (status %d): %s", resp.StatusCode, resp.String())
	}

	k.keys.Store(name, result.Key.ID)
	return result.Key.ID, nil
}

func (k *InfisicalKMS) CreateKey(ctx context.Context, name string) error {
	var result struct {
		Key struct {
			ID string `json:"id"`
		} `json:"key"`
	}

	resp, err := k.client.R().
		SetContext(ctx).
		SetBodyJsonMarshal(map[string]string{
			"projectId":           k.projectID,
			"name":                name,
			"encryptionAlgorithm": "aes-256-gcm",
		}).
		SetSuccessResult(&result).
		Post("/v1/kms/keys")
	if err != nil {
		return fmt.Errorf("creating KMS key: %w", err)
	}
	if !resp.IsSuccessState() {
		return fmt.Errorf("infisical create key failed (status %d): %s", resp.StatusCode, resp.String())
	}

	k.keys.Store(name, result.Key.ID)
	return nil
}

func (k *InfisicalKMS) Encrypt(ctx context.Context, name string, plaintext []byte) (string, error) {
	keyID, err := k.resolveKeyID(ctx, name)
	if err != nil {
		return "", err
	}

	var result struct {
		Ciphertext string `json:"ciphertext"`
	}

	resp, err := k.client.R().
		SetContext(ctx).
		SetPathParam("keyId", keyID).
		SetBodyJsonMarshal(map[string]string{
			"plaintext": base64.StdEncoding.EncodeToString(plaintext),
		}).
		SetSuccessResult(&result).
		Post("/v1/kms/keys/{keyId}/encrypt")
	if err != nil {
		return "", fmt.Errorf("encrypting via KMS: %w", err)
	}
	if !resp.IsSuccessState() {
		return "", fmt.Errorf("infisical encrypt failed (status %d): %s", resp.StatusCode, resp.String())
	}

	return result.Ciphertext, nil
}

func (k *InfisicalKMS) Decrypt(ctx context.Context, name string, ciphertext string) ([]byte, error) {
	keyID, err := k.resolveKeyID(ctx, name)
	if err != nil {
		return nil, err
	}

	var result struct {
		Plaintext string `json:"plaintext"`
	}

	resp, err := k.client.R().
		SetContext(ctx).
		SetPathParam("keyId", keyID).
		SetBodyJsonMarshal(map[string]string{"ciphertext": ciphertext}).
		SetSuccessResult(&result).
		Post("/v1/kms/keys/{keyId}/decrypt")
	if err != nil {
		return nil, fmt.Errorf("decrypting via KMS: %w", err)
	}
	if !resp.IsSuccessState() {
		return nil, fmt.Errorf("infisical decrypt failed (status %d): %s", resp.StatusCode, resp.String())
	}

	decoded, err := base64.StdEncoding.DecodeString(result.Plaintext)
	if err != nil {
		return nil, fmt.Errorf("decoding plaintext: %w", err)
	}

	return decoded, nil
}
