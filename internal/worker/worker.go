package worker

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/riverqueue/river"
	"github.com/riverqueue/river/riverdriver/riverpgxv5"
	"github.com/riverqueue/river/rivertype"
	"go.uber.org/zap"

	"github.com/prettyirrelevant/kassi/internal/datastore"
)

type wideEventErrorHandler struct {
	logger *zap.Logger
}

func (h *wideEventErrorHandler) HandleError(ctx context.Context, job *rivertype.JobRow, err error) *river.ErrorHandlerResult {
	h.logger.Error("job failed",
		zap.String("kind", job.Kind),
		zap.String("queue", job.Queue),
		zap.Int("attempt", job.Attempt),
		zap.Int64("job_id", job.ID),
		zap.Error(err),
	)
	return nil
}

func (h *wideEventErrorHandler) HandlePanic(ctx context.Context, job *rivertype.JobRow, panicVal any, trace string) *river.ErrorHandlerResult {
	h.logger.Error("job panicked",
		zap.String("kind", job.Kind),
		zap.String("queue", job.Queue),
		zap.Int("attempt", job.Attempt),
		zap.Int64("job_id", job.ID),
		zap.Any("panic_value", panicVal),
		zap.String("trace", trace),
	)
	return nil
}

func SetupClient(ctx context.Context, pool *pgxpool.Pool, store *datastore.Store, logger *zap.Logger) (*river.Client[pgx.Tx], error) {
	workers := river.NewWorkers()
	river.AddWorker(workers, &DepositWorker{store: store, logger: logger})
	river.AddWorker(workers, &WebhookWorker{store: store, logger: logger})
	river.AddWorker(workers, &ExpiryWorker{store: store, logger: logger})

	client, err := river.NewClient(riverpgxv5.New(pool), &river.Config{
		Queues: map[string]river.QueueConfig{
			"deposits":      {MaxWorkers: 5},
			"webhooks":      {MaxWorkers: 10},
			"expiry":        {MaxWorkers: 1},
			"confirmations": {MaxWorkers: 5},
			"sweeps":        {MaxWorkers: 3},
			"refunds":       {MaxWorkers: 3},
		},
		Workers:      workers,
		ErrorHandler: &wideEventErrorHandler{logger: logger},
	})
	if err != nil {
		return nil, fmt.Errorf("creating river client: %w", err)
	}

	return client, nil
}
