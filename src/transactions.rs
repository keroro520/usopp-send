use crate::accounts::AccountInfo;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_response::RpcSimulateTransactionResult;
use solana_sdk::{
    message::Message, signature::Signature, system_instruction, transaction::Transaction,
};
use std::thread as std_thread;
use std::{error::Error, time::Instant};
use tokio::runtime::Builder as TokioRuntimeBuilder;
use tokio::sync::oneshot;

// Minimum balance to leave in sender's account after a transaction, in lamports.
const MIN_SENDER_RESERVE_LAMPORTS: u64 = 5_000; // Default rent-exempt minimum + a bit

/// Represents a signed transaction ready to be sent to a specific RPC node.
#[derive(Debug)]
pub struct PreparedTransaction {
    pub rpc_url: String,
    pub transaction: Transaction,
    pub signature: Signature,
    pub amount_lamports: u64,
}

/// Holds the result of a single transaction send attempt.
#[derive(Debug, Clone)]
pub struct SendAttempt {
    pub rpc_url: String,
    pub original_signature: Signature,
    pub amount_lamports: u64,
    pub send_result: Result<Signature, String>,
    pub send_start_instant: Instant,
    pub send_duration_ms: u128,
}

/// Holds the result of a single transaction simulation attempt.
#[derive(Debug)]
pub struct SimulationAttempt {
    pub rpc_url: String,
    pub original_signature: Signature,
    pub amount_lamports: u64,
    pub simulation_result: Result<RpcSimulateTransactionResult, String>,
    pub simulation_duration_ms: u128,
}

