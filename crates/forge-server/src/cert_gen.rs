//! Self-signed certificate generation for development and testing.
//!
//! Certificates produced by this module are not suitable for production. They
//! are intended to make local TLS/WSS development possible without a CA.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::net::IpAddr;
use std::path::Path;

use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, KeyPair,
    KeyUsagePurpose, SanType,
};

use crate::ServerError;

/// Default lifetime for generated certificates.
pub const DEFAULT_CERT_VALIDITY_DAYS: u32 = 365;

const MAX_CERT_VALIDITY_DAYS: u32 = 36_500;
const CLOCK_SKEW_ALLOWANCE_MINUTES: i64 = 5;

/// Generate a self-signed TLS certificate for `domain`.
///
/// The returned tuple contains `(cert_pem, key_pem)`. The certificate is valid
/// for 365 days and includes `domain` as a DNS or IP subject alternative name.
pub fn generate_self_signed_cert(domain: &str) -> Result<(String, String), ServerError> {
    generate_self_signed_cert_with_validity(domain, DEFAULT_CERT_VALIDITY_DAYS)
}

/// Generate a self-signed TLS certificate with a custom validity period.
///
/// This is used by the CLI's `--days` option. Most callers should use
/// [`generate_self_signed_cert`], which applies the default lifetime.
pub fn generate_self_signed_cert_with_validity(
    domain: &str,
    days_valid: u32,
) -> Result<(String, String), ServerError> {
    if domain.is_empty() {
        return Err(ServerError::InvalidRequest(
            "Domain cannot be empty".to_string(),
        ));
    }

    if !(1..=MAX_CERT_VALIDITY_DAYS).contains(&days_valid) {
        return Err(ServerError::InvalidRequest(format!(
            "Days valid must be between 1 and {MAX_CERT_VALIDITY_DAYS}"
        )));
    }

    let mut params = CertificateParams::default();

    let mut distinguished_name = DistinguishedName::new();
    distinguished_name.push(DnType::CommonName, domain);
    distinguished_name.push(DnType::OrganizationName, "FORGE Development");
    params.distinguished_name = distinguished_name;

    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::minutes(CLOCK_SKEW_ALLOWANCE_MINUTES);
    params.not_after = now
        .checked_add(time::Duration::days(i64::from(days_valid)))
        .ok_or_else(|| ServerError::ServerError("Invalid validity period".to_string()))?;

    params.subject_alt_names = match domain.parse::<IpAddr>() {
        Ok(ip) => vec![SanType::IpAddress(ip)],
        Err(_) => vec![SanType::DnsName(domain.try_into().map_err(|_| {
            ServerError::InvalidRequest(format!("Invalid DNS name: {domain}"))
        })?)],
    };
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

    let key_pair = KeyPair::generate()
        .map_err(|error| ServerError::ServerError(format!("Key generation failed: {error}")))?;
    let certificate = params.self_signed(&key_pair).map_err(|error| {
        ServerError::ServerError(format!("Certificate generation failed: {error}"))
    })?;

    Ok((certificate.pem(), key_pair.serialize_pem()))
}

/// Write a certificate and private key to disk.
///
/// Parent directories are created when needed. On Unix, the certificate is
/// mode `0644` and the private key is mode `0600`. Permissions are applied to
/// an existing key file before new key material is written.
pub fn write_cert_files(
    cert_path: &Path,
    key_path: &Path,
    cert_pem: &str,
    key_pem: &str,
) -> Result<(), ServerError> {
    create_parent_directory(cert_path)?;
    create_parent_directory(key_path)?;

    let mut cert_file = open_with_mode(cert_path, 0o644)?;
    cert_file.write_all(cert_pem.as_bytes())?;

    let mut key_file = open_with_mode(key_path, 0o600)?;
    key_file.write_all(key_pem.as_bytes())?;

    Ok(())
}

