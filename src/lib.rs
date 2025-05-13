pub mod config;
pub mod solana_utils;
pub mod transaction_sender;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Solana client error: {0}")]
    SolanaClient(#[from] solana_client::client_error::ClientError),
    #[error("Solana SDK error: {0}")]
    SolanaSdk(#[from] solana_sdk::signer::SignerError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("No RPC URLs provided")]
    NoRpcUrls,
    #[error("Insufficient RPC URLs provided. Need at least 2.")]
    InsufficientRpcUrls,
    #[error("Keypair loading error: {0}")]
    KeypairLoad(String),
    #[error("Transaction sending error: {0}")]
    TransactionSend(String),
    #[error("Transaction confirmation error: {0}")]
    TransactionConfirm(String),
    #[error("Dry run simulation error: {0}")]
    DryRunSimulate(String),
    #[error("All transactions failed or timed out")]
    AllTransactionsFailed,
    #[error("An unexpected error occurred: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
