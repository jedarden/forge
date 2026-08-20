//! Server configuration loading from YAML files.
//!
//! This module provides support for loading FORGE server configuration
//! from YAML files, typically stored in `~/.forge/server.yaml`.
//!
//! Configuration can be overridden by CLI arguments, which take precedence.

use crate::websocket::{ServerConfig, TlsConfig};
use crate::ServerError;
use serde::Deserialize;
use std::path::Path;

/// YAML configuration file structure for FORGE server.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServerYamlConfig {
    /// Server configuration section
    pub server: Option<ServerSection>,
}

/// Server configuration section.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ServerSection {
    /// Bind address (default: 127.0.0.1)
    pub bind_address: Option<String>,

    /// Server port (default: 8080)
    pub port: Option<u16>,

    /// TLS configuration
    pub tls: Option<TlsSection>,
}

impl Default for ServerSection {
    fn default() -> Self {
        Self {
            bind_address: None,
            port: None,
            tls: None,
        }
    }
}

/// TLS configuration section.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TlsSection {
    /// Enable TLS/WSS
    pub enabled: Option<bool>,

    /// Path to TLS certificate file (PEM format)
    pub cert_path: Option<String>,

    /// Path to TLS private key file (PEM format)
    pub key_path: Option<String>,

    /// Verify TLS certificates (default: true)
    pub verify: Option<bool>,

    /// Minimum TLS version (default: "TLSv1.2")
    pub min_version: Option<String>,
}

impl Default for ServerYamlConfig {
    fn default() -> Self {
        Self {
            server: None,
        }
    }
}

/// Load server configuration from a YAML file.
///
/// # Arguments
///
/// * `config_path` - Path to the YAML configuration file
///
/// # Returns
///
/// Ok(ServerYamlConfig) if loading succeeds
/// Err(ServerError) if loading fails
pub fn load_server_yaml_config(config_path: &Path) -> Result<ServerYamlConfig, ServerError> {
    use std::fs::File;
    use std::io::Read;

    let mut file = File::open(config_path).map_err(|e| {
        ServerError::ConfigLoadError(format!(
            "Failed to open config file '{}': {}",
            config_path.display(),
            e
        ))
    })?;

    let mut contents = String::new();
    file.read_to_string(&mut contents).map_err(|e| {
        ServerError::ConfigLoadError(format!(
            "Failed to read config file '{}': {}",
            config_path.display(),
            e
        ))
    })?;

    serde_yaml::from_str(&contents).map_err(|e| {
        ServerError::ConfigParseError(format!(
            "Failed to parse config file '{}': {}",
            config_path.display(),
            e
        ))
    })
}

/// Convert YAML configuration to ServerConfig.
///
/// This function converts the loaded YAML configuration into a ServerConfig
/// that can be used by the FORGE server.
///
/// # Arguments
///
/// * `yaml_config` - The loaded YAML configuration
///
/// # Returns
///
/// ServerConfig for use by the server
impl From<ServerYamlConfig> for ServerConfig {
    fn from(yaml_config: ServerYamlConfig) -> Self {
        let server = yaml_config.server.unwrap_or_default();

        // Extract TLS configuration if present
        let tls_config = server.tls.and_then(|tls_section| {
            // Only create TlsConfig if both cert and key are provided
            if let (Some(cert_path), Some(key_path)) = (tls_section.cert_path, tls_section.key_path) {
                Some(TlsConfig {
                    cert_path,
                    key_path,
                    verify: tls_section.verify.unwrap_or(true),
                    min_version: tls_section.min_version.unwrap_or_else(|| "TLSv1.2".to_string()),
                })
            } else {
                None
            }
        });

        ServerConfig {
            bind_address: server.bind_address.unwrap_or_else(|| "127.0.0.1".to_string()),
            port: server.port.unwrap_or(8080),
            tls: tls_config,
        }
    }
}

