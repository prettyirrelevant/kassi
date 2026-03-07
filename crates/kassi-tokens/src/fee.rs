use rust_decimal::Decimal;

/// Fee configuration read from environment.
#[derive(Debug, Clone)]
pub struct FeeConfig {
    /// Fee rate in basis points (e.g. 100 = 1%).
    pub fee_bps: u64,
    /// Optional max fee in USD. `None` means no cap.
    pub fee_cap_usd: Option<Decimal>,
}

/// Result of a fee calculation, all values in the token's smallest unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeResult {
    pub fee_amount: u128,
    pub net_amount: u128,
}

/// Compute the fee for a given deposit amount.
///
/// - `amount`: deposit amount in the token's smallest unit
/// - `config`: fee rate and optional USD cap
/// - `exchange_rate`: USD price per one whole token (e.g. 1.0168 for USDC)
/// - `decimals`: token decimals (e.g. 6 for USDC, 18 for ETH)
#[must_use]
pub fn calculate_fee(
    amount: u128,
    config: &FeeConfig,
    exchange_rate: Decimal,
    decimals: u8,
) -> FeeResult {
    if config.fee_bps == 0 || amount == 0 {
        return FeeResult {
            fee_amount: 0,
            net_amount: amount,
        };
    }

    let fee_before_cap = amount * u128::from(config.fee_bps) / 10_000;

    let fee_amount = match config.fee_cap_usd {
        Some(cap_usd) if !exchange_rate.is_zero() => {
            let one_token = Decimal::from(10_u128.pow(u32::from(decimals)));
            // cap_in_token_units = floor(cap_usd / exchange_rate * one_token)
            let cap_in_token_units = (cap_usd / exchange_rate * one_token)
                .floor()
                .to_string()
                .parse::<u128>()
                .unwrap_or(0);
            fee_before_cap.min(cap_in_token_units)
        }
        _ => fee_before_cap,
    };

    FeeResult {
        fee_amount,
        net_amount: amount - fee_amount,
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    mod calculate_fee {
        use super::*;

        #[test]
        fn basic_one_percent() {
            let result = calculate_fee(
                25_420_000,
                &FeeConfig {
                    fee_bps: 100,
                    fee_cap_usd: None,
                },
                dec("1"),
                6,
            );
            assert_eq!(result.fee_amount, 254_200);
            assert_eq!(result.net_amount, 25_420_000 - 254_200);
        }

        #[test]
        fn fee_with_cap_kicks_in() {
            // 1,000,000 USDC (6 decimals), 1% fee, $500 cap
            let amount = 1_000_000 * 1_000_000_u128;
            let result = calculate_fee(
                amount,
                &FeeConfig {
                    fee_bps: 100,
                    fee_cap_usd: Some(dec("500")),
                },
                dec("1.0168"),
                6,
            );

            // cap_in_token_units = floor(500 / 1.0168 * 1_000_000) = 491_738_788
            assert_eq!(result.fee_amount, 491_738_788);
            assert_eq!(result.net_amount, amount - 491_738_788);
        }

        #[test]
        fn fee_below_cap_uses_percentage() {
            // 100 USDC, 1% fee, $500 cap (cap won't trigger)
            let amount = 100 * 1_000_000_u128;
            let result = calculate_fee(
                amount,
                &FeeConfig {
                    fee_bps: 100,
                    fee_cap_usd: Some(dec("500")),
                },
                dec("1"),
                6,
            );
            assert_eq!(result.fee_amount, 1_000_000);
            assert_eq!(result.net_amount, 99_000_000);
        }

        #[test]
        fn zero_fee_bps_returns_zero_fee() {
            let result = calculate_fee(
                25_420_000,
                &FeeConfig {
                    fee_bps: 0,
                    fee_cap_usd: None,
                },
                dec("1"),
                6,
            );
            assert_eq!(result.fee_amount, 0);
            assert_eq!(result.net_amount, 25_420_000);
        }

        #[test]
        fn zero_amount_returns_zero_fee() {
            let result = calculate_fee(
                0,
                &FeeConfig {
                    fee_bps: 100,
                    fee_cap_usd: None,
                },
                dec("1"),
                6,
            );
            assert_eq!(result.fee_amount, 0);
            assert_eq!(result.net_amount, 0);
        }

        #[test]
        fn no_cap_means_full_percentage() {
            let amount = 1_000_000 * 1_000_000_u128;
            let result = calculate_fee(
                amount,
                &FeeConfig {
                    fee_bps: 100,
                    fee_cap_usd: None,
                },
                dec("1"),
                6,
            );
            // 1% of 1M USDC = 10,000 USDC
            assert_eq!(result.fee_amount, 10_000_000_000);
            assert_eq!(result.net_amount, amount - 10_000_000_000);
        }

        #[test]
        fn cap_with_non_usd_pegged_token() {
            // 10 ETH (18 decimals), 1% fee, $500 cap, ETH at $2000
            let amount = 10 * 10_u128.pow(18);
            let result = calculate_fee(
                amount,
                &FeeConfig {
                    fee_bps: 100,
                    fee_cap_usd: Some(dec("500")),
                },
                dec("2000"),
                18,
            );

            // fee_before_cap = 0.1 ETH = 100_000_000_000_000_000
            // cap_in_token_units = floor(500 / 2000 * 10^18) = 250_000_000_000_000_000 (0.25 ETH)
            // fee = min(0.1, 0.25) = 0.1 ETH (cap doesn't kick in)
            assert_eq!(result.fee_amount, 100_000_000_000_000_000);
        }

        #[test]
        fn cap_kicks_in_for_large_eth_deposit() {
            // 1000 ETH (18 decimals), 1% fee, $500 cap, ETH at $2000
            let amount = 1000 * 10_u128.pow(18);
            let result = calculate_fee(
                amount,
                &FeeConfig {
                    fee_bps: 100,
                    fee_cap_usd: Some(dec("500")),
                },
                dec("2000"),
                18,
            );

            // fee_before_cap = 10 ETH
            // cap_in_token_units = floor(500 / 2000 * 10^18) = 250_000_000_000_000_000
            assert_eq!(result.fee_amount, 250_000_000_000_000_000);
        }

        #[test]
        fn small_amount_truncates_to_zero_fee() {
            // 99 smallest units, 1% fee => floor(99 * 100 / 10000) = 0
            let result = calculate_fee(
                99,
                &FeeConfig {
                    fee_bps: 100,
                    fee_cap_usd: None,
                },
                dec("1"),
                6,
            );
            assert_eq!(result.fee_amount, 0);
            assert_eq!(result.net_amount, 99);
        }

        #[test]
        fn fee_plus_net_equals_amount() {
            let amount = 123_456_789_u128;
            let result = calculate_fee(
                amount,
                &FeeConfig {
                    fee_bps: 250,
                    fee_cap_usd: None,
                },
                dec("1"),
                6,
            );
            assert_eq!(result.fee_amount + result.net_amount, amount);
        }
    }
}
