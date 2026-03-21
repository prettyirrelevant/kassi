package server

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"net/http"
	"strings"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"go.uber.org/zap"

	"github.com/prettyirrelevant/kassi/internal/datastore"
	"github.com/prettyirrelevant/kassi/internal/server/handlers"
	"github.com/prettyirrelevant/kassi/internal/util"
)

type responseWriter struct {
	http.ResponseWriter
	status      int
	wroteHeader bool
}

func (rw *responseWriter) WriteHeader(code int) {
	if !rw.wroteHeader {
		rw.status = code
		rw.wroteHeader = true
	}
	rw.ResponseWriter.WriteHeader(code)
}

func (rw *responseWriter) Write(b []byte) (int, error) {
	if !rw.wroteHeader {
		rw.wroteHeader = true
	}
	return rw.ResponseWriter.Write(b)
}

func (s *Server) wideEventLog(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		fields := map[string]any{
			"method": r.Method,
			"path":   r.URL.Path,
		}
		ctx := context.WithValue(r.Context(), handlers.CtxWideEvent, fields)

		rw := &responseWriter{ResponseWriter: w, status: http.StatusOK}
		next.ServeHTTP(rw, r.WithContext(ctx))

		fields["status_code"] = rw.status
		fields["duration_ms"] = time.Since(start).Milliseconds()

		zapFields := make([]zap.Field, 0, len(fields))
		for k, v := range fields {
			zapFields = append(zapFields, zap.Any(k, v))
		}

		if rw.status >= 500 {
			s.logger.Error("request", zapFields...)
		} else {
			s.logger.Info("request", zapFields...)
		}
	})
}

func (s *Server) requireSession(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		header := r.Header.Get("Authorization")
		if !strings.HasPrefix(header, "Bearer ") {
			writeUnauthorized(w)
			return
		}

		claims, err := s.parseJWT(strings.TrimPrefix(header, "Bearer "))
		if err != nil {
			writeUnauthorized(w)
			return
		}

		merchantID, _ := claims["merchant_id"].(string)
		if merchantID == "" {
			writeUnauthorized(w)
			return
		}

		merchant, err := s.store.FindMerchantByID(r.Context(), merchantID)
		if err != nil {
			writeUnauthorized(w)
			return
		}

		s.setMerchant(w, r, next, merchant)
	})
}

//nolint:unused // wired when merchant/deposit/payment routes are added
func (s *Server) requireSecretKey(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		key := r.Header.Get("X-API-Key")
		if key == "" {
			writeUnauthorized(w)
			return
		}

		merchant, err := s.store.FindMerchantBySecretKeyHash(r.Context(), hashAPIKey(key))
		if err != nil {
			writeUnauthorized(w)
			return
		}

		s.setMerchant(w, r, next, merchant)
	})
}

//nolint:unused // wired when read-only routes are added
func (s *Server) requirePublicKey(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		key := r.Header.Get("X-API-Key")
		if key == "" {
			writeUnauthorized(w)
			return
		}

		merchant, err := s.store.FindMerchantByPublicKeyHash(r.Context(), hashAPIKey(key))
		if err != nil {
			writeUnauthorized(w)
			return
		}

		s.setMerchant(w, r, next, merchant)
	})
}

//nolint:unused // wired when routes need multiple auth strategies
func (s *Server) requireAny(middlewares ...func(http.Handler) http.Handler) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			for _, mw := range middlewares {
				passed := false
				probe := mw(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
					passed = true
					next.ServeHTTP(w, r)
				}))

				rw := &noopResponseWriter{}
				probe.ServeHTTP(rw, r)
				if passed {
					return
				}
			}

			writeUnauthorized(w)
		})
	}
}

//nolint:unused // used by requireAny
type noopResponseWriter struct{}

//nolint:unused // implements http.ResponseWriter for requireAny
func (noopResponseWriter) Header() http.Header { return http.Header{} }

//nolint:unused // implements http.ResponseWriter for requireAny
func (noopResponseWriter) Write(b []byte) (int, error) { return len(b), nil }

//nolint:unused // implements http.ResponseWriter for requireAny
func (noopResponseWriter) WriteHeader(int) {}

func (s *Server) setMerchant(w http.ResponseWriter, r *http.Request, next http.Handler, merchant *datastore.Merchant) {
	if fields := handlers.WideEventFields(r.Context()); fields != nil {
		fields["merchant_id"] = merchant.ID
	}

	ctx := context.WithValue(r.Context(), handlers.CtxMerchant, merchant)
	next.ServeHTTP(w, r.WithContext(ctx))
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

func writeUnauthorized(w http.ResponseWriter) {
	util.WriteJSON(w, util.ErrUnauthorized.Status, util.ApiError{
		Error: util.ErrorBody{Code: util.ErrUnauthorized.Code, Message: util.ErrUnauthorized.Message},
	})
}

//nolint:unused // used by requireSecretKey and requirePublicKey
func hashAPIKey(key string) string {
	h := sha256.Sum256([]byte(key))
	return hex.EncodeToString(h[:])
}
