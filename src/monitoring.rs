use crate::transactions::SendAttempt;
use solana_client::{
    client_error::{ClientError as SolanaClientError, Result as ClientResult},
    rpc_client::RpcClient,
    rpc_response::Response,
};
use solana_sdk::{commitment_config::CommitmentConfig, signature::Signature};
use solana_transaction_status::{TransactionConfirmationStatus, TransactionStatus};
use std::{
    collections::HashMap,
    error::Error,
    time::{Duration, Instant},
};

/// Holds information about the transaction that was confirmed first.
#[derive(Debug, Clone)]
pub struct WinningTransactionInfo {
    pub signature: Signature,
    pub rpc_url: String,
    pub amount_lamports: u64,
    pub time_to_confirm_ms: u128,
    pub slot: u64,
    pub confirmation_status_description: String,
}

/// Holds the final observed status of a transaction that did not win the race.
#[derive(Debug, Clone)]
pub struct NonWinningTransactionOutcome {
    pub original_signature: Signature,
    pub rpc_url: String,
    pub amount_lamports: u64,
    pub status_summary: String,
    pub last_known_slot: Option<u64>,
}

/// Errors that can occur while tracking a single transaction's confirmation status.
#[derive(Debug)]
#[allow(dead_code)]
enum TrackError {
    RpcError(SolanaClientError),
    TransactionFailedOnChain(solana_sdk::transaction::TransactionError),
}

