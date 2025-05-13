pub mod config;
pub mod solana_utils;
pub mod transaction_sender;

use clap::Parser;
use config::Config;
use log::{error, info, warn};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig, pubkey::Pubkey, signer::Signer, signer::keypair::Keypair,
};
use solana_utils::{
    MonitoredTxInfo, get_account_balance, get_latest_blockhash, load_keypair_from_file,
    monitor_transaction_statuses,
};
use transaction_sender::{
    SendAttempt, construct_conflicting_transactions, send_transactions_concurrently,
};

/// Usopp-Send: A Solana RPC transaction racing tool
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to the configuration file (e.g., config.json)
    #[arg(short, long, default_value = "config.json")]
    config_path: String,

    /// Run in dry-run mode (simulate transactions without sending)
    #[arg(long)]
    dry_run: bool,
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let cli = Cli::parse();

    info!("Starting Usopp-Send...");
    info!("Config path: {}", cli.config_path);
    info!("Dry-run mode: {}", cli.dry_run);

    let config = match Config::load(&cli.config_path) {
        Ok(conf) => conf,
        Err(e) => {
            error!("Error loading configuration: {}", e);
            std::process::exit(1);
        }
    };

    let setup_rpc_client = if !config.rpc_urls.is_empty() {
        info!("\nUsing RPC URL for initial setup: {}", &config.rpc_urls[0]);
        RpcClient::new_with_commitment(config.rpc_urls[0].clone(), CommitmentConfig::confirmed())
    } else {
        error!("No RPC URLs provided in configuration for setup. Exiting.");
        std::process::exit(1);
    };

    let rpc_clients_for_sending: Vec<RpcClient> = config
        .rpc_urls
        .iter()
        .map(|url| RpcClient::new_with_commitment(url.clone(), CommitmentConfig::confirmed()))
        .collect();

    if rpc_clients_for_sending.is_empty() {
        error!("No RPC clients created for sending. Check RPC URLs in config. Exiting.");
        std::process::exit(1);
    }
    info!(
        "Initialized {} RPC clients for sending transactions.",
        rpc_clients_for_sending.len()
    );

    let mut keypair1_data: Option<(Keypair, u64)> = None;
    let mut keypair2_data: Option<(Keypair, u64)> = None;

    if let Ok(kp) = load_keypair_from_file(&config.keypair_path_1) {
        match get_account_balance(&setup_rpc_client, &kp.pubkey()) {
            Ok(balance) => {
                info!("Keypair 1: {}, Balance: {} lamports", kp.pubkey(), balance);
                keypair1_data = Some((kp, balance));
            }
            Err(e) => error!(
                "Failed to get balance for keypair 1 ({}): {}",
                kp.pubkey(),
                e
            ),
        }
    } else {
        warn!(
            "Failed to load keypair 1 from path: {}. This may be expected if file doesn't exist.",
            &config.keypair_path_1
        );
    }

    if let Ok(kp) = load_keypair_from_file(&config.keypair_path_2) {
        match get_account_balance(&setup_rpc_client, &kp.pubkey()) {
            Ok(balance) => {
                info!("Keypair 2: {}, Balance: {} lamports", kp.pubkey(), balance);
                keypair2_data = Some((kp, balance));
            }
            Err(e) => error!(
                "Failed to get balance for keypair 2 ({}): {}",
                kp.pubkey(),
                e
            ),
        }
    } else {
        warn!(
            "Failed to load keypair 2 from path: {}. This may be expected if file doesn't exist.",
            &config.keypair_path_2
        );
    }

    let sender_keypair: Keypair;
    let sender_balance: u64;
    let recipient_pubkey: Pubkey;

    if let (Some((kp1, bal1)), Some((kp2, bal2))) = (keypair1_data, keypair2_data) {
        if bal1 >= bal2 {
            sender_keypair = kp1;
            sender_balance = bal1;
            recipient_pubkey = kp2.pubkey();
            info!(
                "\nSender: Keypair 1 ({}), Balance: {} lamports. Recipient: Keypair 2 ({})",
                sender_keypair.pubkey(),
                sender_balance,
                recipient_pubkey
            );
        } else {
            sender_keypair = kp2;
            sender_balance = bal2;
            recipient_pubkey = kp1.pubkey();
            info!(
                "\nSender: Keypair 2 ({}), Balance: {} lamports. Recipient: Keypair 1 ({})",
                sender_keypair.pubkey(),
                sender_balance,
                recipient_pubkey
            );
        }
    } else {
        error!("\nCould not determine roles. Keypair/balance info missing. Exiting.");
        std::process::exit(1);
    }

    let latest_blockhash = match get_latest_blockhash(&setup_rpc_client) {
        Ok(bh) => {
            info!("Latest blockhash: {}", bh);
            bh
        }
        Err(e) => {
            error!("Error fetching blockhash: {}", e);
            std::process::exit(1);
        }
    };

    let num_transactions_to_construct = rpc_clients_for_sending.len();
    info!("\nConstructing {} txs...", num_transactions_to_construct);

    let transactions_to_send = match construct_conflicting_transactions(
        &sender_keypair,
        sender_balance,
        &recipient_pubkey,
        latest_blockhash,
        num_transactions_to_construct,
    ) {
        Ok(transactions) => {
            info!(
                "Successfully constructed {} transactions.",
                transactions.len()
            );
            if !transactions.is_empty() {
                info!(
                    "First transaction signature (example): {}",
                    transactions[0].signatures[0]
                );
                for (i, tx) in transactions.iter().enumerate() {
                    if let Some(instruction) = tx.message.instructions.get(0) {
                        let program_id =
                            tx.message.program_id(instruction.program_id_index as usize);
                        if let Some(pid) = program_id {
                            if *pid == solana_sdk::system_program::id()
                                && instruction.data.len() >= 12
                            {
                                let mut lamport_bytes = [0u8; 8];
                                lamport_bytes.copy_from_slice(&instruction.data[4..12]);
                                let lamports = u64::from_le_bytes(lamport_bytes);
                                info!("Tx {}: amount {} lamports", i, lamports);
                            }
                        }
                    }
                }
            }
            transactions
        }
        Err(e) => {
            error!("Error constructing transactions: {}", e);
            std::process::exit(1);
        }
    };

    if transactions_to_send.len() != rpc_clients_for_sending.len() {
        error!(
            "Mismatch: {} txs, {} RPC clients. Exiting.",
            transactions_to_send.len(),
            rpc_clients_for_sending.len()
        );
        std::process::exit(1);
    }

    if !cli.dry_run {
        info!("\nSending transactions concurrently...");
        let send_attempts =
            send_transactions_concurrently(rpc_clients_for_sending, transactions_to_send).await;

        let mut txs_to_monitor: Vec<MonitoredTxInfo> = Vec::new();
        info!("\nTransaction send attempt results:");
        for (i, attempt) in send_attempts.iter().enumerate() {
            if let Some(signature) = attempt.signature {
                info!(
                    "  Tx Attempt {} ({}): Submitted. Signature: {}",
                    i, attempt.rpc_url, signature
                );
                if let Some(time) = attempt.send_time {
                    txs_to_monitor.push(MonitoredTxInfo {
                        signature,
                        send_time: time,
                        rpc_url_sent_via: attempt.rpc_url.clone(),
                    });
                }
            }
            if let Some(error) = &attempt.error {
                eprintln!("  Error: {}", error);
            }
        }

        if !txs_to_monitor.is_empty() {
            println!(
                "\n{} transactions submitted successfully. Starting monitoring...",
                txs_to_monitor.len()
            );
            if let Some(winner) =
                monitor_transaction_statuses(&setup_rpc_client, txs_to_monitor).await
            {
                println!("\n--- Race Winner ---");
                println!("Signature: {}", winner.signature);
                println!("Sent via RPC: {}", winner.rpc_url_sent_via);
                println!("Latency: {:?}", winner.latency);
                println!("Confirmed in Slot: {}", winner.slot);
            } else {
                println!("\nNo transaction confirmed within the timeout period.");
            }
        } else {
            println!("\nNo transactions were successfully submitted to monitor.");
        }
    } else {
        info!("\nDry-run mode: Simulating transactions...");
        info!(
            "Constructed {} transactions for dry-run simulation:",
            transactions_to_send.len()
        );
        for (i, tx_ref) in transactions_to_send.iter().enumerate() {
            info!(
                "Simulating Tx {} (Signature placeholder: {})...",
                i, tx_ref.signatures[0]
            );
            let tx_to_simulate = tx_ref.clone();
            match setup_rpc_client.simulate_transaction(&tx_to_simulate) {
                Ok(sim_response) => {
                    if sim_response.value.err.is_some() {
                        error!(
                            "  Tx {} simulation FAILED: {:?}",
                            i,
                            sim_response.value.err.unwrap()
                        );
                    } else {
                        info!("  Tx {} simulation SUCCEEDED.", i);
                        if let Some(units) = sim_response.value.units_consumed {
                            info!("    Compute units consumed: {}", units);
                        }
                    }
                    if let Some(logs) = sim_response.value.logs {
                        if !logs.is_empty() {
                            // info!("    Logs:"); // Can be verbose
                            // for log in logs { info!("      {}", log); }
                        }
                    }
                }
                Err(e) => {
                    error!("  Tx {} simulation RPC error: {}", i, e);
                }
            }
        }
    }

    println!("\n--- Usopp-Send finished ---");
}
