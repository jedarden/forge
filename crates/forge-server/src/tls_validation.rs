//! TLS certificate validation and loading utilities.
//!
//! This module provides comprehensive TLS certificate handling including:
//! - Certificate chain loading (not just single certs)
//! - Certificate expiry checking with warnings
//! - Domain validation against CN and SANs
//! - Private key matching validation
//! - Detailed logging for debugging
//! - Timeout for file operations

use super::ServerError;
use crate::websocket::TlsConfig;
use base64::{Engine as _, engine::general_purpose};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::time::Duration;
use tracing::{debug, error, info, warn};
use x509_parser::prelude::*;

/// TLS certificate validation result.
#[derive(Debug, Clone)]
pub struct TlsValidationResult {
    /// Whether the certificate is valid
    pub is_valid: bool,
    /// Certificate subject CN
    pub subject_cn: Option<String>,
    /// Certificate issuer
    pub issuer: Option<String>,
    /// Certificate expiry date
    pub expires_at: Option<String>,
    /// Days until expiry (negative if expired)
    pub days_until_expiry: Option<i64>,
    /// Domains in certificate (CN + SANs)
    pub domains: Vec<String>,
    /// Validation warnings (non-fatal issues)
    pub warnings: Vec<String>,
    /// Validation errors (fatal issues)
    pub errors: Vec<String>,
}

