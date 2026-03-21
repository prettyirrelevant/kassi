package handler

import (
	"encoding/json"
	"errors"
	"net/http"

	validation "github.com/go-ozzo/ozzo-validation/v4"
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

func (e *AppError) Error() string { return e.Message }

var (
	ErrNotFound          = &AppError{Status: http.StatusNotFound, Code: "resource_not_found", Message: "resource not found"}
	ErrRouteNotFound     = &AppError{Status: http.StatusNotFound, Code: "route_not_found", Message: "no route matched"}
	ErrUnauthorized      = &AppError{Status: http.StatusUnauthorized, Code: "authentication_required", Message: "authentication required"}
	ErrForbidden         = &AppError{Status: http.StatusForbidden, Code: "forbidden", Message: "access denied"}
	ErrInvalidSignature  = &AppError{Status: http.StatusUnauthorized, Code: "invalid_signature", Message: "wallet signature verification failed"}
	ErrRateLimitExceeded = &AppError{Status: http.StatusTooManyRequests, Code: "rate_limit_exceeded", Message: "too many requests"}
	ErrInternal          = &AppError{Status: http.StatusInternalServerError, Code: "internal_error", Message: "an unexpected error occurred"}
)

type HandlerFunc func(w http.ResponseWriter, r *http.Request) error

func Wrap(fn HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if err := fn(w, r); err != nil {
			var appErr *AppError
			if errors.As(err, &appErr) {
				WriteJSON(w, appErr.Status, ApiError{
					Error: ErrorBody{
						Code:    appErr.Code,
						Message: appErr.Message,
						Details: appErr.Details,
					},
				})
				return
			}
			WriteJSON(w, http.StatusInternalServerError, ApiError{
				Error: ErrorBody{
					Code:    "internal_error",
					Message: "an unexpected error occurred",
				},
			})
		}
	}
}

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

func WriteJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}