/// Constructs `n` conflicting transfer transactions.
/// `n` is determined by the number of `rpc_urls`.
/// Each transaction attempts to send a decreasing percentage of the sender's balance.
pub fn construct_conflicting_transactions(
    sender_account: &AccountInfo,
    recipient_account: &AccountInfo,
    rpc_urls: &[String],
    rpc_client: &RpcClient,
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
    prepared_transactions_input: Vec<PreparedTransaction>,
) -> Vec<SendAttempt> {
    if prepared_transactions_input.is_empty() {
        println!("No transactions to send.");
        return Vec::new();
    }

    let num_transactions = prepared_transactions_input.len();
    println!(
        "Phase 1: Setting up {} system threads for transaction sending...",
        num_transactions
    );

    let mut thread_setups = Vec::with_capacity(num_transactions);

    for i in 0..num_transactions {
        let rpc_url_for_thread_logging = prepared_transactions_input[i].rpc_url.clone();

        let (tx_to_thread, rx_from_main_for_tx) = oneshot::channel::<PreparedTransaction>();
        let (tx_from_thread_for_result, rx_for_main_for_result) = oneshot::channel::<SendAttempt>();

        let rpc_url_for_closure = rpc_url_for_thread_logging.clone();

        let handle = std_thread::spawn(move || {
            let runtime_result = TokioRuntimeBuilder::new_multi_thread().enable_all().build();

            let runtime = match runtime_result {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!(
                        "Thread for future RPC {}: Failed to create Tokio runtime: {}. Thread will exit.",
                        rpc_url_for_closure, e
                    );
                    return;
                }
            };

            runtime.block_on(async {
                println!(
                    "Thread for future RPC {}: Started, waiting for transaction...",
                    rpc_url_for_closure
                );
                match rx_from_main_for_tx.await {
                    Ok(prep_tx) => {
                        println!(
                            "Thread for RPC {}: Received Tx (sig: {}), commencing send.",
                            prep_tx.rpc_url, prep_tx.signature
                        );

                        let rpc_client = RpcClient::new(prep_tx.rpc_url.clone());
                        let start_time = Instant::now();
                        let send_tx_result = rpc_client.send_transaction(&prep_tx.transaction);
                        let duration = start_time.elapsed();

                        let send_result_outcome = match send_tx_result {
                            Ok(returned_signature) => {
                                println!(
                                    "Thread for RPC {}: Successfully sent Tx (original sig: {}). Returned sig: {}. Time: {}ms",
                                    prep_tx.rpc_url,
                                    prep_tx.signature,
                                    returned_signature,
                                    duration.as_millis()
                                );
                                Ok(returned_signature)
                            }
                            Err(e) => {
                                eprintln!(
                                    "Thread for RPC {}: Error sending Tx (original sig: {}). Error: {}. Time: {}ms",
                                    prep_tx.rpc_url,
                                    prep_tx.signature,
                                    e,
                                    duration.as_millis()
                                );
                                Err(e.to_string())
                            }
                        };

                        let attempt = SendAttempt {
                            rpc_url: prep_tx.rpc_url.clone(),
                            original_signature: prep_tx.signature,
                            amount_lamports: prep_tx.amount_lamports,
                            send_result: send_result_outcome,
                            send_start_instant: start_time,
                            send_duration_ms: duration.as_millis(),
                        };

                        if tx_from_thread_for_result.send(attempt).is_err() {
                            eprintln!(
                                "Thread for RPC {}: Failed to send result back to main. Original sig: {}.",
                                prep_tx.rpc_url,
                                prep_tx.signature
                            );
                        }
                    }
                    Err(_) => {
                        eprintln!(
                            "Thread for future RPC {}: Failed to receive transaction from main. Channel closed. Thread will exit.",
                            rpc_url_for_closure
                        );
                    }
                }
            });
        });
        thread_setups.push((
            handle,
            tx_to_thread,
            rx_for_main_for_result,
            rpc_url_for_thread_logging,
        ));
    }

    println!(
        "Phase 1 complete. All {} threads created and waiting.",
        thread_setups.len()
    );
    println!("Phase 2: Wait 2 seconds and then dispatching transactions to respective threads...");
    std_thread::sleep(std::time::Duration::from_secs(2));

    let mut result_collectors = Vec::with_capacity(num_transactions);
    let mut handles_to_join = Vec::with_capacity(num_transactions);

    for (prep_tx, (handle, sender_to_thread, result_receiver, _thread_rpc_url_for_log)) in
        prepared_transactions_input
            .into_iter()
            .zip(thread_setups.into_iter())
    {
        let log_sig_on_dispatch_fail = prep_tx.signature;
        let log_rpc_on_dispatch_fail = prep_tx.rpc_url.clone();

        if sender_to_thread.send(prep_tx).is_err() {
            eprintln!(
                "Main: Failed to dispatch Tx (sig: {}) to thread for RPC {}. The thread will likely error out.",
                log_sig_on_dispatch_fail, log_rpc_on_dispatch_fail
            );
        } else {
            println!(
                "Main: Dispatched Tx (sig: {}) to thread for RPC {}.",
                log_sig_on_dispatch_fail, log_rpc_on_dispatch_fail
            );
        }
        handles_to_join.push(handle);
        result_collectors.push((
            result_receiver,
            log_sig_on_dispatch_fail,
            log_rpc_on_dispatch_fail,
        ));
    }

    println!("Phase 2 complete. All transactions dispatched.",);
    println!(
        "Phase 3: Collecting results from {} threads...",
        result_collectors.len()
    );

    let mut send_attempts = Vec::with_capacity(result_collectors.len());

    for (receiver, original_sig_for_error, rpc_url_for_error) in result_collectors {
        match receiver.await {
            Ok(attempt) => {
                send_attempts.push(attempt);
            }
            Err(_) => {
                eprintln!(
                    "Main: Failed to receive result from thread for Tx (original sig: {}, RPC: {}). Channel closed. Thread may have failed/panicked.",
                    original_sig_for_error, rpc_url_for_error
                );
            }
        }
    }
    println!(
        "Phase 3 complete. All results collected (or failures noted). {} attempts recorded.",
        send_attempts.len()
    );
    println!(
        "Phase 4: Joining {} system threads...",
        handles_to_join.len()
    );

    for (i, handle) in handles_to_join.into_iter().enumerate() {
        if let Err(e) = handle.join() {
            eprintln!(
                "Main: System thread {} (associated with an earlier logged Tx) panicked: {:?}",
                i, e
            );
        }
    }
    println!("Phase 4 complete. All threads joined.");

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
                simulation_status_output_str,
                duration.as_millis()
            );

            SimulationAttempt {
                rpc_url: prep_tx.rpc_url,
                original_signature: prep_tx.signature,
                amount_lamports: prep_tx.amount_lamports,
                simulation_result: final_sim_result_for_struct,
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
