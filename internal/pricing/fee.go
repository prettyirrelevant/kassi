package pricing

import "github.com/shopspring/decimal"

// FeeConfig holds the platform fee parameters from server config.
type FeeConfig struct {
	BPS      decimal.Decimal // fee rate in basis points (e.g. 50 = 0.5%)
	MaxCents decimal.Decimal // max fee cap in cents, currency-agnostic
}

// FeeResult holds the computed fee and net amount, both in token smallest units.
type FeeResult struct {
	FeeAmount decimal.Decimal
	NetAmount decimal.Decimal
}

var (
	bpsBase  = decimal.NewFromInt(10000)
	centsDiv = decimal.NewFromInt(100)
)

// CalculateFee computes the platform fee for a deposit amount.
// All amounts are integers in token smallest units. exchangeRate is the fiat
// price per whole token unit (fractional). decimals is the token's decimal places.
//
// fee = min(floor(amount * BPS / 10000), floor(MaxCents / 100 / exchangeRate * 10^decimals))
// net = amount - fee
func CalculateFee(amount decimal.Decimal, config FeeConfig, exchangeRate decimal.Decimal, decimals int) FeeResult {
	if config.BPS.IsZero() || amount.IsZero() {
		return FeeResult{FeeAmount: decimal.Zero, NetAmount: amount}
	}

	feePct := amount.Mul(config.BPS).Div(bpsBase).Floor()

	maxInTokens := config.MaxCents.
		Div(centsDiv).
		Div(exchangeRate).
		Mul(decimal.New(1, int32(decimals))). //nolint:gosec
		Floor()

	fee := decimal.Min(feePct, maxInTokens)

	return FeeResult{
		FeeAmount: fee,
		NetAmount: amount.Sub(fee),
	}
}
