package worker

import (
	"context"

	"github.com/riverqueue/river"
	"go.uber.org/zap"

	"github.com/prettyirrelevant/kassi/internal/datastore"
)

type WebhookArgs struct {
	WebhookDeliveryID string `json:"webhook_delivery_id"`
	MerchantID        string `json:"merchant_id"`
	EventType         string `json:"event_type"`
	URL               string `json:"url"`
	Payload           string `json:"payload"`
}

func (WebhookArgs) Kind() string { return "webhook" }

func (args WebhookArgs) InsertOpts() river.InsertOpts {
	return river.InsertOpts{Queue: "webhooks", MaxAttempts: 10}
}

type WebhookWorker struct {
	river.WorkerDefaults[WebhookArgs]
	store  *datastore.Store
	logger *zap.Logger
}

func (w *WebhookWorker) Work(ctx context.Context, job *river.Job[WebhookArgs]) error {
	// TODO: implement webhook delivery flow
	return nil
}
