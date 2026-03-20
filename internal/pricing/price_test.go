package pricing

import (
	"testing"

	"github.com/jarcoal/httpmock"
	"github.com/stretchr/testify/require"
)

func TestLiveOracle_FetchPrice(t *testing.T) {
	oracle := NewLiveOracle()
	httpmock.ActivateNonDefault(oracle.client.GetClient())
	defer httpmock.DeactivateAndReset()

	t.Run("defillama success", func(t *testing.T) {
		httpmock.Reset()
		httpmock.RegisterResponder("GET", "https://coins.llama.fi/prices/current/coingecko:ethereum",
			httpmock.NewJsonResponderOrPanic(200, map[string]any{
				"coins": map[string]any{
					"coingecko:ethereum": map[string]any{
						"price": 3245.50,
					},
				},
			}),
		)

		price, err := oracle.FetchPrice(t.Context(), "ethereum", "usd")
		require.NoError(t, err)
		require.Equal(t, "3245.5", price.String())
	})

	t.Run("defillama fails, coingecko fallback", func(t *testing.T) {
		httpmock.Reset()
		httpmock.RegisterResponder("GET", "https://coins.llama.fi/prices/current/coingecko:ethereum",
			httpmock.NewStringResponder(500, "internal error"),
		)
		httpmock.RegisterResponder("GET", "https://api.coingecko.com/api/v3/simple/price",
			httpmock.NewJsonResponderOrPanic(200, map[string]any{
				"ethereum": map[string]any{
					"usd": 3200.00,
				},
			}),
		)

		price, err := oracle.FetchPrice(t.Context(), "ethereum", "USD")
		require.NoError(t, err)
		require.Equal(t, "3200", price.String())
	})

	t.Run("both providers fail", func(t *testing.T) {
		httpmock.Reset()
		httpmock.RegisterResponder("GET", "https://coins.llama.fi/prices/current/coingecko:ethereum",
			httpmock.NewStringResponder(500, "error"),
		)
		httpmock.RegisterResponder("GET", "https://api.coingecko.com/api/v3/simple/price",
			httpmock.NewStringResponder(429, "rate limited"),
		)

		_, err := oracle.FetchPrice(t.Context(), "ethereum", "USD")
		require.Error(t, err)
		require.Contains(t, err.Error(), "defillama")
		require.Contains(t, err.Error(), "coingecko")
	})

	t.Run("defillama missing coin falls back to coingecko", func(t *testing.T) {
		httpmock.Reset()
		httpmock.RegisterResponder("GET", "https://coins.llama.fi/prices/current/coingecko:fakecoin",
			httpmock.NewJsonResponderOrPanic(200, map[string]any{
				"coins": map[string]any{},
			}),
		)
		httpmock.RegisterResponder("GET", "https://api.coingecko.com/api/v3/simple/price",
			httpmock.NewJsonResponderOrPanic(200, map[string]any{
				"fakecoin": map[string]any{
					"usd": 1.5,
				},
			}),
		)

		price, err := oracle.FetchPrice(t.Context(), "fakecoin", "USD")
		require.NoError(t, err)
		require.Equal(t, "1.5", price.String())
	})

	t.Run("coingecko missing currency", func(t *testing.T) {
		httpmock.Reset()
		httpmock.RegisterResponder("GET", "https://coins.llama.fi/prices/current/coingecko:ethereum",
			httpmock.NewStringResponder(500, "error"),
		)
		httpmock.RegisterResponder("GET", "https://api.coingecko.com/api/v3/simple/price",
			httpmock.NewJsonResponderOrPanic(200, map[string]any{
				"ethereum": map[string]any{},
			}),
		)

		_, err := oracle.FetchPrice(t.Context(), "ethereum", "EUR")
		require.Error(t, err)
	})
}
