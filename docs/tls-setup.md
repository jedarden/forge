# TLS/WSS Setup Guide for FORGE Server

This guide explains how to configure TLS (Transport Layer Security) for FORGE server to enable secure WebSocket connections (WSS) in production deployments.

## Overview

FORGE server supports both:
- **WS (WebSocket)**: Unencrypted connections for development/testing
- **WSS (WebSocket Secure)**: TLS-encrypted connections for production

**Always use WSS with valid certificates in production environments.**

## Quick Start

### 1. Start Server with TLS

```bash
forge --server \
  --server-tls \
  --server-tls-cert /path/to/cert.pem \
  --server-tls-key /path/to/key.pem \
  --server-bind 0.0.0.0 \
  --server-port 8080
```

### 2. Connect Clients with WSS

```bash
forge --connect wss://your-server.example.com:8080/ws --user admin --password "***"
```

## Certificate Options

### Option 1: Let's Encrypt (Recommended for Production)

**Pros**: Free, trusted by all browsers, auto-renewal
**Cons**: Requires domain name, cannot use IP addresses

#### Using Certbot

```bash
# Install certbot
sudo apt-get update
sudo apt-get install certbot

# Obtain certificate (standalone mode)
sudo certbot certonly --standalone -d forge.example.com

# Certificate files will be at:
# /etc/letsencrypt/live/forge.example.com/fullchain.pem  <- Use this for --server-tls-cert
# /etc/letsencrypt/live/forge.example.com/privkey.pem     <- Use this for --server-tls-key

# Start FORGE with Let's Encrypt certificates
forge --server \
  --server-tls \
  --server-tls-cert /etc/letsencrypt/live/forge.example.com/fullchain.pem \
  --server-tls-key /etc/letsencrypt/live/forge.example.com/privkey.pem \
  --server-bind 0.0.0.0 \
  --server-port 8080
```

#### Auto-Renewal Setup

Let's Encrypt certificates expire after 90 days. Set up auto-renewal:

```bash
# Test renewal (dry-run)
sudo certbot renew --dry-run

# Certbot creates a systemd timer for auto-renewal
# Verify it's active:
systemctl list-timers | grep certbot

# FORGE will need to be restarted after certificate renewal
# Configure certbot post-renewal hook:
echo -e "#!/bin/bash\nsystemctl restart forge" | sudo tee /etc/letsencrypt/renewal-hooks/post/restart-forge
sudo chmod +x /etc/letsencrypt/renewal-hooks/post/restart-forge
```

### Option 2: Commercial CA Certificate

**Pros**: Validated identity, warranty, support
**Cons**: Costs money, annual renewal

Purchase from providers like:
- DigiCert
- GlobalSign
- SSL.com
- GoDaddy

After purchase, you'll receive certificate files (usually .pem or .crt format) and a private key.

### Option 3: Self-Signed Certificate (Development/Testing)

**Pros**: Free, instant generation
**Cons**: Not trusted by clients, requires manual trust configuration

#### Using the FORGE Development Script

```bash
# Generate self-signed certificate for development
./scripts/generate-dev-cert.sh

# This creates:
# - ~/.forge/tls/cert.pem
# - ~/.forge/tls/key.pem

# Start FORGE with self-signed cert
forge --server \
  --server-tls \
  --server-tls-cert ~/.forge/tls/cert.pem \
  --server-tls-key ~/.forge/tls/key.pem
```

#### Manual Generation with OpenSSL

```bash
# Create directory for certificates
mkdir -p ~/.forge/tls

# Generate private key
openssl genrsa -out ~/.forge/tls/key.pem 2048

# Generate self-signed certificate (valid for 365 days)
openssl req -new -x509 -key ~/.forge/tls/key.pem \
  -out ~/.forge/tls/cert.pem -days 365 \
  -subj "/C=US/ST=State/L=City/O=Organization/CN=localhost"

# View certificate details
openssl x509 -in ~/.forge/tls/cert.pem -text -noout

# Start FORGE
forge --server \
  --server-tls \
  --server-tls-cert ~/.forge/tls/cert.pem \
  --server-tls-key ~/.forge/tls/key.pem
```

#### Trusting Self-Signed Certificates

Clients connecting to a server with a self-signed certificate will get TLS verification errors. You need to:

