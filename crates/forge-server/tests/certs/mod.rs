//! Test certificate generation utilities for TLS testing.
//!
//! Provides functions to generate self-signed X.509 certificates for testing
//! WebSocket Secure (WSS) connections.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};

/// Generate a self-signed certificate and private key for testing.
///
/// # Arguments
/// * `cert_path` - Path where the certificate PEM file will be written
/// * `key_path` - Path where the private key PEM file will be written
/// * `common_name` - Common name for the certificate (e.g., "localhost")
///
/// # Returns
/// Result indicating success or failure
pub fn generate_test_cert(
    cert_path: &Path,
    key_path: &Path,
    common_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    generate_test_cert_with_validity(cert_path, key_path, common_name, 365)
}

/// Generate a self-signed certificate and private key with custom validity period.
///
/// # Arguments
/// * `cert_path` - Path where the certificate PEM file will be written
/// * `key_path` - Path where the private key PEM file will be written
/// * `common_name` - Common name for the certificate (e.g., "localhost")
/// * `days_valid` - Number of days the certificate should be valid (can be negative for expired certs)
///
/// # Returns
/// Result indicating success or failure
pub fn generate_test_cert_with_validity(
    cert_path: &Path,
    key_path: &Path,
    common_name: &str,
    days_valid: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut params = CertificateParams::default();

    // Set distinguished name
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    dn.push(DnType::OrganizationName, "FORGE Test");
    dn.push(DnType::OrganizationalUnitName, "Development");
    params.distinguished_name = dn;

    // Set validity period
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1); // Start yesterday to avoid clock skew
    params.not_after = now + time::Duration::days(days_valid);

    // Add Subject Alternative Names for localhost and 127.0.0.1
    params.subject_alt_names = vec![
        SanType::DnsName("localhost".try_into()?),
        SanType::DnsName("*.localhost".try_into()?),
        SanType::IpAddress("127.0.0.1".parse()?),
        SanType::IpAddress("::1".parse()?),
    ];

    // Generate the certificate
    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    // Write certificate to file
    let cert_pem = cert.pem();
    let mut cert_file = File::create(cert_path)?;
    cert_file.write_all(cert_pem.as_bytes())?;

    // Write private key to file
    let key_pem = key_pair.serialize_pem();
    let mut key_file = File::create(key_path)?;
    key_file.write_all(key_pem.as_bytes())?;

    Ok(())
}

/// Certificate file names for testing
pub const TEST_CERT_FILE: &str = "test-cert.pem";
pub const TEST_KEY_FILE: &str = "test-key.pem";

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_generate_cert() {
        let temp_dir = TempDir::new().unwrap();
        let cert_path = temp_dir.path().join(TEST_CERT_FILE);
        let key_path = temp_dir.path().join(TEST_KEY_FILE);

        generate_test_cert(&cert_path, &key_path, "localhost").unwrap();

        // Verify files exist
        assert!(cert_path.exists());
        assert!(key_path.exists());

        // Verify file contents
        let cert_contents = std::fs::read_to_string(&cert_path).unwrap();
        assert!(cert_contents.contains("BEGIN CERTIFICATE"));
        assert!(cert_contents.contains("END CERTIFICATE"));

        let key_contents = std::fs::read_to_string(&key_path).unwrap();
        assert!(key_contents.contains("BEGIN PRIVATE KEY"));
        assert!(key_contents.contains("END PRIVATE KEY"));
    }

    #[test]
    fn test_generate_expired_cert() {
        let temp_dir = TempDir::new().unwrap();
        let cert_path = temp_dir.path().join("expired-cert.pem");
        let key_path = temp_dir.path().join("expired-key.pem");

        // Generate certificate that expired yesterday
        generate_test_cert_with_validity(&cert_path, &key_path, "localhost", -1).unwrap();

        // Verify files exist
        assert!(cert_path.exists());
        assert!(key_path.exists());

        // Verify file contents
        let cert_contents = std::fs::read_to_string(&cert_path).unwrap();
        assert!(cert_contents.contains("BEGIN CERTIFICATE"));
        assert!(cert_contents.contains("END CERTIFICATE"));
    }
}
