package handler

import (
	"fmt"
	"net/http"

	"github.com/labstack/echo/v5"

	"github.com/prettyirrelevant/kassi/internal/config"
	"github.com/prettyirrelevant/kassi/internal/datastore"
	"github.com/prettyirrelevant/kassi/internal/helpers"
)

type MerchantHandler struct {
	Store  *datastore.Store
	Config *config.Config
}

type updateMerchantRequest struct {
	Name       *string `json:"name"`
	WebhookURL *string `json:"webhook_url"`
}

// GetMe godoc
// @Summary Retrieve the authenticated merchant
// @Tags merchants
// @Produce json
// @Success 200 {object} ApiSuccess
// @Failure 401 {object} ApiError
// @Security BearerAuth
// @Security APIKeyAuth
// @Router /merchants/me [get]
func (h *MerchantHandler) GetMe(c *echo.Context) error {
	return c.JSON(http.StatusOK, ApiSuccess{Data: MerchantFromCtx(c)})
}

// UpdateMe godoc
// @Summary Update merchant settings
// @Tags merchants
// @Accept json
// @Produce json
// @Param body body updateMerchantRequest true "fields to update"
// @Success 200 {object} ApiSuccess
// @Failure 400 {object} ApiError
// @Failure 401 {object} ApiError
// @Security BearerAuth
// @Router /merchants/me [patch]
func (h *MerchantHandler) UpdateMe(c *echo.Context) error {
	var req updateMerchantRequest
	if err := c.Bind(&req); err != nil {
		return &AppError{
			Status:  http.StatusBadRequest,
			Code:    "invalid_request",
			Message: "invalid request body",
		}
	}

	updated, err := h.Store.UpdateMerchant(
		c.Request().Context(),
		MerchantFromCtx(c).ID,
		req.Name,
		req.WebhookURL,
	)
	if err != nil {
		return fmt.Errorf("updating merchant: %w", err)
	}

	return c.JSON(http.StatusOK, ApiSuccess{Data: updated})
}

// RotateKey godoc
// @Summary Rotate the API secret key
// @Tags merchants
// @Produce json
// @Success 200 {object} ApiSuccess{data=map[string]string}
// @Failure 401 {object} ApiError
// @Security BearerAuth
// @Router /merchants/me/rotate-key [post]
func (h *MerchantHandler) RotateKey(c *echo.Context) error {
	secretKey := helpers.RandomString(h.Config.SecretKeyPrefix(), 32)
	if err := h.Store.UpdateSecretKeyHash(
		c.Request().Context(),
		MerchantFromCtx(c).ID,
		helpers.HashAPIKey(secretKey),
	); err != nil {
		return fmt.Errorf("rotating secret key: %w", err)
	}

	return c.JSON(http.StatusOK, ApiSuccess{Data: map[string]string{"secret_key": secretKey}})
}

// RotateWebhookSecret godoc
// @Summary Rotate the webhook signing secret
// @Tags merchants
// @Produce json
// @Success 200 {object} ApiSuccess{data=map[string]string}
// @Failure 401 {object} ApiError
// @Security BearerAuth
// @Router /merchants/me/rotate-webhook-secret [post]
func (h *MerchantHandler) RotateWebhookSecret(c *echo.Context) error {
	secret := helpers.RandomString("whsec_", 32)
	if err := h.Store.UpdateWebhookSecret(
		c.Request().Context(),
		MerchantFromCtx(c).ID,
		secret,
	); err != nil {
		return fmt.Errorf("rotating webhook secret: %w", err)
	}

	return c.JSON(http.StatusOK, ApiSuccess{Data: map[string]string{"webhook_secret": secret}})
}