**Option A: Disable TLS verification (NOT recommended for production)**

```bash
# Note: FORGE client doesn't currently support --insecure flag
# You'll need to add the CA certificate to your system trust store
```

**Option B: Add to system trust store**

```bash
# On Linux (Ubuntu/Debian)
sudo cp ~/.forge/tls/cert.pem /usr/local/share/ca-certificates/forge-dev-cert.crt
sudo update-ca-certificates

# On macOS
sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain ~/.forge/tls/cert.pem

# On Windows
certutil -addstore -f "ROOT" ~/.forge/tls/cert.pem
```

## Configuration Reference

### Server CLI Arguments

| Argument | Required | Description | Example |
|----------|----------|-------------|---------|
| `--server` | Yes | Enable server mode | --server |
| `--server-tls` | No | Enable TLS/WSS | --server-tls |
| `--server-tls-cert` | Yes* | Path to TLS certificate file | --server-tls-cert /path/to/cert.pem |
| `--server-tls-key` | Yes* | Path to TLS private key file | --server-tls-key /path/to/key.pem |
| `--server-bind` | No | Bind address (default: 127.0.0.1) | --server-bind 0.0.0.0 |
| `--server-port` | No | Server port (default: 8080) | --server-port 8443 |

*Required when `--server-tls` is enabled

### Client CLI Arguments

