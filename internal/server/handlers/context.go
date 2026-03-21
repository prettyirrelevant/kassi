package handlers

import (
	"context"

	"github.com/prettyirrelevant/kassi/internal/datastore"
)

type ContextKey string

const (
	CtxMerchant  ContextKey = "merchant"
	CtxWideEvent ContextKey = "wide_event"
)

func MerchantFromCtx(ctx context.Context) *datastore.Merchant {
	m, _ := ctx.Value(CtxMerchant).(*datastore.Merchant)
	return m
}

func WideEventFields(ctx context.Context) map[string]any {
	m, _ := ctx.Value(CtxWideEvent).(map[string]any)
	return m
}
