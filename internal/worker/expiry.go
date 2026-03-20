package worker

import (
	"context"

	"github.com/riverqueue/river"
	"go.uber.org/zap"

	"github.com/prettyirrelevant/kassi/internal/datastore"
)

// ExpiryArgs is empty because the expiry worker is sweep-style:
// it ignores the job payload and scans the DB for expired payment intents.
type ExpiryArgs struct{}

func (ExpiryArgs) Kind() string { return "expiry" }

func (args ExpiryArgs) InsertOpts() river.InsertOpts {
	return river.InsertOpts{Queue: "expiry", MaxAttempts: 3}
}

type ExpiryWorker struct {
	river.WorkerDefaults[ExpiryArgs]
	store  *datastore.Store
	logger *zap.Logger
}

func (w *ExpiryWorker) Work(ctx context.Context, job *river.Job[ExpiryArgs]) error {
	// TODO: implement expiry scanning flow
	return nil
}
