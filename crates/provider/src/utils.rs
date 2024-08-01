//! Provider-related utilities.

use crate::ProviderCall;
use alloy_json_rpc::{RpcError, RpcParam, RpcReturn};
use alloy_primitives::{U128, U64};
use alloy_rpc_client::WeakClient;
use alloy_rpc_types_eth::BlockId;
use alloy_transport::{Transport, TransportResult};
use std::borrow::Cow;
/// The number of blocks from the past for which the fee rewards are fetched for fee estimation.
pub const EIP1559_FEE_ESTIMATION_PAST_BLOCKS: u64 = 10;
/// Multiplier for the current base fee to estimate max base fee for the next block.
pub const EIP1559_BASE_FEE_MULTIPLIER: u128 = 2;
/// The default percentile of gas premiums that are fetched for fee estimation.
pub const EIP1559_FEE_ESTIMATION_REWARD_PERCENTILE: f64 = 20.0;
/// The minimum priority fee to provide.
pub const EIP1559_MIN_PRIORITY_FEE: u128 = 1;

/// An estimator function for EIP1559 fees.
pub type EstimatorFunction = fn(u128, &[Vec<u128>]) -> Eip1559Estimation;

/// Return type of EIP1155 gas fee estimator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Eip1559Estimation {
    /// The base fee per gas.
    pub max_fee_per_gas: u128,
    /// The max priority fee per gas.
    pub max_priority_fee_per_gas: u128,
}

fn estimate_priority_fee(rewards: &[Vec<u128>]) -> u128 {
    let mut rewards =
        rewards.iter().filter_map(|r| r.first()).filter(|r| **r > 0_u128).collect::<Vec<_>>();
    if rewards.is_empty() {
        return EIP1559_MIN_PRIORITY_FEE;
    }

    rewards.sort_unstable();

    let n = rewards.len();

    let median =
        if n % 2 == 0 { (*rewards[n / 2 - 1] + *rewards[n / 2]) / 2 } else { *rewards[n / 2] };

    std::cmp::max(median, EIP1559_MIN_PRIORITY_FEE)
}

/// The default EIP-1559 fee estimator which is based on the work by [MetaMask](https://github.com/MetaMask/core/blob/main/packages/gas-fee-controller/src/fetchGasEstimatesViaEthFeeHistory/calculateGasFeeEstimatesForPriorityLevels.ts#L56)
/// (constants for "medium" priority level are used)
pub fn eip1559_default_estimator(
    base_fee_per_gas: u128,
    rewards: &[Vec<u128>],
) -> Eip1559Estimation {
    let max_priority_fee_per_gas = estimate_priority_fee(rewards);
    let potential_max_fee = base_fee_per_gas * EIP1559_BASE_FEE_MULTIPLIER;

    Eip1559Estimation {
        max_fee_per_gas: potential_max_fee + max_priority_fee_per_gas,
        max_priority_fee_per_gas,
    }
}

/// Convert `U128` to `u128`.
pub(crate) fn convert_u128(r: U128) -> u128 {
    r.to::<u128>()
}

pub(crate) fn convert_u64(r: U64) -> u64 {
    r.to::<u64>()
}

/// Into ProviderCall::RpcCall
///
/// Note: This function is only used to converted to ProviderCall::RpcCall and not any other
/// variant. Hence, client should always be Some.
pub fn into_prov_rpc_call<T: Transport + Clone, Params: RpcParam, Resp: RpcReturn>(
    method: Cow<'static, str>,
    params: Params,
    block_id: BlockId,
    client: Option<WeakClient<T>>,
) -> TransportResult<ProviderCall<T, Params, Resp>> {
    // serialize the params
    let mut ser = serde_json::to_value(params.clone()).map_err(RpcError::ser_err)?;

    // serialize the block id
    let block_id = serde_json::to_value(block_id).map_err(RpcError::ser_err)?;

    // append the block id to the params
    if let serde_json::Value::Array(ref mut arr) = ser {
        arr.push(block_id);
    } else if ser.is_null() {
        ser = serde_json::Value::Array(vec![block_id]);
    } else {
        ser = serde_json::Value::Array(vec![ser, block_id]);
    };

    println!("Serialized Params {:#?}", ser);
    client.map_or_else(
        || unreachable!("WeakClient is None"),
        |client| {
            let client = client.upgrade().unwrap();

            let rpc_call = client.request(method, params); // TODO: params should be ser. However using `ser` will throw type mismatch.

            Ok(ProviderCall::from(rpc_call))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;

    #[test]
    fn test_estimate_priority_fee() {
        let rewards =
            vec![vec![10_000_000_000_u128], vec![200_000_000_000_u128], vec![3_000_000_000_u128]];
        assert_eq!(super::estimate_priority_fee(&rewards), 10_000_000_000_u128);

        let rewards = vec![
            vec![400_000_000_000_u128],
            vec![2_000_000_000_u128],
            vec![5_000_000_000_u128],
            vec![3_000_000_000_u128],
        ];

        assert_eq!(super::estimate_priority_fee(&rewards), 4_000_000_000_u128);

        let rewards = vec![vec![0_u128], vec![0_u128], vec![0_u128]];

        assert_eq!(super::estimate_priority_fee(&rewards), EIP1559_MIN_PRIORITY_FEE);

        assert_eq!(super::estimate_priority_fee(&[]), EIP1559_MIN_PRIORITY_FEE);
    }

    #[test]
    fn test_eip1559_default_estimator() {
        let base_fee_per_gas = 1_000_000_000_u128;
        let rewards = vec![
            vec![200_000_000_000_u128],
            vec![200_000_000_000_u128],
            vec![300_000_000_000_u128],
        ];
        assert_eq!(
            super::eip1559_default_estimator(base_fee_per_gas, &rewards),
            Eip1559Estimation {
                max_fee_per_gas: 202_000_000_000_u128,
                max_priority_fee_per_gas: 200_000_000_000_u128
            }
        );

        let base_fee_per_gas = 0u128;
        let rewards = vec![
            vec![200_000_000_000_u128],
            vec![200_000_000_000_u128],
            vec![300_000_000_000_u128],
        ];

        assert_eq!(
            super::eip1559_default_estimator(base_fee_per_gas, &rewards),
            Eip1559Estimation {
                max_fee_per_gas: 200_000_000_000_u128,
                max_priority_fee_per_gas: 200_000_000_000_u128
            }
        );
    }
}
