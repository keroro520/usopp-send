use solana_client::rpc_client::RpcClient; // Added RpcClient import
use solana_sdk::{
    hash::Hash,
    // instruction::Instruction, // Keep if direct Instruction use is planned, else remove
    pubkey::Pubkey,
    signature::{Signature, Signer}, // Added Signature for return type
    signer::keypair::Keypair,
    system_instruction,
    transaction::Transaction,
};
use std::time::SystemTime;
use tokio::task::JoinHandle; // For tokio::spawn // For recording send time

// Placeholder for transaction sending logic

pub fn send_transactions() -> Result<(), String> {
    // Implement concurrent transaction sending logic here
    Err("Transaction sending not yet implemented".to_string())
}

// Renaming original placeholder to avoid conflict if it was meant for actual sending
pub fn send_actual_transactions_placeholder() -> Result<(), String> {
    Err("Transaction sending not yet implemented".to_string())
}

/// Constructs N conflicting transactions.
/// Each transaction attempts to send a large, slightly different portion of the sender's balance.
pub fn construct_conflicting_transactions(
    sender_keypair: &Keypair,
    sender_balance: u64,
    recipient_pubkey: &Pubkey,
    latest_blockhash: Hash,
    num_transactions: usize,
) -> Result<Vec<Transaction>, String> {
    let mut transactions = Vec::new();
    let sender_pubkey = sender_keypair.pubkey();

    if num_transactions == 0 {
        return Err("Number of transactions must be greater than 0".to_string());
    }

    // A typical transfer fee is 5000 lamports.
    const MIN_FEE_PER_TRANSACTION: u64 = 5000;
    if sender_balance < MIN_FEE_PER_TRANSACTION {
        return Err(format!(
            "Sender balance {} is less than minimum fee {} to construct any transaction",
            sender_balance, MIN_FEE_PER_TRANSACTION
        ));
    }

    for i in 0..num_transactions {
        // Calculate amount: (90 - i)% of (balance - fee_allowance_for_this_tx).
        let percentage_factor = (90.0 - i as f64) / 100.0;
        if percentage_factor <= 0.0 {
            // Stop if percentage becomes non-positive
            break;
        }

        // Max amount that can be transferred, leaving room for this one transaction's fee.
        let max_transfer_this_tx = sender_balance.saturating_sub(MIN_FEE_PER_TRANSACTION);

        if max_transfer_this_tx == 0 {
            // This specific iteration cannot proceed if balance can't even cover fee + 1 lamport.
            // If this happens for i=0, the initial balance check should have caught it.
            // For subsequent i, it means previous (larger percentage) tx would have been too big.
            println!(
                "Warning: For tx {}, sender balance {} not enough to cover fee {} and send >0 amount. Skipping.",
                i, sender_balance, MIN_FEE_PER_TRANSACTION
            );
            continue;
        }

        let mut amount = (max_transfer_this_tx as f64 * percentage_factor) as u64;

        // Ensure amount is at least 1 lamport if it was rounded down to 0 but percentage factor was positive
        if amount == 0 && percentage_factor > 0.0 {
            amount = 1;
        }

        // Final check: ensure amount is not more than what's possible to transfer (shouldn't happen with above logic but good safeguard)
        if amount > max_transfer_this_tx {
            amount = max_transfer_this_tx;
        }

        if amount == 0 {
            // If after all adjustments, amount is 0, skip this transaction.
            println!(
                "Warning: Calculated amount for transaction {} is 0. Skipping.",
                i
            );
            continue;
        }

        let instructions = vec![system_instruction::transfer(
            &sender_pubkey,
            recipient_pubkey,
            amount,
        )];

        let message = solana_sdk::message::Message::new_with_blockhash(
            &instructions,
            Some(&sender_pubkey),
            &latest_blockhash,
        );

        let mut tx = Transaction::new_unsigned(message);

        tx.sign(&[sender_keypair], latest_blockhash);
        transactions.push(tx);
    }

    if transactions.is_empty() && num_transactions > 0 {
        return Err("No transactions were constructed. This might be due to very low sender balance or an issue with the amount calculation logic.".to_string());
    }

    Ok(transactions)
}

#[derive(Debug)] // For easy printing of results
pub struct SendAttempt {
    pub rpc_url: String,
    pub signature: Option<Signature>,
    pub send_time: Option<SystemTime>,
    pub error: Option<String>,
    // We might add confirmation_time and latency here later
}

pub async fn send_transactions_concurrently(
    rpc_clients: Vec<RpcClient>,
    transactions: Vec<Transaction>,
) -> Vec<SendAttempt> {
    // Return a vector of SendAttempt results
    if rpc_clients.len() != transactions.len() {
        // This is a critical error, should not happen if logic is correct upstream.
        // Returning a single error in a Vec<SendAttempt> to indicate a systemic failure.
        return vec![SendAttempt {
            rpc_url: "N/A".to_string(),
            signature: None,
            send_time: None,
            error: Some("Mismatch between number of RPC clients and transactions".to_string()),
        }];
    }

    let mut send_handles: Vec<JoinHandle<SendAttempt>> = Vec::new();

    for (rpc_client, transaction) in rpc_clients.into_iter().zip(transactions.into_iter()) {
        let rpc_url_clone = rpc_client.url(); // Clone for use in the error case of the spawned task
        let handle = tokio::spawn(async move {
            println!(
                "Attempting to send transaction via RPC: {}",
                rpc_client.url()
            );
            let send_time = SystemTime::now();
            match rpc_client.send_transaction(&transaction) {
                // Use non-blocking send_transaction
                Ok(signature) => {
                    println!(
                        "Transaction {} submitted successfully via {} at {:?}",
                        signature,
                        rpc_client.url(),
                        send_time
                    );
                    SendAttempt {
                        rpc_url: rpc_client.url(),
                        signature: Some(signature),
                        send_time: Some(send_time),
                        error: None,
                    }
                }
                Err(e) => {
                    eprintln!("Failed to send transaction via {}: {}", rpc_url_clone, e);
                    SendAttempt {
                        rpc_url: rpc_url_clone, // Use cloned URL as rpc_client is moved
                        signature: None,
                        send_time: Some(send_time), // Record send attempt time even on failure
                        error: Some(format!("Send failed: {}", e.to_string())),
                    }
                }
            }
        });
        send_handles.push(handle);
    }

    let mut results = Vec::new();
    for handle in send_handles {
        match handle.await {
            Ok(send_attempt_result) => results.push(send_attempt_result),
            Err(join_err) => {
                eprintln!("Tokio join error during send: {}", join_err);
                // This indicates a panic in the spawned task or a tokio-related issue.
                // It's hard to associate with a specific RPC URL here without more context passing.
                results.push(SendAttempt {
                    rpc_url: "Unknown (task join error)".to_string(),
                    signature: None,
                    send_time: None, // No send attempt if task panicked before/during.
                    error: Some(format!("Tokio task failed: {}", join_err.to_string())),
                });
            }
        }
    }
    results
}
