package api

import (
	"errors"
	"net/http"
	"strings"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/labstack/echo/v5"
	"go.uber.org/zap"

	"github.com/prettyirrelevant/kassi/internal/api/handler"
	"github.com/prettyirrelevant/kassi/internal/helpers"
)

func (s *Server) wideEventLog(next echo.HandlerFunc) echo.HandlerFunc {
	return func(c *echo.Context) error {
		start := time.Now()

		err := next(c)

		status := http.StatusOK
		if err != nil {
			var sc echo.HTTPStatusCoder
			if errors.As(err, &sc) {
				status = sc.StatusCode()
			} else {
				status = http.StatusInternalServerError
			}
		} else if rw, unwrapErr := echo.UnwrapResponse(c.Response()); unwrapErr == nil {
			status = rw.Status
		}

		fields := []zap.Field{
			zap.String("method", c.Request().Method),
			zap.String("path", c.Request().URL.Path),
			zap.Int("status_code", status),
			zap.Int64("duration_ms", time.Since(start).Milliseconds()),
			zap.String("request_id", c.Response().Header().Get(echo.HeaderXRequestID)),
		}

		if merchant := handler.MerchantFromCtx(c); merchant != nil {
			fields = append(fields, zap.String("merchant_id", merchant.ID))
		}

		if err != nil {
			fields = append(fields, zap.Error(err))
			s.logger.Error("request", fields...)
		} else if status >= http.StatusInternalServerError {
			s.logger.Error("request", fields...)
		} else {
			s.logger.Info("request", fields...)
		}

		return err
	}
}

func (s *Server) requireSession(next echo.HandlerFunc) echo.HandlerFunc {
	return func(c *echo.Context) error {
		header := c.Request().Header.Get("Authorization")
		if !strings.HasPrefix(header, "Bearer ") {
			return handler.ErrUnauthorized
		}

		claims, err := s.parseJWT(strings.TrimPrefix(header, "Bearer "))
		if err != nil {
			return handler.ErrUnauthorized
		}

		merchantID, _ := claims["merchant_id"].(string)
		if merchantID == "" {
			return handler.ErrUnauthorized
		}

		merchant, err := s.store.FindMerchantByID(c.Request().Context(), merchantID)
		if err != nil {
			return handler.ErrUnauthorized
		}

		c.Set("merchant", merchant)
		return next(c)
	}
}

func (s *Server) requireSecretKey(next echo.HandlerFunc) echo.HandlerFunc {
	return func(c *echo.Context) error {
		key := c.Request().Header.Get("X-API-Key")
		if key == "" {
			return handler.ErrUnauthorized
		}

		merchant, err := s.store.FindMerchantBySecretKeyHash(
			c.Request().Context(),
			helpers.HashAPIKey(key),
		)
		if err != nil {
			return handler.ErrUnauthorized
		}

		c.Set("merchant", merchant)
		return next(c)
	}
}

//nolint:unused // wired when read-only routes are added
func (s *Server) requirePublicKey(next echo.HandlerFunc) echo.HandlerFunc {
	return func(c *echo.Context) error {
		key := c.Request().Header.Get("X-API-Key")
		if key == "" {
			return handler.ErrUnauthorized
		}

		merchant, err := s.store.FindMerchantByPublicKeyHash(
			c.Request().Context(),
			helpers.HashAPIKey(key),
		)
		if err != nil {
			return handler.ErrUnauthorized
		}

		c.Set("merchant", merchant)
		return next(c)
	}
}

func (s *Server) requireAny(middlewares ...echo.MiddlewareFunc) echo.MiddlewareFunc {
	return func(next echo.HandlerFunc) echo.HandlerFunc {
		return func(c *echo.Context) error {
			for _, mw := range middlewares {
				if err := mw(next)(c); err == nil {
					return nil
				}
			}
			return handler.ErrUnauthorized
		}
	}
}

func (s *Server) parseJWT(tokenStr string) (jwt.MapClaims, error) {
	token, err := jwt.Parse(tokenStr, func(token *jwt.Token) (any, error) {
		if _, ok := token.Method.(*jwt.SigningMethodHMAC); !ok {
			return nil, jwt.ErrSignatureInvalid
		}
		return []byte(s.config.JWTSecret), nil
	})
	if err != nil {
		return nil, err
	}
	claims, ok := token.Claims.(jwt.MapClaims)
	if !ok || !token.Valid {
		return nil, jwt.ErrSignatureInvalid
	}
	return claims, nil
}
