use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    hash::Hash,
    pubkey::Pubkey,
    signature::{Signature, Signer},
    signer::keypair::Keypair,
    system_instruction,
    transaction::Transaction,
};
use std::time::SystemTime;
use tokio::task::JoinHandle;

pub fn send_transactions() -> Result<(), String> {
    Err("Transaction sending not yet implemented".to_string())
}

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

    const MIN_FEE_PER_TRANSACTION: u64 = 5000;
    if sender_balance < MIN_FEE_PER_TRANSACTION {
        return Err(format!(
            "Sender balance {} is less than minimum fee {} to construct any transaction",
            sender_balance, MIN_FEE_PER_TRANSACTION
        ));
    }

    for i in 0..num_transactions {
        let percentage_factor = (90.0 - i as f64) / 100.0;
        if percentage_factor <= 0.0 {
            break;
        }

        let max_transfer_this_tx = sender_balance.saturating_sub(MIN_FEE_PER_TRANSACTION);

        if max_transfer_this_tx == 0 {
            println!(
                "Warning: For tx {}, sender balance {} not enough to cover fee {} and send >0 amount. Skipping.",
                i, sender_balance, MIN_FEE_PER_TRANSACTION
            );
            continue;
        }

        let mut amount = (max_transfer_this_tx as f64 * percentage_factor) as u64;

        if amount == 0 && percentage_factor > 0.0 {
            amount = 1;
        }

        if amount > max_transfer_this_tx {
            amount = max_transfer_this_tx;
        }

        if amount == 0 {
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

#[derive(Debug)]
pub struct SendAttempt {
    pub rpc_url: String,
    pub signature: Option<Signature>,
    pub send_time: Option<SystemTime>,
    pub error: Option<String>,
}

pub async fn send_transactions_concurrently(
    rpc_clients: Vec<RpcClient>,
    transactions: Vec<Transaction>,
) -> Vec<SendAttempt> {
    if rpc_clients.len() != transactions.len() {
        return vec![SendAttempt {
            rpc_url: "N/A".to_string(),
            signature: None,
            send_time: None,
            error: Some("Mismatch between number of RPC clients and transactions".to_string()),
        }];
    }

    let mut send_handles: Vec<JoinHandle<SendAttempt>> = Vec::new();

    for (rpc_client, transaction) in rpc_clients.into_iter().zip(transactions.into_iter()) {
        let rpc_url_clone = rpc_client.url();
        let handle = tokio::spawn(async move {
            println!(
                "Attempting to send transaction via RPC: {}",
                rpc_client.url()
            );
            let send_time = SystemTime::now();
            match rpc_client.send_transaction(&transaction) {
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
                        rpc_url: rpc_url_clone,
                        signature: None,
                        send_time: Some(send_time),
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
                results.push(SendAttempt {
                    rpc_url: "Unknown (task join error)".to_string(),
                    signature: None,
                    send_time: None,
                    error: Some(format!("Tokio task failed: {}", join_err.to_string())),
                });
            }
        }
    }
    results
}
