use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Deserialize, Debug, PartialEq)] // Added PartialEq for comparison in tests
pub struct Config {
    pub rpc_urls: Vec<String>,
    pub keypair_path_1: String,
    pub keypair_path_2: String,
}

impl Config {
    pub fn load(path_str: &str) -> Result<Self, String> {
        let path = Path::new(path_str);
        if !path.exists() {
            return Err(format!("Configuration file not found at: {}", path_str));
        }

        let contents = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read configuration file {}: {}", path_str, e))?;

        serde_json::from_str(&contents)
            .map_err(|e| format!("Failed to parse configuration file {}: {}", path_str, e))
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
        assert!(
            result
                .unwrap_err()
                .contains("Failed to parse configuration file")
        );
    }
}