/// Merge YAML configuration with CLI argument overrides.
///
/// CLI arguments take precedence over YAML configuration.
///
/// # Arguments
///
/// * `yaml_config` - Optional configuration from YAML file
/// * `bind_address` - CLI override for bind address
/// * `port` - CLI override for port
/// * `tls_enabled` - CLI override for TLS enabled
/// * `tls_cert_path` - CLI override for TLS certificate path
/// * `tls_key_path` - CLI override for TLS key path
/// * `tls_verify` - CLI override for TLS verification
/// * `tls_min_version` - CLI override for minimum TLS version
///
/// # Returns
///
/// Merged ServerConfig with CLI overrides applied
pub fn merge_config_with_cli_overrides(
    yaml_config: Option<ServerYamlConfig>,
    bind_address: Option<String>,
    port: Option<u16>,
    tls_enabled: Option<bool>,
    tls_cert_path: Option<String>,
    tls_key_path: Option<String>,
    tls_verify: Option<bool>,
    tls_min_version: Option<String>,
) -> ServerConfig {
    // Start with YAML config or defaults
    let mut server_config: ServerConfig = yaml_config.map(Into::into).unwrap_or_default();

    // Apply CLI overrides
    if let Some(bind) = bind_address {
        server_config.bind_address = bind;
    }

    if let Some(p) = port {
        server_config.port = p;
    }

    // Handle TLS overrides
    let tls = server_config.tls.as_ref();

    // If TLS is explicitly disabled via CLI, remove TLS config
    if tls_enabled == Some(false) {
        server_config.tls = None;
    } else if tls_enabled == Some(true) {
        // TLS is explicitly enabled via CLI
        // Both cert and key must be provided
        if let (Some(cert), Some(key)) = (tls_cert_path, tls_key_path) {
            server_config.tls = Some(TlsConfig {
                cert_path: cert,
                key_path: key,
                verify: tls_verify.unwrap_or(tls.map(|t| t.verify).unwrap_or(true)),
                min_version: tls_min_version.unwrap_or_else(|| tls.as_ref().map(|t| t.min_version.clone()).unwrap_or_else(|| "TLSv1.2".to_string())),
            });
        } else {
            // If TLS is enabled but cert/key not provided, keep YAML config or error
            if tls.is_none() {
                // No YAML TLS config and no CLI cert/key provided
                // This is an error condition that will be caught later
                server_config.tls = None;
            }
        }
    } else if tls_enabled.is_none() {
        // No TLS CLI flag, check if cert/key are provided as overrides
        if let (Some(cert), Some(key)) = (tls_cert_path, tls_key_path) {
            // Cert and key provided via CLI, enable TLS
            server_config.tls = Some(TlsConfig {
                cert_path: cert,
                key_path: key,
                verify: tls_verify.unwrap_or(tls.map(|t| t.verify).unwrap_or(true)),
                min_version: tls_min_version.unwrap_or_else(|| tls.as_ref().map(|t| t.min_version.clone()).unwrap_or_else(|| "TLSv1.2".to_string())),
            });
        }
    }

    server_config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_server_yaml_config() {
        let config = ServerYamlConfig::default();
        assert!(config.server.is_none());
    }

    #[test]
    fn test_yaml_config_to_server_config() {
        let yaml = ServerYamlConfig {
            server: Some(ServerSection {
                bind_address: Some("0.0.0.0".to_string()),
                port: Some(9000),
                tls: None,
            }),
        };

        let server_config: ServerConfig = yaml.into();
        assert_eq!(server_config.bind_address, "0.0.0.0");
        assert_eq!(server_config.port, 9000);
        assert!(server_config.tls.is_none());
    }

    #[test]
    fn test_merge_config_with_cli_overrides() {
        let yaml = ServerYamlConfig {
            server: Some(ServerSection {
                bind_address: Some("127.0.0.1".to_string()),
                port: Some(8080),
                tls: None,
            }),
        };

        let merged = merge_config_with_cli_overrides(
            Some(yaml),
            Some("0.0.0.0".to_string()),
            Some(9000),
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(merged.bind_address, "0.0.0.0");
        assert_eq!(merged.port, 9000);
    }

    #[test]
    fn test_tls_config_from_yaml() {
        let yaml = ServerYamlConfig {
            server: Some(ServerSection {
                bind_address: None,
                port: None,
                tls: Some(TlsSection {
                    enabled: Some(true),
                    cert_path: Some("/path/to/cert.pem".to_string()),
                    key_path: Some("/path/to/key.pem".to_string()),
                    verify: Some(false),
                    min_version: Some("TLSv1.3".to_string()),
                }),
            }),
        };

        let server_config: ServerConfig = yaml.into();
        assert!(server_config.tls.is_some());

        let tls = server_config.tls.unwrap();
        assert_eq!(tls.cert_path, "/path/to/cert.pem");
        assert_eq!(tls.key_path, "/path/to/key.pem");
        assert_eq!(tls.verify, false);
        assert_eq!(tls.min_version, "TLSv1.3");
    }
}
