package signer

import (
	"context"
	"fmt"
	"sync"
	"time"

	"github.com/imroc/req/v3"
)

type InfisicalKMS struct {
	clientID     string
	clientSecret string
	projectID    string
	client       *req.Client

	mu          sync.RWMutex
	accessToken string
	expiresAt   time.Time
}

func NewInfisicalKMS(clientID, clientSecret, projectID string) *InfisicalKMS {
	return &InfisicalKMS{
		clientID:     clientID,
		clientSecret: clientSecret,
		projectID:    projectID,
		client: req.C().
			SetBaseURL("https://app.infisical.com/api").
			SetTimeout(10 * time.Second).
			SetUserAgent("kassi"),
	}
}

func (k *InfisicalKMS) getToken(ctx context.Context) (string, error) {
	k.mu.RLock()
	if k.accessToken != "" && time.Now().Before(k.expiresAt) {
		token := k.accessToken
		k.mu.RUnlock()
		return token, nil
	}
	k.mu.RUnlock()

	k.mu.Lock()
	defer k.mu.Unlock()

	if k.accessToken != "" && time.Now().Before(k.expiresAt) {
		return k.accessToken, nil
	}

	var result struct {
		AccessToken string `json:"accessToken"`
		ExpiresIn   int64  `json:"expiresIn"`
	}

	resp, err := k.client.R().
		SetContext(ctx).
		SetBodyJsonMarshal(map[string]string{
			"clientId":     k.clientID,
			"clientSecret": k.clientSecret,
		}).
		SetSuccessResult(&result).
		Post("/v1/auth/universal-auth/login")
	if err != nil {
		return "", fmt.Errorf("authenticating with infisical: %w", err)
	}
	if !resp.IsSuccessState() {
		return "", fmt.Errorf("infisical auth failed (status %d): %s", resp.StatusCode, resp.String())
	}

	k.accessToken = result.AccessToken
	k.expiresAt = time.Now().Add(time.Duration(result.ExpiresIn)*time.Second - 30*time.Second)

	return k.accessToken, nil
}

func (k *InfisicalKMS) authedRequest(ctx context.Context) (*req.Request, error) {
	token, err := k.getToken(ctx)
	if err != nil {
		return nil, err
	}
	return k.client.R().
		SetContext(ctx).
		SetBearerAuthToken(token), nil
}

func (k *InfisicalKMS) CreateKey(ctx context.Context, name string) error {
	r, err := k.authedRequest(ctx)
	if err != nil {
		return err
	}

	resp, err := r.
		SetBodyJsonMarshal(map[string]string{
			"projectId":           k.projectID,
			"name":                name,
			"encryptionAlgorithm": "aes-256-gcm",
		}).
		Post("/v1/kms/keys")
	if err != nil {
		return fmt.Errorf("creating KMS key: %w", err)
	}
	if !resp.IsSuccessState() {
		return fmt.Errorf("infisical create key failed (status %d): %s", resp.StatusCode, resp.String())
	}

	return nil
}

func (k *InfisicalKMS) Encrypt(ctx context.Context, name string, plaintext []byte) (string, error) {
	r, err := k.authedRequest(ctx)
	if err != nil {
		return "", err
	}

	var result struct {
		Ciphertext string `json:"ciphertext"`
	}

	resp, err := r.
		SetPathParam("name", name).
		SetBodyJsonMarshal(map[string]any{"plaintext": plaintext}).
		SetSuccessResult(&result).
		Post("/v1/kms/keys/{name}/encrypt")
	if err != nil {
		return "", fmt.Errorf("encrypting via KMS: %w", err)
	}
	if !resp.IsSuccessState() {
		return "", fmt.Errorf("infisical encrypt failed (status %d): %s", resp.StatusCode, resp.String())
	}

	return result.Ciphertext, nil
}

func (k *InfisicalKMS) Decrypt(ctx context.Context, name string, ciphertext string) ([]byte, error) {
	r, err := k.authedRequest(ctx)
	if err != nil {
		return nil, err
	}

	var result struct {
		Plaintext []byte `json:"plaintext"`
	}

	resp, err := r.
		SetPathParam("name", name).
		SetBodyJsonMarshal(map[string]string{"ciphertext": ciphertext}).
		SetSuccessResult(&result).
		Post("/v1/kms/keys/{name}/decrypt")
	if err != nil {
		return nil, fmt.Errorf("decrypting via KMS: %w", err)
	}
	if !resp.IsSuccessState() {
		return nil, fmt.Errorf("infisical decrypt failed (status %d): %s", resp.StatusCode, resp.String())
	}

	return result.Plaintext, nil
}
