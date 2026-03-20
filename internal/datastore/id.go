package datastore

import "github.com/rs/xid"

const (
	prefixMerchant              = "mer_"
	prefixMerchantConfig        = "mcfg_"
	prefixSettlementDestination = "sdst_"
	prefixSigner                = "sig_"
	prefixAsset                 = "ast_"
	prefixDepositAddress        = "dep_"
	prefixNetworkAddress        = "nadr_"
	prefixPaymentIntent         = "pi_"
	prefixLedgerEntry           = "le_"
	prefixWebhookDelivery       = "whd_"
	prefixJob                   = "job_"
)

func newID(prefix string) string {
	return prefix + xid.New().String()
}

func NewMerchantID() string              { return newID(prefixMerchant) }
func NewMerchantConfigID() string        { return newID(prefixMerchantConfig) }
func NewSettlementDestinationID() string { return newID(prefixSettlementDestination) }
func NewSignerID() string                { return newID(prefixSigner) }
func NewAssetID() string                 { return newID(prefixAsset) }
func NewDepositAddressID() string        { return newID(prefixDepositAddress) }
func NewNetworkAddressID() string        { return newID(prefixNetworkAddress) }
func NewPaymentIntentID() string         { return newID(prefixPaymentIntent) }
func NewLedgerEntryID() string           { return newID(prefixLedgerEntry) }
func NewWebhookDeliveryID() string       { return newID(prefixWebhookDelivery) }
func NewJobID() string                   { return newID(prefixJob) }
