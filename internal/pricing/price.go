package pricing

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/imroc/req/v3"
	"github.com/shopspring/decimal"
)

// Oracle fetches the current fiat price for a given asset.
type Oracle interface {
	FetchPrice(ctx context.Context, coingeckoID, fiatCurrency string) (decimal.Decimal, error)
}

// LiveOracle implements Oracle. Tries DefiLlama first, falls back to CoinGecko.
type LiveOracle struct {
	client *req.Client
}

func NewLiveOracle() *LiveOracle {
	return &LiveOracle{
		client: req.C().
			SetTimeout(10 * time.Second).
			SetUserAgent("kassi").
			OnAfterResponse(func(_ *req.Client, resp *req.Response) error {
				if !resp.IsSuccessState() {
					resp.Err = fmt.Errorf("status %s: %s", resp.Status, resp.String())
				}
				return nil
			}),
	}
}

func (o *LiveOracle) FetchPrice(ctx context.Context, coingeckoID, fiatCurrency string) (decimal.Decimal, error) {
	price, err := o.fetchFromDefiLlama(ctx, coingeckoID)
	if err == nil {
		return price, nil
	}

	price, fallbackErr := o.fetchFromCoinGecko(ctx, coingeckoID, fiatCurrency)
	if fallbackErr != nil {
		return decimal.Zero, fmt.Errorf("defillama: %w, coingecko: %w", err, fallbackErr)
	}

	return price, nil
}

type defiLlamaResponse struct {
	Coins map[string]struct {
		Price decimal.Decimal `json:"price"`
	} `json:"coins"`
}

func (o *LiveOracle) fetchFromDefiLlama(ctx context.Context, coingeckoID string) (decimal.Decimal, error) {
	var result defiLlamaResponse
	_, err := o.client.R().
		SetContext(ctx).
		SetSuccessResult(&result).
		SetPathParam("id", coingeckoID).
		Get("https://coins.llama.fi/prices/current/coingecko:{id}")
	if err != nil {
		return decimal.Zero, err
	}

	coin, ok := result.Coins["coingecko:"+coingeckoID]
	if !ok {
		return decimal.Zero, fmt.Errorf("price not found for coingecko:%s", coingeckoID)
	}

	return coin.Price, nil
}

func (o *LiveOracle) fetchFromCoinGecko(ctx context.Context, id, fiatCurrency string) (decimal.Decimal, error) {
	currency := strings.ToLower(fiatCurrency)

	var result map[string]map[string]decimal.Decimal
	_, err := o.client.R().
		SetContext(ctx).
		SetSuccessResult(&result).
		SetQueryParams(map[string]string{
			"ids":           id,
			"vs_currencies": currency,
		}).
		Get("https://api.coingecko.com/api/v3/simple/price")
	if err != nil {
		return decimal.Zero, err
	}

	price, ok := result[id][currency]
	if !ok {
		return decimal.Zero, fmt.Errorf("price not found for %s/%s", id, currency)
	}

	return price, nil
}
