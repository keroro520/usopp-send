use crate::accounts::AccountInfo;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_response::RpcSimulateTransactionResult;
use solana_sdk::{
    message::Message, signature::Signature, system_instruction, transaction::Transaction,
};
use std::{error::Error, time::Instant};

// Minimum balance to leave in sender's account after a transaction, in lamports.
// This should cover potential future transaction fees or leave a small dust amount.
const MIN_SENDER_RESERVE_LAMPORTS: u64 = 5_000; // Default rent-exempt minimum + a bit

/// Represents a signed transaction ready to be sent to a specific RPC node.
#[derive(Debug)]
pub struct PreparedTransaction {
    pub rpc_url: String,
    pub transaction: Transaction,
    pub signature: Signature, // Base58 encoded string
    pub amount_lamports: u64,
}

/// Holds the result of a single transaction send attempt.
#[derive(Debug, Clone)] // Added Clone for use in monitoring
pub struct SendAttempt {
    pub rpc_url: String,
    pub original_signature: Signature, // The signature of the transaction we attempted to send
    pub amount_lamports: u64,
    pub send_result: Result<Signature, String>, // Ok(signature_returned_by_rpc) or Err(error_message)
    pub send_start_instant: Instant,            // When this specific send operation began
    pub send_duration_ms: u128,                 // Duration of the send_transaction RPC call
}

/// Holds the result of a single transaction simulation attempt.
#[derive(Debug)]
pub struct SimulationAttempt {
    pub rpc_url: String,
    pub original_signature: Signature,
    pub amount_lamports: u64,
    pub simulation_result: Result<RpcSimulateTransactionResult, String>, // Ok contains logs, units consumed etc.
    pub simulation_duration_ms: u128,
}

/// Constructs `n` conflicting transfer transactions.
/// `n` is determined by the number of `rpc_urls`.
/// Each transaction attempts to send a decreasing percentage of the sender's balance.
pub fn construct_conflicting_transactions(
    sender_account: &AccountInfo,
    recipient_account: &AccountInfo,
    rpc_urls: &[String],
    rpc_client: &RpcClient, // Pass in the RpcClient for fetching blockhash
) -> Result<Vec<PreparedTransaction>, Box<dyn Error>> {
    if rpc_urls.is_empty() {
        return Err("No RPC URLs provided for transaction construction.".into());
    }
    if sender_account.balance <= MIN_SENDER_RESERVE_LAMPORTS {
        return Err(format!(
            "Sender balance ({} lamports) is too low. Must be > {} lamports to construct transactions.",
            sender_account.balance,
            MIN_SENDER_RESERVE_LAMPORTS
        ).into());
    }

    println!("Fetching a recent blockhash...");
    let recent_blockhash = rpc_client.get_latest_blockhash()?;
    println!("Using blockhash: {}", recent_blockhash);

    let mut prepared_transactions = Vec::new();

    let max_transferable_balance = sender_account
        .balance
        .saturating_sub(MIN_SENDER_RESERVE_LAMPORTS);

    for (i, rpc_url) in rpc_urls.iter().enumerate() {
        let percentage = 0.90 - (0.01 * i as f64);
        if percentage <= 0.0 {
            println!(
                "Skipping transaction {} as percentage ({:.2}%) is too low or negative.",
                i,
                percentage * 100.0
            );
            continue;
        }

        let amount_lamports = (max_transferable_balance as f64 * percentage) as u64;

        if amount_lamports == 0 {
            println!(
                "Skipping transaction {} for RPC {} as calculated amount is 0 lamports (percentage: {:.2}% of {} available lamports).",
                i, rpc_url, percentage * 100.0, max_transferable_balance
            );
            continue;
        }

        println!(
            "Constructing transaction {} for RPC: {}. Amount: {} lamports ({:.2}% of available {} lamports).",
            i,
            rpc_url,
            amount_lamports,
            percentage * 100.0,
            max_transferable_balance
        );

        let transfer_instruction = system_instruction::transfer(
            &sender_account.pubkey,
            &recipient_account.pubkey,
            amount_lamports,
        );

        let message = Message::new(&[transfer_instruction], Some(&sender_account.pubkey));
        let mut transaction = Transaction::new_unsigned(message);

        transaction.try_sign(&[&sender_account.keypair], recent_blockhash)?;
        let signature = transaction.signatures[0];

        prepared_transactions.push(PreparedTransaction {
            rpc_url: rpc_url.clone(),
            transaction,
            signature,
            amount_lamports,
        });
    }

    if prepared_transactions.is_empty() && !rpc_urls.is_empty() {
        return Err("Failed to construct any transactions. Sender balance might be too low or percentages resulted in zero amounts.".into());
    }

    Ok(prepared_transactions)
}