fn create_parent_directory(path: &Path) -> Result<(), ServerError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn open_with_mode(path: &Path, _mode: u32) -> Result<File, ServerError> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(_mode);
    }

    let file = options.open(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(_mode))?;
    }

    Ok(file)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use ::time::{Duration, OffsetDateTime};
    use rustls::pki_types::CertificateDer;
    use rustls_pemfile::{certs, private_key};
    use tempfile::TempDir;
    use x509_parser::extensions::GeneralName;
    use x509_parser::prelude::*;

    use super::*;

    fn certificate_der(cert_pem: &str) -> CertificateDer<'static> {
        let mut reader = Cursor::new(cert_pem);
        let parsed = certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .expect("certificate PEM should parse");
        assert_eq!(parsed.len(), 1, "expected one certificate");
        parsed.into_iter().next().unwrap()
    }

    #[test]
    fn generates_basic_certificate() {
        let (cert_pem, key_pem) =
            generate_self_signed_cert("localhost").expect("certificate generation should succeed");

        let cert_der = certificate_der(&cert_pem);
        let (_, certificate) = X509Certificate::from_der(cert_der.as_ref())
            .expect("generated DER should be a valid X.509 certificate");
        let common_name = certificate
            .subject()
            .iter_common_name()
            .next()
            .expect("certificate should have a common name")
            .as_str()
            .expect("common name should be UTF-8");
        assert_eq!(common_name, "localhost");

        let mut key_reader = Cursor::new(key_pem);
        assert!(
            private_key(&mut key_reader)
                .expect("private-key PEM should parse")
                .is_some()
        );
    }

    #[test]
    fn honors_requested_validity_period() {
        const DAYS: u32 = 30;
        let generated_after = OffsetDateTime::now_utc();
        let (cert_pem, _) = generate_self_signed_cert_with_validity("localhost", DAYS)
            .expect("certificate generation should succeed");
        let generated_before = OffsetDateTime::now_utc();

        let cert_der = certificate_der(&cert_pem);
        let (_, certificate) = X509Certificate::from_der(cert_der.as_ref())
            .expect("generated DER should be a valid X.509 certificate");
        let not_after = certificate.validity().not_after.to_datetime();

        // ASN.1 timestamps are encoded with one-second precision.
        let tolerance = Duration::seconds(2);
        assert!(not_after >= generated_after + Duration::days(i64::from(DAYS)) - tolerance);
        assert!(not_after <= generated_before + Duration::days(i64::from(DAYS)) + tolerance);
    }

    #[test]
    fn encodes_requested_domains_as_subject_alternative_names() {
        for domain in ["localhost", "example.com"] {
            let (cert_pem, _) = generate_self_signed_cert(domain)
                .unwrap_or_else(|error| panic!("generation failed for {domain}: {error}"));
            let cert_der = certificate_der(&cert_pem);
            let (_, certificate) = X509Certificate::from_der(cert_der.as_ref())
                .expect("generated DER should be a valid X.509 certificate");
            let names = &certificate
                .subject_alternative_name()
                .expect("SAN extension should parse")
                .expect("SAN extension should be present")
                .value
                .general_names;

            assert!(
                names
                    .iter()
                    .any(|name| matches!(name, GeneralName::DNSName(name) if *name == domain)),
                "certificate should contain DNS SAN {domain}"
            );
        }

        let (cert_pem, _) = generate_self_signed_cert("127.0.0.1")
            .expect("IP certificate generation should succeed");
        let cert_der = certificate_der(&cert_pem);
        let (_, certificate) = X509Certificate::from_der(cert_der.as_ref())
            .expect("generated DER should be a valid X.509 certificate");
        let names = &certificate
            .subject_alternative_name()
            .expect("SAN extension should parse")
            .expect("SAN extension should be present")
            .value
            .general_names;
        assert!(
            names.iter().any(
                |name| matches!(name, GeneralName::IPAddress(bytes) if *bytes == [127, 0, 0, 1])
            )
        );
    }

    #[test]
    fn outputs_pem_formatted_certificate_and_key() {
        let (cert_pem, key_pem) =
            generate_self_signed_cert("localhost").expect("certificate generation should succeed");

        assert!(cert_pem.starts_with("-----BEGIN CERTIFICATE-----\n"));
        assert!(cert_pem.ends_with("-----END CERTIFICATE-----\n"));
        assert!(key_pem.starts_with("-----BEGIN PRIVATE KEY-----\n")); // gitleaks:allow
        assert!(key_pem.ends_with("-----END PRIVATE KEY-----\n"));
    }

    #[test]
    fn rejects_invalid_generation_options() {
        assert!(generate_self_signed_cert("").is_err());
        assert!(generate_self_signed_cert_with_validity("localhost", 0).is_err());
        assert!(
            generate_self_signed_cert_with_validity("localhost", MAX_CERT_VALIDITY_DAYS + 1)
                .is_err()
        );
    }

    #[test]
    fn writes_loadable_files_with_restricted_key_permissions() {
        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let cert_path = temp_dir.path().join("certificates/server-cert.pem");
        let key_path = temp_dir.path().join("private/server-key.pem");
        let (cert_pem, key_pem) =
            generate_self_signed_cert("localhost").expect("certificate generation should succeed");

        fs::create_dir_all(key_path.parent().unwrap()).unwrap();
        fs::write(&key_path, "stale key material").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&key_path, fs::Permissions::from_mode(0o644)).unwrap();
        }

        write_cert_files(&cert_path, &key_path, &cert_pem, &key_pem)
            .expect("certificate files should be written");

        let written_cert = fs::read_to_string(&cert_path).expect("certificate should be readable");
        let written_key = fs::read_to_string(&key_path).expect("private key should be readable");
        certificate_der(&written_cert);
        let mut key_reader = Cursor::new(&written_key);
        assert!(
            private_key(&mut key_reader)
                .expect("written private key should parse")
                .is_some()
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let cert_mode = fs::metadata(&cert_path).unwrap().permissions().mode() & 0o777;
            let key_mode = fs::metadata(&key_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(cert_mode, 0o644);
            assert_eq!(key_mode, 0o600);
        }
    }
}
