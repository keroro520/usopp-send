// Placeholder for Solana utility functions

use crate::{Error, Result};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    commitment_config::CommitmentLevel, // For checking confirmation status
    hash::Hash,
    native_token::lamports_to_sol,
    pubkey::Pubkey,
    signature::Signature,
    signer::keypair::{read_keypair_file, Keypair},
};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration as StdDuration, SystemTime}; // Renamed to avoid conflict with tokio::time::Duration
use tokio::time::{sleep, Duration as TokioDuration};

/// Loads a Solana Keypair from a JSON file path.
/// Handles tilde expansion for the home directory.
pub fn load_keypair_from_file(path_str: &str) -> Result<Keypair, String> {
    let path = if path_str.starts_with('~') {
        match dirs::home_dir() {
            Some(mut home) => {
                home.push(&path_str[2..]); // Skip "~/"
                home
            }
            None => return Err("Failed to resolve home directory".to_string()),
        }
    } else {
        PathBuf::from(path_str)
    };

    read_keypair_file(&path)
        .map_err(|e| format!("Failed to read keypair from file {}: {}", path.display(), e))
}

/// Fetches the balance of a given Solana account.
pub fn get_account_balance(client: &RpcClient, pubkey: &Pubkey) -> Result<u64, String> {
    client
        .get_balance(pubkey)
        .map_err(|e| format!("Failed to get balance for pubkey {}: {}", pubkey, e))
}

/// Fetches the latest blockhash from the Solana network.
pub fn get_latest_blockhash(client: &RpcClient) -> Result<Hash, String> {
    client
        .get_latest_blockhash()
        .map_err(|e| format!("Failed to get latest blockhash: {}", e))
}

#[derive(Debug, Clone)] // Added Clone
pub struct MonitoredTxInfo {
    pub signature: Signature,
    pub send_time: SystemTime,
    pub rpc_url_sent_via: String, // Which RPC was it sent through
}

#[derive(Debug)]
pub struct WinningTxResult {
    pub signature: Signature,
    pub rpc_url_sent_via: String,
    pub latency: StdDuration,
    pub slot: u64,
}

const POLLING_INTERVAL_MS: u64 = 500;
const MONITORING_TIMEOUT_S: u64 = 60; // 60 seconds timeout for monitoring

pub async fn monitor_transaction_statuses(
    monitoring_client: &RpcClient, // A single client for consistent status checks
    submitted_txs: Vec<MonitoredTxInfo>,
) -> Option<WinningTxResult> {
    // Returns the first confirmed transaction
    if submitted_txs.is_empty() {
        return None;
    }
    println!(
        "Monitoring {} submitted transactions for confirmation...",
        submitted_txs.len()
    );

    let start_monitoring_time = SystemTime::now();
    let signatures_to_check: Vec<Signature> = submitted_txs.iter().map(|tx| tx.signature).collect();

    loop {
        // Check for overall timeout
        if start_monitoring_time
            .elapsed()
            .unwrap_or_default()
            .as_secs()
            >= MONITORING_TIMEOUT_S
        {
            println!(
                "Monitoring timeout reached after {} seconds.",
                MONITORING_TIMEOUT_S
            );
            return None;
        }

        match monitoring_client.get_signature_statuses(&signatures_to_check) {
            Ok(rpc_response) => {
                for (index, status_option) in rpc_response.value.iter().enumerate() {
                    if let Some(status) = status_option {
                        // Check for desired confirmation level
                        // CommitmentLevel::Confirmed or CommitmentLevel::Finalized
                        // status.confirmations might be None if not yet confirmed past Processed,
                        // or it might be the number of blocks confirmed.
                        // A slot is usually present if it's at least processed.
                        if status.satisfies_commitment(monitoring_client.commitment())
                            && status.err.is_none()
                        {
                            // Let's consider it confirmed if it meets the client's commitment level (e.g. Confirmed)
                            // and has no transaction error.
                            // The spec mentions "confirmed" or higher. RpcClient default is Confirmed.
                            let confirmed_tx_info = &submitted_txs[index];
                            let confirmation_time = SystemTime::now();
                            let latency = confirmation_time
                                .duration_since(confirmed_tx_info.send_time)
                                .unwrap_or_else(|_| StdDuration::from_secs(0)); // Handle potential clock skew

                            println!(
                                "Transaction {} confirmed first via {}! Latency: {:?}, Slot: {}",
                                confirmed_tx_info.signature,
                                confirmed_tx_info.rpc_url_sent_via,
                                latency,
                                status.slot
                            );
                            return Some(WinningTxResult {
                                signature: confirmed_tx_info.signature,
                                rpc_url_sent_via: confirmed_tx_info.rpc_url_sent_via.clone(),
                                latency,
                                slot: status.slot,
                            });
                        }
                    } // else: status is None, means transaction not found or not yet processed to a queryable state.
                }
            }
            Err(e) => {
                eprintln!("Error fetching signature statuses: {}. Retrying...", e);
                // Decide on retry strategy or if this is fatal for this polling attempt
            }
        }
        sleep(TokioDuration::from_millis(POLLING_INTERVAL_MS)).await;
    }
}