/// Asynchronously sends a list of prepared transactions to their respective RPC URLs.
pub async fn send_transactions_concurrently(
    prepared_transactions: Vec<PreparedTransaction>,
) -> Vec<SendAttempt> {
    if prepared_transactions.is_empty() {
        println!("No transactions to send.");
        return Vec::new();
    }

    let mut send_tasks = Vec::new();
    println!(
        "Starting to send {} transactions concurrently...",
        prepared_transactions.len()
    );

    for prep_tx in prepared_transactions {
        let task = tokio::spawn(async move {
            println!(
                "Preparing to send Tx (sig: {}) to RPC: {}",
                prep_tx.signature, prep_tx.rpc_url
            );
            let rpc_client = RpcClient::new(prep_tx.rpc_url.clone());
            let start_time = Instant::now();

            let result = rpc_client.send_transaction(&prep_tx.transaction);
            let duration = start_time.elapsed();

            let send_result = match result {
                Ok(returned_signature) => {
                    println!(
                        "Successfully sent Tx (original sig: {}) via RPC: {}. Returned sig: {}. Time: {}ms",
                        prep_tx.signature,
                        prep_tx.rpc_url,
                        returned_signature,
                        duration.as_millis()
                    );
                    Ok(returned_signature)
                }
                Err(e) => {
                    eprintln!(
                        "Error sending Tx (original sig: {}) via RPC: {}. Error: {}. Time: {}ms",
                        prep_tx.signature,
                        prep_tx.rpc_url,
                        e,
                        duration.as_millis()
                    );
                    Err(e.to_string())
                }
            };

            SendAttempt {
                rpc_url: prep_tx.rpc_url,
                original_signature: prep_tx.signature,
                amount_lamports: prep_tx.amount_lamports,
                send_result,
                send_start_instant: start_time,
                send_duration_ms: duration.as_millis(),
            }
        });
        send_tasks.push(task);
    }

    let mut send_attempts = Vec::new();
    for task in send_tasks {
        match task.await {
            Ok(attempt) => send_attempts.push(attempt),
            Err(e) => {
                eprintln!("Tokio task for send_transaction failed (JoinError): {}", e);
            }
        }
    }
    send_attempts
}

/// Asynchronously simulates a list of prepared transactions on their respective RPC URLs.
pub async fn simulate_transactions_concurrently(
    prepared_transactions: Vec<PreparedTransaction>,
) -> Vec<SimulationAttempt> {
    if prepared_transactions.is_empty() {
        println!("No transactions to simulate.");
        return Vec::new();
    }

    let mut simulation_tasks = Vec::new();
    println!(
        "Starting to simulate {} transactions concurrently...",
        prepared_transactions.len()
    );

    for prep_tx in prepared_transactions {
        let task = tokio::spawn(async move {
            println!(
                "Preparing to simulate Tx (sig: {}) on RPC: {}",
                prep_tx.signature, prep_tx.rpc_url
            );
            let rpc_client = RpcClient::new(prep_tx.rpc_url.clone());
            let start_time = Instant::now();

            let result = rpc_client.simulate_transaction(&prep_tx.transaction);
            let duration = start_time.elapsed();

            let (simulation_status_output_str, final_sim_result_for_struct) = match result {
                Ok(response) => {
                    // Access fields through response.value
                    let sim_value = response.value;
                    let output = if sim_value.err.is_some() {
                        format!(
                            "Simulation FAILED. Error: {:?}, Logs: {:?}, Units: {:?}",
                            sim_value.err,
                            sim_value.logs.as_deref().unwrap_or_default(),
                            sim_value.units_consumed
                        )
                    } else {
                        format!(
                            "Simulation SUCCEEDED. Logs: {:?}, Units: {:?}",
                            sim_value.logs.as_deref().unwrap_or_default(),
                            sim_value.units_consumed
                        )
                    };
                    (output, Ok(sim_value))
                }
                Err(e) => (
                    format!("RPC Error during simulation: {}", e),
                    Err(e.to_string()),
                ),
            };

            println!(
                "Tx (original sig: {}) on RPC {}: {}. Time: {}ms",
                prep_tx.signature,
                prep_tx.rpc_url,
                simulation_status_output_str, // Use the formatted string
                duration.as_millis()
            );

            SimulationAttempt {
                rpc_url: prep_tx.rpc_url,
                original_signature: prep_tx.signature,
                amount_lamports: prep_tx.amount_lamports,
                simulation_result: final_sim_result_for_struct, // Assign the processed result
                simulation_duration_ms: duration.as_millis(),
            }
        });
        simulation_tasks.push(task);
    }

    let mut simulation_attempts = Vec::new();
    for task in simulation_tasks {
        match task.await {
            Ok(attempt) => simulation_attempts.push(attempt),
            Err(e) => {
                eprintln!("Tokio task for simulation failed (JoinError): {}", e);
            }
        }
    }
    simulation_attempts
}
