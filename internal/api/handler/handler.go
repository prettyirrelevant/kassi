package handler

import (
	"net/http"

	validation "github.com/go-ozzo/ozzo-validation/v4"
	"github.com/labstack/echo/v5"

	"github.com/prettyirrelevant/kassi/internal/datastore"
)

type ApiSuccess struct {
	Data any `json:"data"`
}

type ApiList struct {
	Data any      `json:"data"`
	Meta ListMeta `json:"meta"`
}

type ListMeta struct {
	Page    int `json:"page"`
	PerPage int `json:"per_page"`
	Total   int `json:"total"`
}

type ApiError struct {
	Error ErrorBody `json:"error"`
}

type ErrorBody struct {
	Code    string       `json:"code"`
	Message string       `json:"message"`
	Details []FieldError `json:"details,omitempty"`
}

type FieldError struct {
	Field   string `json:"field"`
	Code    string `json:"code"`
	Message string `json:"message"`
}

type AppError struct {
	Status  int
	Code    string
	Message string
	Details []FieldError
}

func (e *AppError) Error() string      { return e.Message }
func (e *AppError) StatusCode() int    { return e.Status }

var (
	ErrNotFound          = &AppError{Status: http.StatusNotFound, Code: "resource_not_found", Message: "resource not found"}
	ErrUnauthorized      = &AppError{Status: http.StatusUnauthorized, Code: "authentication_required", Message: "authentication required"}
	ErrForbidden         = &AppError{Status: http.StatusForbidden, Code: "forbidden", Message: "access denied"}
	ErrInvalidSignature  = &AppError{Status: http.StatusUnauthorized, Code: "invalid_signature", Message: "wallet signature verification failed"}
	ErrRateLimitExceeded = &AppError{Status: http.StatusTooManyRequests, Code: "rate_limit_exceeded", Message: "too many requests"}
	ErrInternal          = &AppError{Status: http.StatusInternalServerError, Code: "internal_error", Message: "an unexpected error occurred"}
)

func ValidationError(err error) *AppError {
	var details []FieldError
	if e, ok := err.(validation.Errors); ok {
		for field, fieldErr := range e {
			details = append(details, FieldError{
				Field:   field,
				Code:    "invalid_field_value",
				Message: fieldErr.Error(),
			})
		}
	}
	return &AppError{
		Status:  http.StatusBadRequest,
		Code:    "validation_failed",
		Message: "request validation failed",
		Details: details,
	}
}

func MerchantFromCtx(c *echo.Context) *datastore.Merchant {
	m, _ := (*c).Get("merchant").(*datastore.Merchant)
	return m
}

type paginationRequest struct {
	Page    int `query:"page"`
	PerPage int `query:"per_page"`
}

func (r *paginationRequest) Validate() error {
	if r.Page == 0 {
		r.Page = 1
	}
	if r.PerPage == 0 {
		r.PerPage = 20
	}
	return validation.ValidateStruct(r,
		validation.Field(&r.Page, validation.Min(1)),
		validation.Field(&r.PerPage, validation.Min(1), validation.Max(100)),
	)
}
