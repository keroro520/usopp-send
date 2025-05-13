use clap::Parser;

/// Usopp-Send: A tool to test Solana RPC node transaction propagation speed.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    /// Path to the configuration file.
    #[arg(short, long, default_value = "config.json")]
    pub config_path: String,

    /// Enable dry-run mode.
    /// In dry-run mode, transactions are constructed and simulated but not sent to the network.
    #[arg(long)]
    pub dry_run: bool,
}

impl CliArgs {
    pub fn parse_args() -> Self {
        CliArgs::parse()
    }
}
