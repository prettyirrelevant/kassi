package handler

import (
	"database/sql"
	"errors"
	"fmt"
	"net/http"

	validation "github.com/go-ozzo/ozzo-validation/v4"
	"github.com/labstack/echo/v5"

	"github.com/prettyirrelevant/kassi/internal/config"
	"github.com/prettyirrelevant/kassi/internal/datastore"
)

type SettlementDestinationHandler struct {
	Store  *datastore.Store
	Config *config.Config
}

type createSettlementDestinationRequest struct {
	Message    string   `json:"message"`
	Signature  string   `json:"signature"`
	NetworkIDs []string `json:"network_ids"`
}

func (r createSettlementDestinationRequest) Validate() error {
	return validation.ValidateStruct(&r,
		validation.Field(&r.Message, validation.Required),
		validation.Field(&r.Signature, validation.Required),
		validation.Field(&r.NetworkIDs, validation.Required, validation.Length(1, 0)),
	)
}

type deleteSettlementDestinationRequest struct {
	Message   string `json:"message"`
	Signature string `json:"signature"`
}

func (r deleteSettlementDestinationRequest) Validate() error {
	return validation.ValidateStruct(&r,
		validation.Field(&r.Message, validation.Required),
		validation.Field(&r.Signature, validation.Required),
	)
}

// Create godoc
// @Summary Create or update settlement destinations for one or more networks
// @Tags settlement-destinations
// @Accept json
// @Produce json
// @Param body body createSettlementDestinationRequest true "signed message, signature, and network IDs"
// @Success 201 {object} ApiSuccess
// @Failure 400 {object} ApiError
// @Failure 401 {object} ApiError
// @Security BearerAuth
// @Router /settlement-destinations [post]
func (h *SettlementDestinationHandler) Create(c *echo.Context) error {
	var req createSettlementDestinationRequest
	if err := c.Bind(&req); err != nil {
		return &AppError{
			Status:  http.StatusBadRequest,
			Code:    "invalid_request",
			Message: "invalid request body",
		}
	}

	if err := req.Validate(); err != nil {
		return ValidationError(err)
	}

	address, signerType, _, err := verifySignature(req.Message, req.Signature)
	if err != nil {
		return ErrInvalidSignature
	}

	ctx := c.Request().Context()
	networks, err := h.Store.FindNetworksByIDs(ctx, req.NetworkIDs)
	if err != nil {
		return fmt.Errorf("finding networks: %w", err)
	}

	var fieldErrors []FieldError
	if len(networks) != len(req.NetworkIDs) {
		fieldErrors = append(fieldErrors, FieldError{
			Field:   "network_ids",
			Code:    "invalid_field_value",
			Message: "one or more network IDs do not exist",
		})
	}
	for _, n := range networks {
		if n.ChainType != signerType {
			fieldErrors = append(fieldErrors, FieldError{
				Field:   "network_ids",
				Code:    "invalid_field_value",
				Message: fmt.Sprintf("wallet signature is %s but network %s requires %s", signerType, n.ID, n.ChainType),
			})
			break
		}
	}
	if len(fieldErrors) > 0 {
		return &AppError{
			Status:  http.StatusBadRequest,
			Code:    "validation_failed",
			Message: "request validation failed",
			Details: fieldErrors,
		}
	}

	destinations, err := h.Store.UpsertSettlementDestinations(
		ctx,
		MerchantFromCtx(c).ID,
		address,
		req.NetworkIDs,
	)
	if err != nil {
		return fmt.Errorf("upserting settlement destinations: %w", err)
	}

	return c.JSON(http.StatusCreated, ApiSuccess{Data: destinations})
}

// List godoc
// @Summary List all settlement destinations
// @Tags settlement-destinations
// @Produce json
// @Param page query int false "page number (default 1)"
// @Param per_page query int false "items per page (default 20, max 100)"
// @Success 200 {object} ApiList
// @Failure 401 {object} ApiError
// @Security BearerAuth
// @Security APIKeyAuth
// @Router /settlement-destinations [get]
func (h *SettlementDestinationHandler) List(c *echo.Context) error {
	var req paginationRequest
	if err := c.Bind(&req); err != nil {
		return &AppError{
			Status:  http.StatusBadRequest,
			Code:    "invalid_request",
			Message: "invalid query parameters",
		}
	}

	if err := req.Validate(); err != nil {
		return ValidationError(err)
	}

	destinations, total, err := h.Store.ListSettlementDestinations(
		c.Request().Context(),
		MerchantFromCtx(c).ID,
		req.Page,
		req.PerPage,
	)
	if err != nil {
		return fmt.Errorf("listing settlement destinations: %w", err)
	}

	return c.JSON(http.StatusOK, ApiList{
		Data: destinations,
		Meta: ListMeta{
			Page:    req.Page,
			PerPage: req.PerPage,
			Total:   total,
		},
	})
}

// Delete godoc
// @Summary Delete a settlement destination
// @Tags settlement-destinations
// @Accept json
// @Produce json
// @Param id path string true "settlement destination ID"
// @Param body body deleteSettlementDestinationRequest true "signed message and signature"
// @Success 204
// @Failure 400 {object} ApiError
// @Failure 401 {object} ApiError
// @Failure 404 {object} ApiError
// @Security BearerAuth
// @Router /settlement-destinations/{id} [delete]
func (h *SettlementDestinationHandler) Delete(c *echo.Context) error {
	var req deleteSettlementDestinationRequest
	if err := c.Bind(&req); err != nil {
		return &AppError{
			Status:  http.StatusBadRequest,
			Code:    "invalid_request",
			Message: "invalid request body",
		}
	}

	if err := req.Validate(); err != nil {
		return ValidationError(err)
	}

	if _, _, _, err := verifySignature(req.Message, req.Signature); err != nil {
		return ErrInvalidSignature
	}

	ctx := c.Request().Context()
	dest, err := h.Store.FindSettlementDestinationByID(ctx, c.Param("id"))
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return ErrNotFound
		}
		return fmt.Errorf("finding settlement destination: %w", err)
	}

	if dest.MerchantID != MerchantFromCtx(c).ID {
		return ErrNotFound
	}

	if err := h.Store.DeleteSettlementDestination(ctx, dest.ID); err != nil {
		return fmt.Errorf("deleting settlement destination: %w", err)
	}

	return c.NoContent(http.StatusNoContent)
}