/// Tracks a single transaction until it's confirmed or a permanent error occurs for this path.
async fn track_single_transaction(
    attempt_to_track: SendAttempt,
    poll_interval: Duration,
) -> Result<WinningTransactionInfo, TrackError> {
    println!(
        "Tracking Tx: {} on RPC: {}",
        attempt_to_track.original_signature, attempt_to_track.rpc_url
    );
    let client = RpcClient::new_with_commitment(
        attempt_to_track.rpc_url.clone(),
        CommitmentConfig::confirmed(),
    );

    loop {
        let result: ClientResult<Response<Vec<Option<TransactionStatus>>>> =
            client.get_signature_statuses(&[attempt_to_track.original_signature]);

        match result {
            Ok(statuses_response) => {
                if let Some(Some(status)) = statuses_response.value.get(0) {
                    if let Some(tx_error) = &status.err {
                        return Err(TrackError::TransactionFailedOnChain(tx_error.clone()));
                    }
                    if let Some(conf_status) = &status.confirmation_status {
                        match conf_status {
                            TransactionConfirmationStatus::Confirmed
                            | TransactionConfirmationStatus::Finalized => {
                                let confirmed_at = Instant::now();
                                let time_to_confirm = confirmed_at
                                    .saturating_duration_since(attempt_to_track.send_start_instant);
                                return Ok(WinningTransactionInfo {
                                    signature: attempt_to_track.original_signature,
                                    rpc_url: attempt_to_track.rpc_url.clone(),
                                    amount_lamports: attempt_to_track.amount_lamports,
                                    time_to_confirm_ms: time_to_confirm.as_millis(),
                                    slot: status.slot,
                                    confirmation_status_description: format!("{:?}", conf_status),
                                });
                            }
                            TransactionConfirmationStatus::Processed => { /* Still waiting */ }
                        }
                    }
                }
            }
            Err(e) => return Err(TrackError::RpcError(e)),
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// Monitors transactions and returns the first one confirmed, along with others' final statuses.
pub async fn monitor_for_first_confirmation(
    all_send_attempts: Vec<SendAttempt>,
    overall_timeout: Duration,
    poll_interval: Duration,
) -> Result<
    (
        Option<WinningTransactionInfo>,
        Vec<NonWinningTransactionOutcome>,
    ),
    Box<dyn Error + Send + Sync>,
> {
    if all_send_attempts.is_empty() {
        return Ok((None, Vec::new()));
    }

    let mut join_set = tokio::task::JoinSet::new();
    let mut successfully_sent_map = HashMap::<Signature, SendAttempt>::new();
    let mut initially_failed_outcomes = Vec::new();

    for attempt in all_send_attempts.iter() {
        if attempt.send_result.is_ok() {
            join_set.spawn(track_single_transaction(attempt.clone(), poll_interval));
            successfully_sent_map.insert(attempt.original_signature, attempt.clone());
        } else {
            initially_failed_outcomes.push(NonWinningTransactionOutcome {
                original_signature: attempt.original_signature,
                rpc_url: attempt.rpc_url.clone(),
                amount_lamports: attempt.amount_lamports,
                status_summary: format!(
                    "Initial send failed: {}",
                    attempt
                        .send_result
                        .as_ref()
                        .err()
                        .map_or("Unknown send error", |s| s.as_str())
                ),
                last_known_slot: None,
            });
        }
    }

    if join_set.is_empty() {
        println!("No transactions were successfully sent to monitor.");
        return Ok((None, initially_failed_outcomes));
    }

    println!(
        "Monitoring {} successfully sent transactions...",
        join_set.len()
    );
    let deadline = Instant::now() + overall_timeout;
    let mut winner: Option<WinningTransactionInfo> = None;
    let mut completed_tracking_results =
        HashMap::<Signature, Result<WinningTransactionInfo, TrackError>>::new();

    while !join_set.is_empty() && winner.is_none() && Instant::now() < deadline {
        tokio::select! {
            biased;
            join_result = join_set.join_next() => {
                if let Some(res) = join_result {
                    match res {
                        Ok(Ok(confirmed_info)) => {
                            if winner.is_none() || confirmed_info.time_to_confirm_ms < winner.as_ref().unwrap().time_to_confirm_ms {
                                winner = Some(confirmed_info.clone());
                            }
                            completed_tracking_results.insert(confirmed_info.signature, Ok(confirmed_info));
                        }
                        Ok(Err(_track_error)) => {}
                        Err(_join_err) => {}
                    }
                } else {
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => { }
        }
    }

    join_set.shutdown().await;
    if winner.is_none() && Instant::now() >= deadline {
        println!("Overall monitoring timeout reached.");
    }

    let mut final_outcomes = initially_failed_outcomes;

    for (sig, sent_attempt) in successfully_sent_map {
        if winner.as_ref().map_or(false, |w| w.signature == sig) {
            continue;
        }

        let final_status_summary: String;
        let final_slot: Option<u64>;

        if let Some(Ok(confirmed_later_info)) = completed_tracking_results.get(&sig) {
            final_status_summary = format!(
                "Confirmed (but not the overall winner at {}ms) - Status: {:?}",
                confirmed_later_info.time_to_confirm_ms,
                confirmed_later_info.confirmation_status_description
            );
            final_slot = Some(confirmed_later_info.slot);
        } else {
            let rpc_client = RpcClient::new_with_commitment(
                sent_attempt.rpc_url.clone(),
                CommitmentConfig::confirmed(),
            );
            match rpc_client.get_signature_statuses(&[sig]) {
                Ok(response) => {
                    if let Some(Some(status_detail)) = response.value.get(0) {
                        final_slot = Some(status_detail.slot);
                        if let Some(err) = &status_detail.err {
                            final_status_summary = format!("Failed on-chain: {:?}", err);
                        } else if let Some(cs) = &status_detail.confirmation_status {
                            final_status_summary =
                                format!("Not the winner. Final status: {:?}", cs);
                        } else {
                            final_status_summary =
                                "Not the winner. Status unclear in final check.".to_string();
                        }
                    } else {
                        final_slot = None;
                        final_status_summary =
                            "Not the winner. Not found in final check.".to_string();
                    }
                }
                Err(e) => {
                    final_slot = None;
                    final_status_summary =
                        format!("Not the winner. RPC error in final check: {}", e);
                }
            }
        }
        final_outcomes.push(NonWinningTransactionOutcome {
            original_signature: sig,
            rpc_url: sent_attempt.rpc_url.clone(),
            amount_lamports: sent_attempt.amount_lamports,
            status_summary: final_status_summary,
            last_known_slot: final_slot,
        });
    }
    Ok((winner, final_outcomes))
}