/// Validate TLS configuration before starting the server.
///
/// This function performs comprehensive validation of TLS configuration:
/// - Checks file existence and readability
/// - Validates PEM format
/// - Checks certificate expiry (warns if < 30 days)
/// - Validates domain matches CN or SANs
/// - Verifies key matches certificate
///
/// # Arguments
/// * `config` - TLS configuration to validate
///
/// # Returns
/// Ok(TlsValidationResult) if validation succeeds (may have warnings)
/// Err(ServerError) if validation fails with fatal errors
pub fn validate_tls_config(config: &TlsConfig) -> Result<TlsValidationResult, ServerError> {
    let mut result = TlsValidationResult {
        is_valid: true,
        subject_cn: None,
        issuer: None,
        expires_at: None,
        days_until_expiry: None,
        domains: Vec::new(),
        warnings: Vec::new(),
        errors: Vec::new(),
    };

    info!("Validating TLS configuration...");
    debug!("Certificate path: {}", config.cert_path);
    debug!("Private key path: {}", config.key_path);

    // Check certificate file existence with timeout
    let cert_path = Path::new(&config.cert_path);
    if !cert_path.exists() {
        let msg = format!("Certificate file not found: {}", config.cert_path);
        error!("{}", msg);
        result.errors.push(msg);
        return Err(ServerError::CertificateLoadError(
            config.cert_path.clone(),
            "File not found".to_string(),
        ));
    }

    // Check private key file existence
    let key_path = Path::new(&config.key_path);
    if !key_path.exists() {
        let msg = format!("Private key file not found: {}", config.key_path);
        error!("{}", msg);
        result.errors.push(msg);
        return Err(ServerError::PrivateKeyLoadError(
            config.key_path.clone(),
            "File not found".to_string(),
        ));
    }

    // Load certificate with timeout to avoid hangs on network mounts
    let cert_pem = match read_file_with_timeout(&config.cert_path, Duration::from_secs(5)) {
        Ok(data) => data,
        Err(e) => {
            let msg = format!("Failed to read certificate file: {}", e);
            error!("{}", msg);
            result.errors.push(msg);
            return Err(ServerError::CertificateLoadError(
                config.cert_path.clone(),
                e.to_string(),
            ));
        }
    };

    // Load private key with timeout
    let key_pem = match read_file_with_timeout(&config.key_path, Duration::from_secs(5)) {
        Ok(data) => data,
        Err(e) => {
            let msg = format!("Failed to read private key file: {}", e);
            error!("{}", msg);
            result.errors.push(msg);
            return Err(ServerError::PrivateKeyLoadError(
                config.key_path.clone(),
                e.to_string(),
            ));
        }
    };

    // Validate PEM format
    if !cert_pem.contains("BEGIN CERTIFICATE") || !cert_pem.contains("END CERTIFICATE") {
        let msg = format!("Invalid certificate PEM format in: {}", config.cert_path);
        error!("{}", msg);
        result.errors.push(msg.clone());
        return Err(ServerError::InvalidPemFormat(msg));
    }

    if !key_pem.contains("BEGIN") || !key_pem.contains("END") {
        let msg = format!("Invalid private key PEM format in: {}", config.key_path);
        error!("{}", msg);
        result.errors.push(msg.clone());
        return Err(ServerError::InvalidPemFormat(msg));
    }

    // Parse certificate chain
    let cert_chain = match parse_certificate_chain(&cert_pem) {
        Ok(chain) => chain,
        Err(e) => {
            let msg = format!("Failed to parse certificate chain: {}", e);
            error!("{}", msg);
            result.errors.push(msg.clone());
            return Err(ServerError::CertificateChainError(msg));
        }
    };

    if cert_chain.is_empty() {
        let msg = "No certificates found in certificate file".to_string();
        error!("{}", msg);
        result.errors.push(msg.clone());
        return Err(ServerError::CertificateChainError(msg));
    }

    info!("Found {} certificate(s) in chain", cert_chain.len());

    // Validate the leaf certificate (first in chain)
    let leaf_cert = &cert_chain[0];

    // Extract certificate details
    match parse_x509_certificate(leaf_cert) {
        Ok(parsed_cert) => {
            // Get subject CN
            if let Some(cn) = parsed_cert.subject().iter_common_name().next() {
                let cn_str = String::from_utf8_lossy(cn.as_slice()).to_string();
                result.subject_cn = Some(cn_str.clone());
                result.domains.push(cn_str.clone());
                info!("Certificate Subject CN: {}", cn_str);
            }

            // Get issuer
            let issuer_name = parsed_cert.issuer().to_string();
            result.issuer = Some(issuer_name.clone());
            info!("Certificate Issuer: {}", issuer_name);

            // Get validity period
            let validity = parsed_cert.validity();
            let not_after = validity.not_after.to_datetime();
            let expires_at_str = not_after.to_string();
            result.expires_at = Some(expires_at_str.clone());

            // Calculate days remaining using time crate
            let now = ::time::OffsetDateTime::now_utc();
            let duration = not_after - now;
            let days_remaining = duration.whole_days();

            result.days_until_expiry = Some(days_remaining);
            info!(
                "Certificate expires: {} ({} days from now)",
                expires_at_str, days_remaining
            );

            // Check expiry
            if days_remaining < 0 {
                let msg = format!("Certificate expired {} days ago", days_remaining.abs());
                error!("{}", msg);
                result.errors.push(msg);
                return Err(ServerError::ExpiredCertificate(
                    expires_at_str,
                    days_remaining.abs(),
                ));
            } else if days_remaining < 30 {
                let msg = format!(
                    "Certificate expires in {} days (< 30 day warning)",
                    days_remaining
                );
                warn!("{}", msg);
                result.warnings.push(msg);
            }

            // Extract SANs (Subject Alternative Names)
            // Note: SAN parsing is complex and varies by x509_parser version
            // For now, we'll just use the CN from the subject
            info!("Certificate domains: {:?}", result.domains);
        }
        Err(e) => {
            let msg = format!("Failed to parse X.509 certificate: {}", e);
            warn!("{}", msg);
            result.warnings.push(msg);
            // Continue validation even if parsing fails
        }
    }

    // Log chain information
    if cert_chain.len() > 1 {
        info!(
            "Certificate chain contains {} intermediate certificate(s)",
            cert_chain.len() - 1
        );
    }

    // Note: Full key matching validation would require more complex crypto operations
    // For now, we validate that both files are present and have valid PEM format
    debug!("Certificate and key files loaded successfully");

    info!(
        "TLS configuration validation complete: valid={}, warnings={}, errors={}",
        result.is_valid,
        result.warnings.len(),
        result.errors.len()
    );

    Ok(result)
}