| Argument | Required | Description | Example |
|----------|----------|-------------|---------|
| `--connect` | Yes | Server URL (ws:// or wss://) | --connect wss://server:8080/ws |
| `--user` | Yes | Username for authentication | --user admin |
| `--password` | Yes | Password for authentication | --password "***" |

## Certificate Formats

FORGE expects certificates and keys in **PEM format**:

### Certificate (.pem, .crt)
```
-----BEGIN CERTIFICATE-----
MIIDXTCCAkWgAwIBAgIJAKL...
...base64-encoded certificate...
-----END CERTIFICATE-----
```

### Private Key (.pem, .key)
```
-----BEGIN PRIVATE KEY-----  <!-- gitleaks:allow — format illustration, the body below is placeholder text, not key material -->
MIIEvQIBADANBgkqhkiG9w0B...
...base64-encoded private key...
-----END PRIVATE KEY-----
```

### Converting Formats

If you have certificates in other formats:

```bash
# DER to PEM
openssl x509 -inform der -in cert.cer -out cert.pem

# PKCS#12 (.p12) to PEM
openssl pkcs12 -in cert.p12 -out cert.pem -nodes

# PKCS#7 (.p7b) to PEM
openssl pkcs7 -print_certs -in cert.p7b -out cert.pem
```

## Production Best Practices

### 1. Use Reverse Proxy (Recommended)

Instead of exposing FORGE directly, use a reverse proxy:

```nginx
# /etc/nginx/sites-available/forge

server {
    listen 443 ssl http2;
    server_name forge.example.com;

    # SSL Configuration
    ssl_certificate /etc/letsencrypt/live/forge.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/forge.example.com/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers ECDHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384;

    # WebSocket upgrade
    location /ws {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

Then run FORGE without TLS (let nginx handle it):

```bash
forge --server --server-bind 127.0.0.1 --server-port 8080
```

### 2. Firewall Configuration

```bash
# Allow WSS traffic (default port 8080)
sudo ufw allow 8080/tcp

# Or use standard HTTPS port (443) if using reverse proxy
sudo ufw allow 443/tcp
```

### 3. Certificate Monitoring

```bash
# Check certificate expiration
openssl x509 -in /path/to/cert.pem -noout -dates

# Set up expiry alert (example with cron)
# Runs daily and alerts if cert expires within 30 days
0 9 * * * [[ $(openssl x509 -in /etc/letsencrypt/live/forge.example.com/fullchain.pem -noout -checkend 2592000) -eq 0 ]] && echo "Certificate expiring soon" | mail -s "TLS Certificate Alert" admin@example.com
```

### 4. Use Strong Cipher Suites

If running FORGE with direct TLS, ensure your system's OpenSSL is configured for strong ciphers:

```bash
# Check available ciphers
openssl ciphers -v

# Test TLS configuration
openssl s_client -connect localhost:8080 -tls1_2
openssl s_client -connect localhost:8080 -tls1_3
```

## Troubleshooting

### Issue: "Failed to load certificate"

**Cause**: Certificate file is corrupted or in wrong format

**Solution**:
```bash
# Verify certificate is valid PEM
openssl x509 -in /path/to/cert.pem -text -noout

# Check file permissions
ls -la /path/to/cert.pem /path/to/key.pem
# Should be readable by FORGE process

# Fix permissions
chmod 644 /path/to/cert.pem
chmod 600 /path/to/key.pem
```

### Issue: "No private key found"

**Cause**: Key file is missing, empty, or in wrong format

**Solution**:
```bash
# Verify private key
openssl rsa -in /path/to/key.pem -check

# Ensure key matches certificate
openssl x509 -noout -modulus -in /path/to/cert.pem | openssl md5
openssl rsa -noout -modulus -in /path/to/key.pem | openssl md5
# Both outputs should match
```

### Issue: "TLS config error"

**Cause**: Incompatible TLS version or cipher suite

**Solution**:
```bash
# Update OpenSSL and system CA certificates
sudo apt-get update
sudo apt-get install openssl libssl-dev ca-certificates

# Regenerate certificate with stronger parameters
openssl req -new -x509 -key ~/.forge/tls/key.pem \
  -out ~/.forge/tls/cert.pem -days 365 \
  -sha256 \
  -subj "/CN=localhost"
```

### Issue: Client "TLS handshake failed"

**Cause**: Client doesn't trust the certificate

**Solution**:
```bash
# For self-signed certs, add to trust store (see above)

# For Let's Encrypt, ensure fullchain.pem is used (not cert.pem)
# fullchain.pem includes intermediate certificates
```

### Issue: "Failed to bind to address"

**Cause**: Port already in use or insufficient permissions

**Solution**:
```bash
# Check if port is in use
sudo lsof -i :8080
sudo netstat -tlnp | grep 8080

# If port 8080 is in use, use different port
forge --server --server-port 8443 --server-tls ...

# For ports < 1024, run with sudo or use authbind
sudo forge --server --server-port 443 --server-tls ...
```

## Testing TLS Configuration

### 1. Test Server Certificate

```bash
# Test TLS connection with OpenSSL
openssl s_client -connect localhost:8080 -showcerts

# Test with curl
curl -I --insecure https://localhost:8080/health
```

### 2. Test WebSocket Connection

```bash
# Use websocat for testing (install with: cargo install websocat)
websocat --insecure wss://localhost:8080/ws

# Send test message after connecting
{"type":"authenticate","user_id":"admin","credentials":"admin123"}
```

### 3. Verify TLS Protocols and Ciphers

```bash
# Check supported TLS versions
nmap --script ssl-enum-ciphers -p 8080 localhost

# Test specific TLS version
openssl s_client -connect localhost:8080 -tls1_2
openssl s_client -connect localhost:8080 -tls1_3
```

## Security Checklist

Before deploying FORGE with TLS to production:

- [ ] TLS enabled with `--server-tls`
- [ ] Valid certificate from trusted CA or properly configured self-signed
- [ ] Private key has restricted permissions (600 or 400)
- [ ] Certificate is not expired (check with `openssl x509 -in cert.pem -noout -dates`)
- [ ] Firewall configured to allow WSS traffic
- [ ] Reverse proxy configured (recommended)
- [ ] Auto-renewal set up for Let's Encrypt certificates
- [ ] Monitoring/alerting configured for certificate expiration
- [ ] Strong passwords used for OAuth/user authentication
- [ ] Backups configured for certificate and key files

## Additional Resources

- [Let's Encrypt Documentation](https://letsencrypt.org/docs/)
- [Mozilla SSL Configuration Generator](https://ssl-config.mozilla.org/)
- [OpenSSL Documentation](https://www.openssl.org/docs/)
- [WebSocket Protocol Specification (RFC 6455)](https://tools.ietf.org/html/rfc6455)

## Summary

- **Development**: Use self-signed certificates for quick testing
- **Production**: Use Let's Encrypt (free) or commercial CA certificates
- **Recommended**: Deploy behind nginx/Apache reverse proxy for better SSL handling
- **Critical**: Keep certificates renewed and monitor expiration dates
- **Security**: Never commit private keys to version control

For questions or issues, see the main [FORGE documentation](../README.md) or open an issue on GitHub.
