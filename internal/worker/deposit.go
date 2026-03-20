package worker

import (
	"context"

	"github.com/riverqueue/river"
	"go.uber.org/zap"

	"github.com/prettyirrelevant/kassi/internal/datastore"
)

type DepositArgs struct {
	NetworkID    string `json:"network_id"`
	TxHash       string `json:"tx_hash"`
	FromAddress  string `json:"from_address"`
	ToAddress    string `json:"to_address"`
	Amount       string `json:"amount"`
	TokenAddress string `json:"token_address"`
	BlockNumber  int64  `json:"block_number"`
}

func (DepositArgs) Kind() string { return "deposit" }

func (args DepositArgs) InsertOpts() river.InsertOpts {
	return river.InsertOpts{Queue: "deposits", MaxAttempts: 5}
}

type DepositWorker struct {
	river.WorkerDefaults[DepositArgs]
	store  *datastore.Store
	logger *zap.Logger
}

func (w *DepositWorker) Work(ctx context.Context, job *river.Job[DepositArgs]) error {
	// TODO: implement deposit processing flow
	return nil
}
