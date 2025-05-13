mod accounts;
mod cli;
mod config;
mod monitoring;
mod transactions;

use accounts::determine_account_roles;
use cli::CliArgs;
use config::Config;
use monitoring::monitor_for_first_confirmation;
use solana_client::rpc_client::RpcClient;
use std::{process::ExitCode, time::Duration};
use transactions::{
    construct_conflicting_transactions, send_transactions_concurrently,
    simulate_transactions_concurrently,
};

const OVERALL_MONITORING_TIMEOUT_SECONDS: u64 = 30;
const POLLING_INTERVAL_MS: u64 = 1000;

#[tokio::main]
async fn main() -> ExitCode {
    let cli_args = CliArgs::parse_args();
    let config_path = &cli_args.config_path;

    println!("Usopp-Send Initializing...");
    if cli_args.dry_run {
        println!("*** DRY-RUN MODE ENABLED ***");
    }
    println!("Attempting to load configuration from: {}", config_path);

    let conf = match Config::load(config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "Error: Failed to load configuration from '{}'.",
                config_path
            );
            eprintln!("Details: {}", e);
            return ExitCode::FAILURE;
        }
    };
    println!("Configuration loaded successfully: {:#?}", conf);

    if conf.rpc_urls.is_empty() {
        eprintln!("Error: No RPC URLs provided in configuration.");
        return ExitCode::FAILURE;
    }

    println!("\nDetermining account roles...");
    let (sender_account, recipient_account) = match determine_account_roles(&conf).await {
        Ok(roles) => roles,
        Err(e) => {
            eprintln!("Error determining account roles: {}", e);
            return ExitCode::FAILURE;
        }
    };
    println!(
        "Sender: Pubkey {}, Balance: {} lamports",
        sender_account.pubkey, sender_account.balance
    );
    println!(
        "Recipient: Pubkey {}, Balance: {} lamports",
        recipient_account.pubkey, recipient_account.balance
    );

    println!("\nConstructing conflicting transactions...");
    let rpc_client_for_construction = RpcClient::new(conf.rpc_urls[0].clone());
    let prepared_txs = match construct_conflicting_transactions(
        &sender_account,
        &recipient_account,
        &conf.rpc_urls,
        &rpc_client_for_construction,
    ) {
        Ok(txs) => txs,
        Err(e) => {
            eprintln!("Error constructing transactions: {}", e);
            return ExitCode::FAILURE;
        }
    };

    if prepared_txs.is_empty() {
        println!("No transactions were constructed. Exiting.");
        return ExitCode::SUCCESS;
    }
    println!(
        "Successfully constructed {} conflicting transactions:",
        prepared_txs.len()
    );
    for (i, tx) in prepared_txs.iter().enumerate() {
        println!(
            "  Tx {}: Signature: {}, Amount: {} lamports, Target RPC: {}",
            i, tx.signature, tx.amount_lamports, tx.rpc_url
        );
    }

    if cli_args.dry_run {
        println!("\n--- DRY-RUN: Simulating Transactions ---");
        let simulation_attempts = simulate_transactions_concurrently(prepared_txs).await;
        println!("\nDry-run simulation attempts summary:");
        let mut successful_simulations = 0;
        for (i, attempt) in simulation_attempts.iter().enumerate() {
            print!(
                "  Sim {}: Tx (sig {}) for RPC {}. Amount: {} lamports. Duration: {}ms -> ",
                i,
                attempt.original_signature,
                attempt.rpc_url,
                attempt.amount_lamports,
                attempt.simulation_duration_ms
            );
            match &attempt.simulation_result {
                Ok(sim_res) => {
                    if let Some(err) = &sim_res.err {
                        println!("SIMULATION FAILED. Error: {:?}", err);
                    } else {
                        successful_simulations += 1;
                        println!("SIMULATION SUCCEEDED.");
                    }
                    if let Some(logs) = &sim_res.logs {
                        if !logs.is_empty() {
                            println!("    Logs:");
                            for log in logs {
                                println!("      {}", log);
                            }
                        }
                    }
                    if let Some(units) = sim_res.units_consumed {
                        println!("    Units Consumed: {}", units);
                    }
                }
                Err(e) => {
                    println!("RPC ERROR during simulation: {}", e);
                }
            }
        }
        println!(
            "Dry-run finished: {} successful simulations, {} failed or had RPC errors.",
            successful_simulations,
            simulation_attempts.len() - successful_simulations
        );
        println!("--- DRY-RUN COMPLETE ---");
    } else {
        println!("\n--- LIVE RUN: Sending Transactions ---");
        let send_attempts = send_transactions_concurrently(prepared_txs).await;
        println!("\nTransaction send attempts summary:");
        let mut successful_sends_count = 0;
        for (i, attempt) in send_attempts.iter().enumerate() {
            match &attempt.send_result {
                Ok(returned_sig) => {
                    successful_sends_count += 1;
                    println!(
                        "  Attempt {}: Tx (original sig: {}) to RPC {} -> SUCCESS. Returned sig: {}. Send duration: {}ms",
                        i, attempt.original_signature, attempt.rpc_url, returned_sig, attempt.send_duration_ms
                    );
                }
                Err(e) => {
                    println!(
                        "  Attempt {}: Tx (original sig: {}) to RPC {} -> FAILED. Error: {}. Send duration: {}ms",
                        i, attempt.original_signature, attempt.rpc_url, e, attempt.send_duration_ms
                    );
                }
            }
        }
        println!(
            "Finished sending: {} successful, {} failed/skipped.",
            successful_sends_count,
            send_attempts.len() - successful_sends_count
        );

        println!("\n--- LIVE RUN: Monitoring Confirmations ---");
        match monitor_for_first_confirmation(
            send_attempts,
            Duration::from_secs(OVERALL_MONITORING_TIMEOUT_SECONDS),
            Duration::from_millis(POLLING_INTERVAL_MS),
        )
        .await
        {
            Ok((Some(winner), non_winning_outcomes)) => {
                println!("\n--- Test Complete: Winner Found! ---");
                println!("Fastest Transaction Signature: {}", winner.signature);
                println!("Winning RPC URL: {}", winner.rpc_url);
                println!("Amount Sent: {} lamports", winner.amount_lamports);
                println!(
                    "Time from Send to {}: {} ms",
                    winner.confirmation_status_description, winner.time_to_confirm_ms
                );
                println!("Confirmed in Slot: {}", winner.slot);

                if !non_winning_outcomes.is_empty() {
                    println!("\nSummary of other transactions:");
                    for outcome in non_winning_outcomes {
                        println!(
                            "  - Sig: {}, RPC: {}, Amount: {} lamports, Status: {}",
                            outcome.original_signature,
                            outcome.rpc_url,
                            outcome.amount_lamports,
                            outcome.status_summary
                        );
                        if let Some(slot) = outcome.last_known_slot {
                            println!("    Last known slot: {}", slot);
                        }
                    }
                }
            }
            Ok((None, non_winning_outcomes)) => {
                println!("\n--- Test Complete: No Winner Found ---");
                println!(
                    "No transaction was confirmed within the timeout of {} seconds.",
                    OVERALL_MONITORING_TIMEOUT_SECONDS
                );
                if !non_winning_outcomes.is_empty() {
                    println!("\nSummary of transactions attempted:");
                    for outcome in non_winning_outcomes {
                        println!(
                            "  - Sig: {}, RPC: {}, Amount: {} lamports, Status: {}",
                            outcome.original_signature,
                            outcome.rpc_url,
                            outcome.amount_lamports,
                            outcome.status_summary
                        );
                        if let Some(slot) = outcome.last_known_slot {
                            println!("    Last known slot: {}", slot);
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("\n--- Test Error: Monitoring Failed ---");
                eprintln!("An error occurred during transaction monitoring: {}", e);
                return ExitCode::FAILURE;
            }
        }
        println!("--- LIVE RUN COMPLETE ---");
    }

    ExitCode::SUCCESS
}