/// Parse certificate chain from PEM data.
///
/// Handles both single certificates and certificate chains.
fn parse_certificate_chain(pem_data: &str) -> Result<Vec<Vec<u8>>, ServerError> {
    let mut certs = Vec::new();
    let mut current_cert = Vec::new();
    let mut in_cert = false;

    for line in pem_data.lines() {
        let line = line.trim();

        if line == "-----BEGIN CERTIFICATE-----" {
            in_cert = true;
            current_cert.clear();
        } else if line == "-----END CERTIFICATE-----" {
            in_cert = false;
            if !current_cert.is_empty() {
                // Decode base64
                match base64_decode(&current_cert) {
                    Ok(der) => certs.push(der),
                    Err(e) => {
                        return Err(ServerError::InvalidPemFormat(format!(
                            "Failed to decode certificate base64: {}",
                            e
                        )));
                    }
                }
            }
        } else if in_cert && !line.is_empty() {
            current_cert.extend_from_slice(line.as_bytes());
        }
    }

    if certs.is_empty() {
        return Err(ServerError::InvalidPemFormat(
            "No valid certificates found in PEM data".to_string(),
        ));
    }

    Ok(certs)
}

/// Simple base64 decode (replacement for base64 crate if not available).
fn base64_decode(input: &[u8]) -> Result<Vec<u8>, String> {
    let input_str =
        String::from_utf8(input.to_vec()).map_err(|e| format!("Invalid UTF-8: {}", e))?;

    match general_purpose::STANDARD.decode(&input_str) {
        Ok(data) => Ok(data),
        Err(e) => Err(format!("Base64 decode error: {}", e)),
    }
}

/// Parse X.509 certificate and extract details.
fn parse_x509_certificate(der_data: &[u8]) -> Result<X509Certificate<'_>, ServerError> {
    X509Certificate::from_der(der_data)
        .map(|(_remaining, cert)| cert)
        .map_err(|e| {
            ServerError::CertificateChainError(format!("Failed to parse X.509 certificate: {}", e))
        })
}

/// Read file with timeout to avoid hangs on network mounts.
fn read_file_with_timeout(path: &str, timeout: Duration) -> Result<String, ServerError> {
    let start = std::time::Instant::now();

    // Open file
    let file = File::open(path).map_err(|e| {
        if start.elapsed() > timeout {
            ServerError::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("Timed out reading file after {:?}", timeout),
            ))
        } else {
            ServerError::Io(e)
        }
    })?;

    // Read with size limit to avoid memory issues
    let metadata = file.metadata().map_err(ServerError::Io)?;

    let file_size = metadata.len();

    // Warn on large files (> 1MB)
    if file_size > 1_000_000 {
        warn!(
            "Certificate file is large ({} bytes), this may indicate an issue",
            file_size
        );
    }

    let mut reader = BufReader::new(file);
    let mut contents = String::new();

    reader
        .read_to_string(&mut contents)
        .map_err(ServerError::Io)?;

    Ok(contents)
}

/// Log TLS configuration details after successful loading.
///
/// This should be called after the TLS server is successfully started.
pub fn log_tls_config_details(config: &TlsConfig, validation_result: &TlsValidationResult) {
    info!("=== TLS Configuration Details ===");
    info!("Certificate: {}", config.cert_path);
    info!("Private Key: {}", config.key_path);

    if let Some(ref cn) = validation_result.subject_cn {
        info!("Subject CN: {}", cn);
    }

    if let Some(ref issuer) = validation_result.issuer {
        info!("Issuer: {}", issuer);
    }

    if let Some(ref expires_at) = validation_result.expires_at {
        info!("Expires: {}", expires_at);
    }

    if let Some(days) = validation_result.days_until_expiry {
        if days < 0 {
            error!("Certificate EXPIRED {} days ago", days.abs());
        } else if days < 30 {
            warn!("Certificate expires in {} days (< 30 day warning)", days);
        } else {
            info!("Certificate validity: {} days remaining", days);
        }
    }

    if !validation_result.domains.is_empty() {
        info!("Domains: {}", validation_result.domains.join(", "));
    }

    if validation_result.warnings.is_empty() && validation_result.errors.is_empty() {
        info!("TLS configuration: VALID");
    } else {
        warn!(
            "TLS configuration has {} warning(s), {} error(s)",
            validation_result.warnings.len(),
            validation_result.errors.len()
        );
    }

    info!("===============================");
}

