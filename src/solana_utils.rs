// Placeholder for Solana utility functions

use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentLevel, // For checking confirmation status
    hash::Hash,
    pubkey::Pubkey,
    signature::Signature,
    signer::keypair::{Keypair, read_keypair_file},
};
use std::path::PathBuf;
use std::time::{Duration as StdDuration, SystemTime}; // Renamed to avoid conflict with tokio::time::Duration
use tokio::time::{Duration as TokioDuration, sleep};

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

// We need to add the `dirs`
