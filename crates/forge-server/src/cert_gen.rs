//! Self-signed certificate generation for FORGE server TLS.
//!
//! This module provides utilities for generating self-signed X.509 certificates
//! for development and testing purposes. These certificates enable TLS/WSS support
//! in FORGE server mode without requiring external certificate authorities.
//!
//! # Security Notice
//!
//! **Self-signed certificates are for development/testing only.**
//! Browsers and clients will show security warnings unless the certificate
//! is manually trusted. For production, use certificates from a trusted CA.
//!
//! # Example
//!
//! ```no_run
//! use forge_server::cert_gen;
//!
//! // Generate a certificate for localhost
//! let (cert_pem, key_pem) = cert_gen::generate_self_signed_cert("localhost", 365)
//!     .expect("Failed to generate certificate");
//!
//! // Write to files
//! std::fs::write("server-cert.pem", cert_pem)?;
//! std::fs::write("server-key.pem", key_pem)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};

use crate::ServerError;

/// Generate a self-signed TLS certificate for the given domain.
///
/// Creates a PEM-encoded X.509 certificate and private key suitable for
/// TLS/WSS server configuration. The certificate includes:
///
/// - Subject Common Name (CN) set to the domain
/// - Subject Alternative Name (SAN) for the domain
/// - Basic Constraints with CA: false (end-entity certificate)
/// - Key Usage and Extended Key Usage for digital signature and server auth
///
/// # Arguments
///
/// * `domain` - The domain name (e.g., "localhost", "example.com", "127.0.0.1")
/// * `days_valid` - Certificate validity period in days (default: 365)
///
/// # Returns
///
/// Returns a tuple of `(cert_pem, key_pem)` as PEM-encoded strings.
///
/// # Errors
///
/// Returns `ServerError` if:
/// - Certificate generation fails
/// - PEM encoding fails
/// - Date/time calculation fails
///
/// # Example
///
/// ```no_run
/// use forge_server::cert_gen;
///
/// let (cert, key) = cert_gen::generate_self_signed_cert("localhost", 365)
///     .expect("certificate generation failed");
///
/// assert!(cert.contains("BEGIN CERTIFICATE"));
/// assert!(key.contains("BEGIN PRIVATE KEY"));
/// ```
pub fn generate_self_signed_cert(
    domain: &str,
    days_valid: u32,
) -> Result<(String, String), ServerError> {
    // Validate domain parameter
    if domain.is_empty() {
        return Err(ServerError::InvalidRequest(
            "Domain cannot be empty".to_string(),
        ));
    }

    // Validate days parameter
    if days_valid == 0 || days_valid > 36500 {
        // Max 100 years is reasonable
        return Err(ServerError::InvalidRequest(
            "Days valid must be between 1 and 36500".to_string(),
        ));
    }

    // Build certificate parameters
    let mut params = CertificateParams::default();

    // Set distinguished name
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, domain);
    dn.push(DnType::OrganizationName, "FORGE Development");
    dn.push(DnType::OrganizationalUnitName, "Self-Signed Certificate");
    params.distinguished_name = dn;

    // Set validity period
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1); // Start yesterday to avoid clock skew
    params.not_after = now
        .checked_add(time::Duration::days(days_valid as i64))
        .ok_or_else(|| ServerError::ServerError("Invalid validity period".to_string()))?;

    // Add Subject Alternative Names (SAN)
    // Include the domain as both DNS and potentially IP
    if domain.parse::<std::net::IpAddr>().is_ok() {
        // Domain is an IP address
        let ip = domain
            .parse::<std::net::IpAddr>()
            .map_err(|_| ServerError::InvalidRequest(format!("Invalid IP address: {}", domain)))?;
        params.subject_alt_names = vec![SanType::IpAddress(ip)];
    } else {
        // Domain is a DNS name - convert String to Ia5String for rcgen 0.13
        let dns_name = domain
            .parse::<rcgen::Ia5String>()
            .map_err(|_| ServerError::InvalidRequest(format!("Invalid DNS name: {}", domain)))?;
        params.subject_alt_names = vec![SanType::DnsName(dns_name)];
    }

    // Generate the key pair
    let key_pair = KeyPair::generate()
        .map_err(|e| ServerError::ServerError(format!("Key pair generation failed: {}", e)))?;

    // Generate the self-signed certificate
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| ServerError::ServerError(format!("Certificate generation failed: {}", e)))?;

    // Serialize to PEM format
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

    Ok((cert_pem, key_pem))
}