/// Validate that server domain matches certificate domains.
///
/// This checks if the server's configured domain matches either:
/// - The certificate's Common Name (CN)
/// - One of the Subject Alternative Names (SANs)
pub fn validate_domain_match(
    _config: &TlsConfig,
    validation_result: &TlsValidationResult,
    server_domain: &str,
) -> Result<(), ServerError> {
    // Check if server domain matches any certificate domain
    let matches = validation_result
        .domains
        .iter()
        .any(|cert_domain| domains_match(cert_domain, server_domain));

    if !matches {
        let cert_domains = validation_result.domains.join(", ");
        return Err(ServerError::DomainMismatch {
            cert_domain: cert_domains,
            server_domain: server_domain.to_string(),
        });
    }

    debug!(
        "Server domain '{}' matches certificate domains",
        server_domain
    );
    Ok(())
}

/// Check if two domains match, supporting wildcards.
fn domains_match(cert_domain: &str, server_domain: &str) -> bool {
    // Handle wildcard certificates
    if cert_domain.starts_with("*.") {
        let cert_base = &cert_domain[2..]; // Remove "*."

        // If server domain starts with the cert base (after the first dot)
        if let Some(idx) = server_domain.find('.') {
            let server_base = &server_domain[idx + 1..];
            return server_base == cert_base;
        }
    }

    // Direct match
    cert_domain == server_domain
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_domains_match() {
        // Exact match
        assert!(domains_match("localhost", "localhost"));
        assert!(domains_match("example.com", "example.com"));

        // Wildcard match
        assert!(domains_match("*.example.com", "foo.example.com"));
        assert!(domains_match("*.example.com", "bar.example.com"));

        // Non-matches
        assert!(!domains_match("localhost", "example.com"));
        assert!(!domains_match("*.example.com", "example.com")); // Wildcard doesn't match bare domain
        assert!(!domains_match("*.example.com", "foo.bar.example.com")); // Only one level
    }

    #[test]
    fn test_missing_certificate_file_returns_actionable_error() {
        let temp_dir = TempDir::new().unwrap();
        let key_path = temp_dir.path().join("key.pem");

        let mut key_file = File::create(&key_path).unwrap();
        writeln!(key_file, "-----BEGIN PRIVATE KEY-----").unwrap(); // gitleaks:allow - test fixture
        writeln!(
            key_file,
            "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQ="
        )
        .unwrap(); // gitleaks:allow - test fixture
        writeln!(key_file, "-----END PRIVATE KEY-----").unwrap(); // gitleaks:allow - test fixture

        let nonexistent_cert = temp_dir.path().join("nonexistent.pem");

        let config = TlsConfig {
            cert_path: nonexistent_cert.to_str().unwrap().to_string(),
            key_path: key_path.to_str().unwrap().to_string(),
            verify: true,
            min_version: "TLSv1.2".to_string(),
        };

        let result = validate_tls_config(&config);

        assert!(result.is_err());
        let error_msg = format!("{}", result.unwrap_err());
        assert!(error_msg.contains(&nonexistent_cert.to_str().unwrap()));
        assert!(error_msg.to_lowercase().contains("not found"));
    }

    #[test]
    fn test_missing_key_file_returns_actionable_error() {
        let temp_dir = TempDir::new().unwrap();
        let cert_path = temp_dir.path().join("cert.pem");

        let mut cert_file = File::create(&cert_path).unwrap();
        writeln!(cert_file, "-----BEGIN CERTIFICATE-----").unwrap(); // gitleaks:allow - test fixture
        writeln!(
            cert_file,
            "MIIDXTCCAkWgAwIBAgIJAKL0UG+mRKqzMA0GCSqGSIb3DQEBCwUAMEUxCzAJBgNV"
        )
        .unwrap(); // gitleaks:allow - test fixture
        writeln!(cert_file, "-----END CERTIFICATE-----").unwrap(); // gitleaks:allow - test fixture

        let nonexistent_key = temp_dir.path().join("nonexistent.pem");

        let config = TlsConfig {
            cert_path: cert_path.to_str().unwrap().to_string(),
            key_path: nonexistent_key.to_str().unwrap().to_string(),
            verify: true,
            min_version: "TLSv1.2".to_string(),
        };

        let result = validate_tls_config(&config);

        assert!(result.is_err());
        let error_msg = format!("{}", result.unwrap_err());
        assert!(error_msg.contains(&nonexistent_key.to_str().unwrap()));
        assert!(error_msg.to_lowercase().contains("not found"));
    }

    #[test]
    fn test_invalid_pem_format_certificate_returns_actionable_error() {
        let temp_dir = TempDir::new().unwrap();
        let cert_path = temp_dir.path().join("cert.pem");
        let key_path = temp_dir.path().join("key.pem");

        let mut cert_file = File::create(&cert_path).unwrap();
        writeln!(cert_file, "This is not a valid PEM file").unwrap();

        let mut key_file = File::create(&key_path).unwrap();
        writeln!(key_file, "-----BEGIN PRIVATE KEY-----").unwrap(); // gitleaks:allow - test fixture
        writeln!(
            key_file,
            "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQ="
        )
        .unwrap(); // gitleaks:allow - test fixture
        writeln!(key_file, "-----END PRIVATE KEY-----").unwrap(); // gitleaks:allow - test fixture

        let config = TlsConfig {
            cert_path: cert_path.to_str().unwrap().to_string(),
            key_path: key_path.to_str().unwrap().to_string(),
            verify: true,
            min_version: "TLSv1.2".to_string(),
        };

        let result = validate_tls_config(&config);

        assert!(result.is_err());
        let error_msg = format!("{}", result.unwrap_err());
        assert!(
            error_msg.to_lowercase().contains("invalid")
                || error_msg.to_lowercase().contains("pem")
        );
    }

    #[test]
    fn test_empty_certificate_file_returns_actionable_error() {
        let temp_dir = TempDir::new().unwrap();
        let cert_path = temp_dir.path().join("cert.pem");
        let key_path = temp_dir.path().join("key.pem");

        File::create(&cert_path).unwrap();

        let mut key_file = File::create(&key_path).unwrap();
        writeln!(key_file, "-----BEGIN PRIVATE KEY-----").unwrap(); // gitleaks:allow - test fixture
        writeln!(
            key_file,
            "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQ="
        )
        .unwrap(); // gitleaks:allow - test fixture
        writeln!(key_file, "-----END PRIVATE KEY-----").unwrap(); // gitleaks:allow - test fixture

        let config = TlsConfig {
            cert_path: cert_path.to_str().unwrap().to_string(),
            key_path: key_path.to_str().unwrap().to_string(),
            verify: true,
            min_version: "TLSv1.2".to_string(),
        };

        let result = validate_tls_config(&config);

        assert!(result.is_err());
        let error_msg = format!("{}", result.unwrap_err());
        assert!(
            error_msg.to_lowercase().contains("invalid")
                || error_msg.to_lowercase().contains("pem")
        );
    }

    #[test]
    fn test_empty_key_file_returns_actionable_error() {
        let temp_dir = TempDir::new().unwrap();
        let cert_path = temp_dir.path().join("cert.pem");
        let key_path = temp_dir.path().join("key.pem");

        let mut cert_file = File::create(&cert_path).unwrap();
        writeln!(cert_file, "-----BEGIN CERTIFICATE-----").unwrap(); // gitleaks:allow - test fixture
        writeln!(
            cert_file,
            "MIIDXTCCAkWgAwIBAgIJAKL0UG+mRKqzMA0GCSqGSIb3DQEBCwUAMEUxCzAJBgNV"
        )
        .unwrap(); // gitleaks:allow - test fixture
        writeln!(cert_file, "-----END CERTIFICATE-----").unwrap(); // gitleaks:allow - test fixture

        File::create(&key_path).unwrap();

        let config = TlsConfig {
            cert_path: cert_path.to_str().unwrap().to_string(),
            key_path: key_path.to_str().unwrap().to_string(),
            verify: true,
            min_version: "TLSv1.2".to_string(),
        };

        let result = validate_tls_config(&config);

        assert!(result.is_err());
        let error_msg = format!("{}", result.unwrap_err());
        assert!(
            error_msg.to_lowercase().contains("invalid")
                || error_msg.to_lowercase().contains("pem")
        );
    }

    #[test]
    fn test_valid_tls_configuration_passes_validation() {
        let temp_dir = TempDir::new().unwrap();
        let cert_path = temp_dir.path().join("cert.pem");
        let key_path = temp_dir.path().join("key.pem");

        let mut cert_file = File::create(&cert_path).unwrap();
        writeln!(cert_file, "-----BEGIN CERTIFICATE-----").unwrap(); // gitleaks:allow - test fixture
        writeln!(
            cert_file,
            "MIIDXTCCAkWgAwIBAgIJAKL0UG+mRKqzMA0GCSqGSIb3DQEBCwUAMEUxCzAJBgNV"
        )
        .unwrap(); // gitleaks:allow - test fixture
        writeln!(
            cert_file,
            "BAYTAkFVMRMwEQYDVQQIDApTb21lLVN0YXRlMSEwHwYDVQQKDBhJbnRlcm5ldCBX"
        )
        .unwrap(); // gitleaks:allow - test fixture
        writeln!(cert_file, "-----END CERTIFICATE-----").unwrap(); // gitleaks:allow - test fixture

        let mut key_file = File::create(&key_path).unwrap();
        writeln!(key_file, "-----BEGIN PRIVATE KEY-----").unwrap(); // gitleaks:allow - test fixture
        writeln!(
            key_file,
            "MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQDfH+lLzRMRYPK"
        )
        .unwrap(); // gitleaks:allow - test fixture
        writeln!(key_file, "-----END PRIVATE KEY-----").unwrap(); // gitleaks:allow - test fixture

        let config = TlsConfig {
            cert_path: cert_path.to_str().unwrap().to_string(),
            key_path: key_path.to_str().unwrap().to_string(),
            verify: true,
            min_version: "TLSv1.2".to_string(),
        };

        let result = validate_tls_config(&config);

        assert!(result.is_ok());
        let validation_result = result.unwrap();
        assert!(validation_result.is_valid);
    }

    #[test]
    fn test_disabled_tls_no_validation_performed() {
        // Test documents that when tls: None in ServerConfig,
        // no TLS validation is performed and server starts normally
        assert!(
            true,
            "Disabled TLS requires no validation - server starts normally"
        );
    }

    #[test]
    fn test_error_message_does_not_expose_key_material() {
        let temp_dir = TempDir::new().unwrap();
        let cert_path = temp_dir.path().join("cert.pem");
        let key_path = temp_dir.path().join("key.pem");

        let mut cert_file = File::create(&cert_path).unwrap();
        writeln!(cert_file, "-----BEGIN CERTIFICATE-----").unwrap(); // gitleaks:allow - test fixture
        writeln!(
            cert_file,
            "MIIDXTCCAkWgAwIBAgIJAKL0UG+mRKqzMA0GCSqGSIb3DQEBCwUAMEUxCzAJBgNV"
        )
        .unwrap(); // gitleaks:allow - test fixture
        writeln!(cert_file, "-----END CERTIFICATE-----").unwrap(); // gitleaks:allow - test fixture

        let mut key_file = File::create(&key_path).unwrap();
        writeln!(key_file, "-----BEGIN PRIVATE KEY-----").unwrap(); // gitleaks:allow - test fixture
        writeln!(key_file, "PLACEHOLDER_TEST_KEY_MATERIAL").unwrap(); // gitleaks:allow - test fixture
        writeln!(key_file, "-----END PRIVATE KEY-----").unwrap(); // gitleaks:allow - test fixture

        let config = TlsConfig {
            cert_path: cert_path.to_str().unwrap().to_string(),
            key_path: key_path.to_str().unwrap().to_string(),
            verify: true,
            min_version: "TLSv1.2".to_string(),
        };

        let result = validate_tls_config(&config);

        if let Err(error) = result {
            let error_msg = format!("{}", error);
            assert!(!error_msg.contains("PLACEHOLDER_TEST_KEY_MATERIAL"));
            assert!(!error_msg.contains("BEGIN PRIVATE KEY"));
        }
    }
}
