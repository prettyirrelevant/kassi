package pricing

import (
	"testing"

	"github.com/shopspring/decimal"
	"github.com/stretchr/testify/require"
)

func d(v string) decimal.Decimal {
	return decimal.RequireFromString(v)
}

func TestCalculateFee(t *testing.T) {
	// BPS=50 (0.5%), MaxCents=2500 ($25), USDC 6 decimals, 1:1 rate
	config := FeeConfig{BPS: d("50"), MaxCents: d("2500")}
	rate := d("1")

	tests := []struct {
		name     string
		amount   string
		decimals int
		wantFee  string
		wantNet  string
	}{
		{
			name:     "small deposit pays percentage",
			amount:   "100000000", // 100 USDC
			decimals: 6,
			wantFee:  "500000",   // 0.50 USDC
			wantNet:  "99500000", // 99.50 USDC
		},
		{
			name:     "large deposit hits fee cap",
			amount:   "10000000000", // 10,000 USDC
			decimals: 6,
			wantFee:  "25000000",   // 25 USDC (capped)
			wantNet:  "9975000000", // 9,975 USDC
		},
		{
			name:     "very large deposit still capped",
			amount:   "1000000000000", // 1,000,000 USDC
			decimals: 6,
			wantFee:  "25000000",     // 25 USDC (capped)
			wantNet:  "999975000000", // 999,975 USDC
		},
		{
			name:     "zero amount returns zero fee",
			amount:   "0",
			decimals: 6,
			wantFee:  "0",
			wantNet:  "0",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := CalculateFee(d(tt.amount), config, rate, tt.decimals)
			require.True(t, result.FeeAmount.Equal(d(tt.wantFee)), "fee: got %s, want %s", result.FeeAmount, tt.wantFee)
			require.True(t, result.NetAmount.Equal(d(tt.wantNet)), "net: got %s, want %s", result.NetAmount, tt.wantNet)
		})
	}
}

func TestCalculateFee_ZeroBPS(t *testing.T) {
	config := FeeConfig{BPS: d("0"), MaxCents: d("2500")}
	result := CalculateFee(d("100000000"), config, d("1"), 6)
	require.True(t, result.FeeAmount.IsZero(), "expected zero fee with BPS=0, got %s", result.FeeAmount)
	require.True(t, result.NetAmount.Equal(d("100000000")), "expected full amount as net, got %s", result.NetAmount)
}

func TestCalculateFee_NonUSDRate(t *testing.T) {
	// ETH at $3245.50, 18 decimals
	config := FeeConfig{BPS: d("50"), MaxCents: d("2500")}
	rate := d("3245.50")
	amount := d("1000000000000000000") // 1 ETH in wei

	result := CalculateFee(amount, config, rate, 18)

	// fee_pct = floor(1e18 * 50 / 10000) = 5e15 (0.005 ETH = ~$16.23)
	// max_in_tokens = floor(2500 / 100 / 3245.50 * 1e18) = floor(7.703..e15) = 7703...
	// fee = min(5e15, 7.7e15) = 5e15
	require.True(t, result.FeeAmount.Equal(d("5000000000000000")), "fee: got %s, want 5000000000000000", result.FeeAmount)
}

func FuzzCalculateFee(f *testing.F) {
	f.Add(uint64(100000000), uint64(50), uint64(2500), uint64(100), 6)
	f.Add(uint64(1), uint64(1), uint64(1), uint64(1), 0)
	f.Add(uint64(1000000000000), uint64(10000), uint64(100000), uint64(324550), 18)

	f.Fuzz(func(t *testing.T, amount, bps, maxCents, rateCents uint64, decimals int) {
		if amount == 0 || bps == 0 || maxCents == 0 || rateCents == 0 {
			return
		}
		if decimals < 0 || decimals > 18 || bps > 10000 {
			return
		}

		amt := decimal.NewFromUint64(amount)
		config := FeeConfig{
			BPS:      decimal.NewFromUint64(bps),
			MaxCents: decimal.NewFromUint64(maxCents),
		}
		rate := decimal.NewFromUint64(rateCents).Div(decimal.NewFromInt(100))

		result := CalculateFee(amt, config, rate, decimals)

		require.True(t, result.FeeAmount.Add(result.NetAmount).Equal(amt), "fee(%s) + net(%s) != amount(%s)", result.FeeAmount, result.NetAmount, amt)
		require.False(t, result.FeeAmount.IsNegative(), "negative fee: %s", result.FeeAmount)
		require.True(t, result.FeeAmount.LessThanOrEqual(amt), "fee %s exceeds amount %s", result.FeeAmount, amt)
	})
}