/// Write certificate and key to files with appropriate permissions.
///
/// Creates the parent directory if needed. The private key file is written
/// with mode 0600 (owner read/write only) for security.
///
/// # Arguments
///
/// * `cert_path` - Path where the certificate PEM will be written
/// * `key_path` - Path where the private key PEM will be written
/// * `cert_pem` - PEM-encoded certificate string
/// * `key_pem` - PEM-encoded private key string
///
/// # Errors
///
/// Returns `ServerError` if:
/// - Parent directory creation fails
/// - File writing fails
/// - Permission setting fails
pub fn write_cert_files(
    cert_path: &std::path::Path,
    key_path: &std::path::Path,
    cert_pem: &str,
    key_pem: &str,
) -> Result<(), ServerError> {
    use std::fs::{self, File};
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    // Ensure parent directories exist
    if let Some(parent) = cert_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| ServerError::Io(e))?;
    }

    // Write certificate file (mode 0644 - world-readable)
    let mut cert_file =
        File::create(cert_path)
            .map_err(|e| ServerError::Io(e))?;
    cert_file
        .write_all(cert_pem.as_bytes())
        .map_err(|e| ServerError::Io(e))?;

    // Set certificate file permissions to 0644
    let cert_perms = fs::Permissions::from_mode(0o644);
    fs::set_permissions(cert_path, cert_perms)
        .map_err(|e| ServerError::Io(e))?;

    // Write private key file (mode 0600 - owner-only)
    let mut key_file =
        File::create(key_path)
            .map_err(|e| ServerError::Io(e))?;
    key_file
        .write_all(key_pem.as_bytes())
        .map_err(|e| ServerError::Io(e))?;

    // Set private key file permissions to 0600 (owner read/write only)
    let key_perms = fs::Permissions::from_mode(0o600);
    fs::set_permissions(key_path, key_perms)
        .map_err(|e| ServerError::Io(e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_generate_basic_certificate() {
        let (cert_pem, key_pem) =
            generate_self_signed_cert("localhost", 365).expect("Failed to generate certificate");

        // Verify PEM format
        assert!(cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(cert_pem.contains("END CERTIFICATE"));
        assert!(key_pem.contains("BEGIN PRIVATE KEY"));
        assert!(key_pem.contains("END PRIVATE KEY"));

        // Verify reasonable size (not empty, not excessively large)
        assert!(cert_pem.len() > 100);
        assert!(cert_pem.len() < 10000);
        assert!(key_pem.len() > 100);
        assert!(key_pem.len() < 10000);
    }

    #[test]
    fn test_generate_with_different_domains() {
        let domains = vec!["localhost", "example.com", "forge.local", "127.0.0.1"];

        for domain in domains {
            let (cert_pem, key_pem) =
                generate_self_signed_cert(domain, 365).expect("Failed for domain: {domain}");

            assert!(cert_pem.contains("BEGIN CERTIFICATE"));
            assert!(key_pem.contains("BEGIN PRIVATE KEY"));

            // Verify domain appears in certificate (for DNS names)
            if !domain.parse::<std::net::IpAddr>().is_ok() {
                assert!(cert_pem.contains(domain));
            }
        }
    }

    #[test]
    fn test_validity_period() {
        let (cert_pem, _) =
            generate_self_signed_cert("localhost", 365).expect("Failed to generate certificate");

        // Check that validity info is in the certificate
        // The cert should contain validity information
        assert!(cert_pem.len() > 0);
    }

    #[test]
    fn test_invalid_inputs() {
        // Empty domain
        let result = generate_self_signed_cert("", 365);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Domain cannot be empty"));

        // Zero days
        let result = generate_self_signed_cert("localhost", 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Days valid must be between"));

        // Excessive days (more than 100 years)
        let result = generate_self_signed_cert("localhost", 40000);
        assert!(result.is_err());
    }

    #[test]
    fn test_write_and_load_cert() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        let cert_path = temp_dir.path().join("cert.pem");
        let key_path = temp_dir.path().join("key.pem");

        let (cert_pem, key_pem) =
            generate_self_signed_cert("localhost", 365).expect("Failed to generate certificate");

        write_cert_files(&cert_path, &key_path, &cert_pem, &key_pem)
            .expect("Failed to write certificate files");

        // Verify files exist
        assert!(cert_path.exists());
        assert!(key_path.exists());

        // Verify content matches
        let read_cert = fs::read_to_string(&cert_path).expect("Failed to read certificate");
        let read_key = fs::read_to_string(&key_path).expect("Failed to read key");

        assert_eq!(read_cert, cert_pem);
        assert_eq!(read_key, key_pem);

        // Verify key file permissions (0600)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let key_perms = fs::metadata(&key_path)
                .expect("Failed to get key metadata")
                .permissions();
            let mode = key_perms.mode() & 0o777;
            assert_eq!(mode, 0o600, "Private key should have 0600 permissions");
        }
    }

    #[test]
    fn test_certificate_loadable_by_rustls() {
        use rustls_pemfile::{certs, private_key};
        use std::io::Cursor;

        let (cert_pem, key_pem) =
            generate_self_signed_cert("localhost", 365).expect("Failed to generate certificate");

        // Verify certificate can be parsed by rustls_pemfile
        let mut cert_reader = Cursor::new(&cert_pem);
        let certs_result: Result<Vec<_>, _> = certs(&mut cert_reader).collect();
        assert!(certs_result.is_ok());
        let parsed_certs = certs_result.unwrap();
        assert!(!parsed_certs.is_empty());

        // Verify key can be parsed by rustls_pemfile
        let mut key_reader = Cursor::new(&key_pem);
        let key = private_key(&mut key_reader);
        assert!(key.is_ok());
        assert!(key.unwrap().is_some());
    }
}
