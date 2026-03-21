package handler

import (
	"crypto/ed25519"
	"database/sql"
	"errors"
	"fmt"
	"net/http"
	"strings"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/labstack/echo/v5"
	"github.com/mr-tron/base58"
	"github.com/rs/xid"
	siwe "github.com/spruceid/siwe-go"

	"github.com/prettyirrelevant/kassi/internal/cache"
	"github.com/prettyirrelevant/kassi/internal/config"
	"github.com/prettyirrelevant/kassi/internal/datastore"
)

const nonceTTL = 5 * time.Minute

type AuthHandler struct {
	Store  *datastore.Store
	Cache  *cache.Cache
	Config *config.Config
}

type verifyRequest struct {
	Message   string `json:"message"`
	Signature string `json:"signature"`
}

type linkRequest struct {
	Message   string `json:"message"`
	Signature string `json:"signature"`
}

// GetNonce godoc
// @Summary Request a nonce for wallet signing
// @Tags auth
// @Produce json
// @Success 200 {object} ApiSuccess{data=map[string]string}
// @Failure 500 {object} ApiError
// @Router /auth/nonce [get]
func (h *AuthHandler) GetNonce(c *echo.Context) error {
	nonce := xid.New().String()

	if err := h.Cache.Set(c.Request().Context(), "nonce:"+nonce, "1", nonceTTL); err != nil {
		return fmt.Errorf("storing nonce: %w", err)
	}

	return c.JSON(http.StatusOK, ApiSuccess{Data: map[string]string{"nonce": nonce}})
}

// Verify godoc
// @Summary Authenticate with a signed wallet message
// @Tags auth
// @Accept json
// @Produce json
// @Param body body verifyRequest true "signed message and signature"
// @Success 200 {object} ApiSuccess{data=map[string]string}
// @Success 201 {object} ApiSuccess{data=map[string]string}
// @Failure 400 {object} ApiError
// @Failure 401 {object} ApiError
// @Router /auth/verify [post]
func (h *AuthHandler) Verify(c *echo.Context) error {
	var req verifyRequest
	if err := c.Bind(&req); err != nil {
		return &AppError{
			Status:  http.StatusBadRequest,
			Code:    "invalid_request",
			Message: "invalid request body",
		}
	}

	address, signerType, nonce, err := verifySignature(req.Message, req.Signature)
	if err != nil {
		return ErrInvalidSignature
	}

	if _, err := h.Cache.GetDel(c.Request().Context(), "nonce:"+nonce); err != nil {
		return &AppError{
			Status:  http.StatusUnauthorized,
			Code:    "invalid_nonce",
			Message: "nonce is invalid or expired",
		}
	}

	ctx := c.Request().Context()
	sgn, err := h.Store.FindSignerByAddress(ctx, address)
	if err != nil {
		if !errors.Is(err, sql.ErrNoRows) {
			return fmt.Errorf("finding signer: %w", err)
		}

		merchant, err := h.Store.CreateMerchantWithConfig(ctx, address, signerType)
		if err != nil {
			return fmt.Errorf("creating merchant: %w", err)
		}

		token, err := h.issueJWT(merchant.ID, address, signerType)
		if err != nil {
			return fmt.Errorf("issuing JWT: %w", err)
		}

		return c.JSON(http.StatusCreated, ApiSuccess{Data: map[string]string{"token": token}})
	}

	token, err := h.issueJWT(sgn.MerchantID, address, signerType)
	if err != nil {
		return fmt.Errorf("issuing JWT: %w", err)
	}

	return c.JSON(http.StatusOK, ApiSuccess{Data: map[string]string{"token": token}})
}

