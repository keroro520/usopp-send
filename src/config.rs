use crate::Result;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub rpc_urls: Vec<String>,
    pub keypair_path_1: String,
    pub keypair_path_2: String,
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let path_expanded = shellexpand::tilde(path).into_owned();
        let config_path = PathBuf::from(path_expanded);
        if !config_path.exists() {
            return Err(crate::Error::Config(format!(
                "Config file not found: {}",
                config_path.display()
            )));
        }

        let config_str = fs::read_to_string(&config_path)?;
        let config: Config = serde_json::from_str(&config_str)?;

        if config.rpc_urls.is_empty() {
            return Err(crate::Error::NoRpcUrls);
        }

        if config.rpc_urls.len() < 2 {
            return Err(crate::Error::InsufficientRpcUrls);
        }

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_config_success() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let content = r#"
        {
            "rpc_urls": [
                "http://localhost:8899",
                "https://api.mainnet-beta.solana.com"
            ],
            "keypair_path_1": "~/.config/solana/id1.json",
            "keypair_path_2": "~/.config/solana/id2.json"
        }
        "#;
        writeln!(temp_file, "{}", content).unwrap();

        let config = Config::load(temp_file.path().to_str().unwrap()).unwrap();

        assert_eq!(config.rpc_urls.len(), 2);
        assert_eq!(config.rpc_urls[0], "http://localhost:8899");
        assert_eq!(config.keypair_path_1, "~/.config/solana/id1.json");
    }

    #[test]
    fn test_load_config_file_not_found() {
        let result = Config::load("non_existent_config.json");
        assert!(matches!(result, Err(crate::Error::Config(_))));
    }

    #[test]
    fn test_load_config_invalid_json() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "invalid json content").unwrap();
        let result = Config::load(temp_file.path().to_str().unwrap());
        assert!(matches!(result, Err(crate::Error::Json(_))));
    }

    #[test]
    fn test_load_config_no_rpc_urls() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let content = r#"
        {
            "rpc_urls": [],
            "keypair_path_1": "~/.config/solana/id1.json",
            "keypair_path_2": "~/.config/solana/id2.json"
        }
        "#;
        writeln!(temp_file, "{}", content).unwrap();
        let result = Config::load(temp_file.path().to_str().unwrap());
        assert!(matches!(result, Err(crate::Error::NoRpcUrls)));
    }

    #[test]
    fn test_load_config_insufficient_rpc_urls() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let content = r#"
        {
            "rpc_urls": ["http://localhost:8899"],
            "keypair_path_1": "~/.config/solana/id1.json",
            "keypair_path_2": "~/.config/solana/id2.json"
        }
        "#;
        writeln!(temp_file, "{}", content).unwrap();
        let result = Config::load(temp_file.path().to_str().unwrap());
        assert!(matches!(result, Err(crate::Error::InsufficientRpcUrls)));
    }
}
