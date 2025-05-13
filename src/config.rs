use serde::Deserialize;
use std::{error::Error, fs::File, io::BufReader, path::PathBuf};

/// Represents the application configuration loaded from `config.json`.
#[derive(Deserialize, Debug)]
pub struct Config {
    pub rpc_urls: Vec<String>,
    pub keypair_path_1: String,
    pub keypair_path_2: String,
}

impl Config {
    /// Loads configuration from the specified file path.
    ///
    /// The path is expected to point to a JSON file structured according
    /// to the `Config` definition.
    pub fn load(path: &str) -> Result<Self, Box<dyn Error>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let config: Config = serde_json::from_reader(reader)?;
        Ok(config)
    }

    /// Expands a path string, resolving tilde (~) to the user's home directory.
    fn expand_path(path_str: &str) -> Result<PathBuf, String> {
        let expanded_path_cow = shellexpand::tilde(path_str);
        Ok(PathBuf::from(expanded_path_cow.as_ref()))
    }

    /// Returns the expanded `PathBuf` for `keypair_path_1`.
    ///
    /// Handles tilde expansion (e.g., `~/path/to/key.json`).
    pub fn keypair_path_1_expanded(&self) -> Result<PathBuf, String> {
        Self::expand_path(&self.keypair_path_1)
    }

    /// Returns the expanded `PathBuf` for `keypair_path_2`.
    ///
    /// Handles tilde expansion.
    pub fn keypair_path_2_expanded(&self) -> Result<PathBuf, String> {
        Self::expand_path(&self.keypair_path_2)
    }
}

#[cfg(test)]
mod tests {
    use super::*; // To import Config and its load method
    use std::io::Write; // For creating a temporary file
    use tempfile::NamedTempFile; // For creating a temporary file

    #[test]
    fn test_config_load_success() {
        let mut tmp_file = NamedTempFile::new().unwrap();
        let config_content = r#"
        {
            "rpc_urls": ["http://localhost:8899"],
            "keypair_path_1": "/tmp/kp1.json",
            "keypair_path_2": "/tmp/kp2.json"
        }
        "#;
        write!(tmp_file, "{}", config_content).unwrap();

        let loaded_config = Config::load(tmp_file.path().to_str().unwrap()).unwrap();

        assert_eq!(
            loaded_config.rpc_urls,
            vec!["http://localhost:8899".to_string()]
        );
        assert_eq!(loaded_config.keypair_path_1, "/tmp/kp1.json");
        assert_eq!(loaded_config.keypair_path_2, "/tmp/kp2.json");
    }

    #[test]
    fn test_config_load_file_not_found() {
        let result = Config::load("non_existent_config.json");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Configuration file not found"));
    }

    #[test]
    fn test_config_load_parse_error() {
        let mut tmp_file = NamedTempFile::new().unwrap();
        write!(tmp_file, "invalid json content").unwrap();
        let result = Config::load(tmp_file.path().to_str().unwrap());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Failed to parse configuration file"));
    }
}