// Link godoc
// @Summary Link an additional wallet to the current merchant
// @Tags auth
// @Accept json
// @Produce json
// @Param body body linkRequest true "signed message and signature"
// @Success 201 {object} ApiSuccess{data=map[string]string}
// @Failure 400 {object} ApiError
// @Failure 401 {object} ApiError
// @Failure 409 {object} ApiError
// @Security BearerAuth
// @Router /auth/link [post]
func (h *AuthHandler) Link(c *echo.Context) error {
	merchant := MerchantFromCtx(c)

	var req linkRequest
	if err := c.Bind(&req); err != nil {
		return &AppError{
			Status:  http.StatusBadRequest,
			Code:    "invalid_request",
			Message: "invalid request body",
		}
	}

	address, signerType, nonce, err := verifySignature(req.Message, req.Signature)
	if err != nil {
		return ErrInvalidSignature
	}

	ctx := c.Request().Context()
	if _, err := h.Cache.GetDel(ctx, "nonce:"+nonce); err != nil {
		return &AppError{
			Status:  http.StatusUnauthorized,
			Code:    "invalid_nonce",
			Message: "nonce is invalid or expired",
		}
	}

	if _, err := h.Store.FindSignerByAddress(ctx, address); err == nil {
		return &AppError{
			Status:  http.StatusConflict,
			Code:    "signer_already_linked",
			Message: "this wallet is already linked to a merchant account",
		}
	}

	if _, err := h.Store.CreateSigner(ctx, merchant.ID, address, signerType); err != nil {
		return fmt.Errorf("creating signer: %w", err)
	}

	return c.JSON(http.StatusCreated, ApiSuccess{Data: map[string]string{"status": "linked"}})
}

func (h *AuthHandler) issueJWT(merchantID, signerAddress, signerType string) (string, error) {
	now := time.Now()
	token := jwt.NewWithClaims(jwt.SigningMethodHS256, jwt.MapClaims{
		"merchant_id":    merchantID,
		"signer_address": signerAddress,
		"signer_type":    signerType,
		"iat":            now.Unix(),
		"exp":            now.Add(h.Config.JWTExpiry).Unix(),
	})
	return token.SignedString([]byte(h.Config.JWTSecret))
}

func verifySignature(message, signature string) (address, signerType, nonce string, err error) {
	if addr, n, err := verifySIWE(message, signature); err == nil {
		return addr, "evm", n, nil
	}

	if addr, n, err := verifySIWS(message, signature); err == nil {
		return addr, "solana", n, nil
	}

	return "", "", "", fmt.Errorf("signature verification failed")
}

func verifySIWE(message, signature string) (string, string, error) {
	msg, err := siwe.ParseMessage(message)
	if err != nil {
		return "", "", fmt.Errorf("parsing SIWE message: %w", err)
	}

	_, err = msg.Verify(signature, nil, nil, nil)
	if err != nil {
		return "", "", fmt.Errorf("verifying SIWE signature: %w", err)
	}

	return msg.GetAddress().Hex(), msg.GetNonce(), nil
}

func verifySIWS(message, signature string) (string, string, error) {
	lines := strings.Split(message, "\n")
	if len(lines) < 2 || !strings.Contains(lines[0], "wants you to sign in with your Solana account:") {
		return "", "", fmt.Errorf("not a SIWS message")
	}

	address := strings.TrimSpace(lines[1])
	if address == "" {
		return "", "", fmt.Errorf("missing address in SIWS message")
	}

	var nonce string
	for _, line := range lines {
		if strings.HasPrefix(line, "Nonce: ") {
			nonce = strings.TrimPrefix(line, "Nonce: ")
			break
		}
	}
	if nonce == "" {
		return "", "", fmt.Errorf("nonce not found in SIWS message")
	}

	pubKeyBytes, err := base58.Decode(address)
	if err != nil {
		return "", "", fmt.Errorf("decoding solana address: %w", err)
	}
	if len(pubKeyBytes) != ed25519.PublicKeySize {
		return "", "", fmt.Errorf("invalid solana public key length")
	}

	sigBytes, err := base58.Decode(signature)
	if err != nil {
		return "", "", fmt.Errorf("decoding solana signature: %w", err)
	}
	if len(sigBytes) != ed25519.SignatureSize {
		return "", "", fmt.Errorf("invalid solana signature length")
	}

	if !ed25519.Verify(pubKeyBytes, []byte(message), sigBytes) {
		return "", "", fmt.Errorf("solana signature verification failed")
	}

	return address, nonce, nil
}
