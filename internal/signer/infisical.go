package signer

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"sync"
	"time"
)

const infisicalBaseURL = "https://app.infisical.com/api"

type InfisicalKMS struct {
	clientID     string
	clientSecret string
	projectID    string
	httpClient   *http.Client

	mu          sync.RWMutex
	accessToken string
	expiresAt   time.Time
}

func NewInfisicalKMS(clientID, clientSecret, projectID string) *InfisicalKMS {
	return &InfisicalKMS{
		clientID:     clientID,
		clientSecret: clientSecret,
		projectID:    projectID,
		httpClient:   &http.Client{Timeout: 10 * time.Second},
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

	// double-check after acquiring write lock
	if k.accessToken != "" && time.Now().Before(k.expiresAt) {
		return k.accessToken, nil
	}

	body, _ := json.Marshal(map[string]string{
		"clientId":     k.clientID,
		"clientSecret": k.clientSecret,
	})

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, infisicalBaseURL+"/v1/auth/universal-auth/login", bytes.NewReader(body))
	if err != nil {
		return "", fmt.Errorf("creating auth request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := k.httpClient.Do(req)
	if err != nil {
		return "", fmt.Errorf("authenticating with infisical: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		respBody, _ := io.ReadAll(resp.Body)
		return "", fmt.Errorf("infisical auth failed (status %d): %s", resp.StatusCode, respBody)
	}

	var result struct {
		AccessToken       string `json:"accessToken"`
		ExpiresIn         int64  `json:"expiresIn"`
		AccessTokenMaxTTL int64  `json:"accessTokenMaxTTL"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return "", fmt.Errorf("decoding auth response: %w", err)
	}

	k.accessToken = result.AccessToken
	k.expiresAt = time.Now().Add(time.Duration(result.ExpiresIn)*time.Second - 30*time.Second)

	return k.accessToken, nil
}

func (k *InfisicalKMS) doRequest(ctx context.Context, method, path string, reqBody any) ([]byte, error) {
	token, err := k.getToken(ctx)
	if err != nil {
		return nil, err
	}

	var bodyReader io.Reader
	if reqBody != nil {
		b, _ := json.Marshal(reqBody)
		bodyReader = bytes.NewReader(b)
	}

	req, err := http.NewRequestWithContext(ctx, method, infisicalBaseURL+path, bodyReader)
	if err != nil {
		return nil, fmt.Errorf("creating request: %w", err)
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := k.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("executing request: %w", err)
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("reading response: %w", err)
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("infisical API error (status %d): %s", resp.StatusCode, respBody)
	}

	return respBody, nil
}

func (k *InfisicalKMS) CreateKey(ctx context.Context, name string) error {
	_, err := k.doRequest(ctx, http.MethodPost, "/v1/kms/keys", map[string]string{
		"projectId":        k.projectID,
		"name":             name,
		"encryptionAlgorithm": "aes-256-gcm",
	})
	return err
}

func (k *InfisicalKMS) Encrypt(ctx context.Context, name string, plaintext []byte) (string, error) {
	respBody, err := k.doRequest(ctx, http.MethodPost, "/v1/kms/keys/"+name+"/encrypt", map[string]any{
		"plaintext": plaintext,
	})
	if err != nil {
		return "", err
	}

	var result struct {
		Ciphertext string `json:"ciphertext"`
	}
	if err := json.Unmarshal(respBody, &result); err != nil {
		return "", fmt.Errorf("decoding encrypt response: %w", err)
	}

	return result.Ciphertext, nil
}

func (k *InfisicalKMS) Decrypt(ctx context.Context, name string, ciphertext string) ([]byte, error) {
	respBody, err := k.doRequest(ctx, http.MethodPost, "/v1/kms/keys/"+name+"/decrypt", map[string]string{
		"ciphertext": ciphertext,
	})
	if err != nil {
		return nil, err
	}

	var result struct {
		Plaintext []byte `json:"plaintext"`
	}
	if err := json.Unmarshal(respBody, &result); err != nil {
		return nil, fmt.Errorf("decoding decrypt response: %w", err)
	}

	return result.Plaintext, nil
}
