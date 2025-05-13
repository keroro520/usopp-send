use crate::config::Config;
use bs58;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signer},
};
use std::{error::Error, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountRole {
    Sender,
    Recipient,
}

#[derive(Debug)]
pub struct AccountInfo {
    pub keypair: Keypair,
    pub pubkey: Pubkey,
    pub balance: u64,
    pub role: Option<AccountRole>,
}

impl AccountInfo {
    fn new_from_path(keypair_path: &Path) -> Result<Self, Box<dyn Error>> {
        let keypair = read_keypair_file(keypair_path).map_err(|e| {
            format!(
                "Failed to read keypair file '{}': {}",
                keypair_path.display(),
                e
            )
        })?;
        let pubkey = keypair.pubkey();
        Ok(AccountInfo {
            keypair,
            pubkey,
            balance: 0,
            role: None,
        })
    }

    fn set_balance_and_role(&mut self, balance: u64, role: AccountRole) {
        self.balance = balance;
        self.role = Some(role);
    }
}

pub async fn determine_account_roles(
    config: &Config,
) -> Result<(AccountInfo, AccountInfo), Box<dyn Error>> {
    if config.rpc_urls.is_empty() {
        return Err("No RPC URLs provided in configuration.".into());
    }
    let rpc_url = &config.rpc_urls[0];
    println!("Using RPC URL for balance check: {}", rpc_url);
    let rpc_client = RpcClient::new(rpc_url.to_string());

    let keypair_path_1_expanded = config.keypair_path_1_expanded()?;
    let mut account1 = AccountInfo::new_from_path(&keypair_path_1_expanded)?;
    println!(
        "Loaded account 1 from '{}' with pubkey: {}",
        keypair_path_1_expanded.display(),
        bs58::encode(account1.pubkey.to_bytes()).into_string()
    );

    let keypair_path_2_expanded = config.keypair_path_2_expanded()?;
    let mut account2 = AccountInfo::new_from_path(&keypair_path_2_expanded)?;
    println!(
        "Loaded account 2 from '{}' with pubkey: {}",
        keypair_path_2_expanded.display(),
        bs58::encode(account2.pubkey.to_bytes()).into_string()
    );

    println!("Fetching balance for account 1 ({})...", account1.pubkey);
    let balance1 = rpc_client.get_balance(&account1.pubkey)?;
    println!("Balance for account 1: {} lamports", balance1);

    println!("Fetching balance for account 2 ({})...", account2.pubkey);
    let balance2 = rpc_client.get_balance(&account2.pubkey)?;
    println!("Balance for account 2: {} lamports", balance2);

    let (sender_account, recipient_account) = if balance1 >= balance2 {
        println!(
            "Account 1 (pubkey: {}) has {} lamports (>= Account 2: {} lamports). Assigning as Sender.",
            account1.pubkey, balance1, balance2
        );
        account1.set_balance_and_role(balance1, AccountRole::Sender);
        account2.set_balance_and_role(balance2, AccountRole::Recipient);
        (account1, account2)
    } else {
        println!(
            "Account 2 (pubkey: {}) has {} lamports (> Account 1: {} lamports). Assigning as Sender.",
            account2.pubkey, balance2, balance1
        );
        account2.set_balance_and_role(balance2, AccountRole::Sender);
        account1.set_balance_and_role(balance1, AccountRole::Recipient);
        (account2, account1)
    };

    println!(
        "Sender: Pubkey {}, Balance: {} lamports",
        bs58::encode(sender_account.pubkey.to_bytes()).into_string(),
        sender_account.balance
    );
    println!(
        "Recipient: Pubkey {}, Balance: {} lamports",
        bs58::encode(recipient_account.pubkey.to_bytes()).into_string(),
        recipient_account.balance
    );

    Ok((sender_account, recipient_account))
}