pub fn load_keypair(path: &str) -> Result<Keypair> {
    let path_expanded = shellexpand::tilde(path).into_owned();
    let keypair_path = PathBuf::from(path_expanded);
    if !keypair_path.exists() {
        return Err(Error::KeypairLoad(format!(
            "Keypair file not found: {}",
            keypair_path.display()
        )));
    }
    Keypair::read_from_file(&keypair_path).map_err(|e| {
        Error::KeypairLoad(format!(
            "Failed to read keypair from file {}: {}",
            keypair_path.display(),
            e
        ))
    })
}

pub fn get_balance(client: &RpcClient, pubkey: &Pubkey) -> Result<u64> {
    client
        .get_balance_with_commitment(pubkey, CommitmentConfig::confirmed())
        .map(|res| res.value)
        .map_err(Error::SolanaClient)
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AccountRole {
    pubkey: Pubkey,
    balance: u64,
}

impl AccountRole {
    pub fn new(pubkey: Pubkey, balance: u64) -> Self {
        Self { pubkey, balance }
    }

    pub fn pubkey(&self) -> &Pubkey {
        &self.pubkey
    }

    pub fn balance_lamports(&self) -> u64 {
        self.balance
    }

    pub fn balance_sol(&self) -> f64 {
        lamports_to_sol(self.balance)
    }
}

pub fn determine_account_roles(
    client: &RpcClient,
    keypair1: &Keypair,
    keypair2: &Keypair,
) -> Result<(AccountRole, AccountRole)> {
    let pubkey1 = keypair1.pubkey();
    let pubkey2 = keypair2.pubkey();

    let balance1 = get_balance(client, &pubkey1)?;
    let balance2 = get_balance(client, &pubkey2)?;

    log::info!(
        "Account 1 ({}): {:.6} SOL",
        pubkey1,
        lamports_to_sol(balance1)
    );
    log::info!(
        "Account 2 ({}): {:.6} SOL",
        pubkey2,
        lamports_to_sol(balance2)
    );

    let acc_role1 = AccountRole::new(pubkey1, balance1);
    let acc_role2 = AccountRole::new(pubkey2, balance2);

    if balance1 >= balance2 {
        log::info!("Account 1 has higher or equal balance. Setting as Sender.");
        Ok((acc_role1, acc_role2)) // (sender, recipient)
    } else {
        log::info!("Account 2 has higher balance. Setting as Sender.");
        Ok((acc_role2, acc_role1)) // (sender, recipient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signature::Keypair;
    use std::io::Write;
    use std::path::Path;
    use tempfile::NamedTempFile;

    // Mock RpcClient for testing purposes
    struct MockRpcClient;

    // This is a simplified mock. In a real scenario, you might use a crate like `mockall`.
    impl MockRpcClient {
        fn get_balance_with_commitment(
            &self,
            pubkey: &Pubkey,
            _commitment: CommitmentConfig,
        ) -> std::result::Result<
            solana_client::rpc_response::RpcResponse<u64>,
            solana_client::client_error::ClientError,
        > {
            // Simulate different balances for different pubkeys for role determination test
            let key1 = Keypair::new().pubkey(); // Doesn't matter as we won't compare exact pubkeys in mock
            let key2 = Keypair::new().pubkey();

            let balance = if pubkey.to_string().len() % 2 == 0 {
                // Arbitrary condition
                100_000_000 // 0.1 SOL
            } else {
                200_000_000 // 0.2 SOL
            };
            // Construct a dummy RpcResponseContext
            let ctx = solana_client::rpc_response::RpcResponseContext {
                slot: 1,
                api_version: None,
            };
            Ok(solana_client::rpc_response::RpcResponse {
                context: ctx,
                value: balance,
            })
        }
    }
    // We need to implement the RpcClient trait for our mock, or adapt tests.
    // For simplicity here, we'll assume `get_balance` can take our MockRpcClient if we adjust its signature
    // or we can create a new function for testing `determine_account_roles_with_mock`.
    // For now, let's assume we can't directly use MockRpcClient with the existing get_balance signature.
    // So, `determine_account_roles` test will be harder to write without a more complex mock or a real RPC endpoint.

    #[test]
    fn test_load_keypair_success() {
        let keypair = Keypair::new();
        let mut temp_file = NamedTempFile::new().unwrap();
        let keypair_bytes = keypair.to_bytes();
        // Write as JSON array of numbers
        let json_array = format!(
            "[{}]
",
            keypair_bytes
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<String>>()
                .join(",")
        );
        temp_file.write_all(json_array.as_bytes()).unwrap();

        let loaded_keypair = load_keypair(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(loaded_keypair.pubkey(), keypair.pubkey());
    }

    #[test]
    fn test_load_keypair_file_not_found() {
        let result = load_keypair("non_existent_keypair.json");
        assert!(matches!(result, Err(Error::KeypairLoad(_))));
        if let Err(Error::KeypairLoad(msg)) = result {
            assert!(msg.contains("Keypair file not found"));
        }
    }

    #[test]
    fn test_load_keypair_invalid_format() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"invalid content").unwrap();
        let result = load_keypair(temp_file.path().to_str().unwrap());
        assert!(matches!(result, Err(Error::KeypairLoad(_))));
        if let Err(Error::KeypairLoad(msg)) = result {
            assert!(msg.contains("Failed to read keypair from file"));
        }
    }

    // Note: Testing `get_balance` and `determine_account_roles` properly requires a running Solana test validator
    // or a more sophisticated mocking setup for RpcClient.
    // The mock provided above is a starting point but not used in a test here due to complexity
    // of injecting it into the current `get_balance` function signature.
}

// We need to add the `dirs`
