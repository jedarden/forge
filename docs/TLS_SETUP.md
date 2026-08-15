# TLS/WSS Setup Guide for FORGE Server

This guide explains how to configure TLS (Transport Layer Security) for FORGE server to enable secure WebSocket connections (WSS) in production deployments.

## Overview

FORGE server supports both:
- **WS (WebSocket)**: Unencrypted connections for development/testing
- **WSS (WebSocket Secure)**: TLS-encrypted connections for production

**Always use WSS with valid certificates in production environments.**

## Quick Start (Development)

### 1. Generate Self-Signed Certificate

Use FORGE's built-in certificate generation:

```bash
# Generate certificate for localhost (valid for 365 days)
forge generate-cert localhost

# Generate for custom domain with custom validity
forge generate-cert forge.example.com --days 90

# Certificates are saved to ~/.forge/tls/
# - ~/.forge/tls/cert.pem
# - ~/.forge/tls/key.pem
```

**Note**: Browsers will warn about self-signed certificates. This is expected for development.

### 2. Start Server with TLS

```bash
forge --server \
  --server-tls \
  --server-tls-cert ~/.forge/tls/cert.pem \
  --server-tls-key ~/.forge/tls/key.pem \
  --server-bind 127.0.0.1 \
  --server-port 8080
```

### 3. Connect with WSS Client

```bash
forge --connect wss://localhost:8080/ws --user admin --password "***"
```

### Alternative: Manual Certificate Generation

If you prefer using OpenSSL directly:

```bash
# Create directory for certificates
mkdir -p ~/.forge/tls

# Generate private key (RSA 2048-bit)
openssl genrsa -out ~/.forge/tls/key.pem 2048

# Generate self-signed certificate (valid for 365 days)
openssl req -new -x509 -key ~/.forge/tls/key.pem \
  -out ~/.forge/tls/cert.pem -days 365 \
  -subj "/C=US/ST=State/L=City/O=Organization/CN=localhost"

# Set secure permissions
chmod 600 ~/.forge/tls/key.pem
chmod 644 ~/.forge/tls/cert.pem

# Verify certificate
openssl x509 -in ~/.forge/tls/cert.pem -text -noout
```

## Production Deployment

### Option 1: Let's Encrypt (Recommended)

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

# Set proper permissions
sudo chmod 755 /etc/letsencrypt/live
sudo chmod 755 /etc/letsencrypt/live/forge.example.com
sudo chmod 644 /etc/letsencrypt/live/forge.example.com/fullchain.pem
sudo chmod 600 /etc/letsencrypt/live/forge.example.com/privkey.pem

# Start FORGE with Let's Encrypt certificates
forge --server \
  --server-tls \
  --server-tls-cert /etc/letsencrypt/live/forge.example.com/fullchain.pem \
  --server-tls-key /etc/letsencrypt/live/forge.example.com/privkey.pem \
  --server-bind 0.0.0.0 \
  --server-port 8080
```

#### Certificate Renewal Automation

Let's Encrypt certificates expire after 90 days. Set up auto-renewal:

```bash
# Test renewal (dry-run)
sudo certbot renew --dry-run

# Certbot creates a systemd timer for auto-renewal
# Verify it's active:
systemctl list-timers | grep certbot

# Create post-renewal hook to restart FORGE
sudo tee /etc/letsencrypt/renewal-hooks/post/restart-forge <<'EOF'
#!/bin/bash
# Restart FORGE after certificate renewal
systemctl restart forge
EOF

sudo chmod +x /etc/letsencrypt/renewal-hooks/post/restart-forge

# Test the hook
sudo certbot renew --post-hook "echo 'Hook executed'"
```

### Option 2: Commercial CA Certificate

**Pros**: Validated identity, warranty, support
**Cons**: Costs money, annual renewal

Purchase from providers like:
- **DigiCert** - https://www.digicert.com/
- **GlobalSign** - https://www.globalsign.com/
- **SSL.com** - https://www.ssl.com/
- **GoDaddy** - https://www.godaddy.com/

After purchase, you'll receive:
- Certificate file (usually .pem or .crt format)
- Private key file
- Intermediate certificate chain (if not included)

**Combining certificate with chain:**

```bash
# If you received separate files, combine them:
cat your_cert.pem intermediate_ca.pem > fullchain.pem

# Use fullchain.pem for --server-tls-cert
forge --server \
  --server-tls \
  --server-tls-cert /path/to/fullchain.pem \
  --server-tls-key /path/to/private_key.pem
```

### File Permissions (Critical)

Private keys must have restricted permissions:

```bash
# Recommended permissions for certificate files
chmod 644 /path/to/cert.pem        # Readable by all
chmod 600 /path/to/key.pem         # Read/write by owner only

# For system directories
chown root:root /etc/letsencrypt/live/*/privkey.pem
chmod 600 /etc/letsencrypt/live/*/privkey.pem

# Verify permissions
ls -la /path/to/key.pem
# Should show: -rw------- (600)
```

**Never commit private keys to git!** Add to `.gitignore`:

```bash
echo "*.pem" >> .gitignore
echo "*.key" >> .gitignore
echo "!scripts/generate-dev-cert.sh" >> .gitignore
```

### Certificate Chain Files

When using certificate authorities, you often need a certificate chain:

```bash
# Let's Encrypt provides fullchain.pem (includes intermediates)
# This is the recommended file to use

# If your CA provides separate files:
cat server.crt intermediate.crt > fullchain.pem

# Verify the chain
openssl s_client -connect forge.example.com:8080 -showcerts

# Check certificate order
openssl crl2pkcs7 -nocrl -certfile fullchain.pem | openssl pkcs7 -print_certs -text
```

### Recommended TLS Settings

FORGE uses rustls which enforces secure defaults:

- **Minimum TLS Version**: TLS 1.2
- **Maximum TLS Version**: TLS 1.3
- **Cipher Suites**: Modern, secure ciphers only
- **No Client Authentication**: By default (no client certificates required)

**Verify TLS configuration:**

```bash
# Check supported TLS versions
nmap --script ssl-enum-ciphers -p 8080 forge.example.com

# Test specific TLS version
openssl s_client -connect forge.example.com:8080 -tls1_2
openssl s_client -connect forge.example.com:8080 -tls1_3

# Check certificate details
openssl x509 -in /path/to/cert.pem -text -noout
```

## Reverse Proxy Setup

### Option 1: Nginx (Recommended)

Terminate TLS at nginx and proxy to FORGE:

```nginx
# /etc/nginx/sites-available/forge

server {
    listen 443 ssl http2;
    server_name forge.example.com;

    # SSL Configuration
    ssl_certificate /etc/letsencrypt/live/forge.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/forge.example.com/privkey.pem;
    
    # Modern SSL configuration
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305:DHE-RSA-AES128-GCM-SHA256:DHE-RSA-AES256-GCM-SHA384;
    ssl_prefer_server_ciphers off;

    # SSL session configuration
    ssl_session_timeout 1d;
    ssl_session_cache shared:SSL:50m;
    ssl_session_tickets off;

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

        # WebSocket timeout settings
        proxy_read_timeout 86400;
        proxy_send_timeout 86400;
    }

    # Optional: Health check endpoint
    location /health {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
    }
}

# HTTP to HTTPS redirect
server {
    listen 80;
    server_name forge.example.com;
    return 301 https://$server_name$request_uri;
}
```

**Enable the configuration:**

```bash
# Create symbolic link
sudo ln -s /etc/nginx/sites-available/forge /etc/nginx/sites-enabled/

# Test nginx configuration
sudo nginx -t

# Reload nginx
sudo systemctl reload nginx

# Start FORGE without TLS (nginx handles it)
forge --server --server-bind 127.0.0.1 --server-port 8080
```

**Advantages of nginx reverse proxy:**
- Automatic TLS certificate renewal via certbot
- Better SSL/TLS configuration options
- HTTP to HTTPS redirect
- Static file serving (if needed)
- Rate limiting and DDoS protection
- Better logging and monitoring

### Option 2: Traefik (Modern Alternative)

Traefik automatically handles TLS with Let's Encrypt:

```yaml
# /etc/traefik/traefik.yml or traefik.toml

entryPoints:
  web:
    address: ":80"
    http:
      redirections:
        entryPoint:
          to: websecure
          scheme: https
  websecure:
    address: ":443"

certificatesResolvers:
  letsencrypt:
    acme:
      email: your-email@example.com
      storage: /etc/traefik/acme.json
      httpChallenge:
        entryPoint: web

providers:
  file:
    filename: /etc/traefik/dynamic.yml

# Dynamic configuration
http:
  routers:
    forge:
      rule: "Host(`forge.example.com`)"
      entryPoints:
        - websecure
      service: forge
      tls:
        certResolver: letsencrypt

  services:
    forge:
      loadBalancer:
        servers:
          - url: "http://127.0.0.1:8080"
```

**Start FORGE with Traefik:**

```bash
# Start Traefik
docker run -d \
  --name traefik \
  -p 80:80 \
  -p 443:443 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v $PWD/traefik.yml:/etc/traefik/traefik.yml:ro \
  -v $PWD/acme.json:/etc/traefik/acme.json \
  traefik:v2.10

# Or run Traefik natively
traefik --configfile=/etc/traefik/traefik.yml

# Start FORGE without TLS (Traefik handles it)
forge --server --server-bind 127.0.0.1 --server-port 8080
```

**Advantages of Traefik:**
- Automatic Let's Encrypt certificate management
- Dynamic configuration without restart
- Built-in metrics and dashboard
- Service discovery integration
- Automatic HTTPS redirect

### When to Use Reverse Proxy vs Direct TLS

**Use Reverse Proxy (nginx/Traefik) when:**
- Hosting on public internet
- Multiple services on same host
- Need advanced SSL/TLS configuration
- Want automatic certificate management
- Need rate limiting or DDoS protection
- Hosting other web services alongside FORGE

**Use Direct TLS when:**
- Internal network only (VPN, Tailscale)
- Simple single-service deployment
- Want to minimize moving parts
- Already have certificate management solution
- Development and testing

## Troubleshooting

### Common TLS Errors and Solutions

#### Error: "Failed to load certificate"

**Cause**: Certificate file is corrupted, in wrong format, or missing

**Solution:**
```bash
# Verify certificate is valid PEM format
openssl x509 -in /path/to/cert.pem -text -noout

# Check file exists and is readable
ls -la /path/to/cert.pem

# Check file permissions
stat /path/to/cert.pem

# Re-download or regenerate certificate
sudo certbot certonly --standalone -d forge.example.com --force-renewal
```

#### Error: "No private key found"

**Cause**: Key file is missing, empty, or in wrong format

**Solution:**
```bash
# Verify private key is valid
openssl rsa -in /path/to/key.pem -check

# Ensure key matches certificate
openssl x509 -noout -modulus -in /path/to/cert.pem | openssl md5
openssl rsa -noout -modulus -in /path/to/key.pem | openssl md5
# Both outputs should match

# Check file permissions
ls -la /path/to/key.pem
# Should be -rw------- (600)

# Fix permissions
chmod 600 /path/to/key.pem
```

#### Error: "TLS config error"

**Cause**: Incompatible TLS version, expired certificate, or domain mismatch

**Solution:**
```bash
# Check certificate expiry
openssl x509 -in /path/to/cert.pem -noout -dates

# Check certificate subject
openssl x509 -in /path/to/cert.pem -noout -subject

# Check if certificate is expired
openssl x509 -in /path/to/cert.pem -noout -checkend 0
# Returns 0 if valid, 1 if expired

# Check domain in certificate
openssl x509 -in /path/to/cert.pem -noout -text | grep -A1 "Subject:"

# Renew certificate if expired
sudo certbot renew
```

#### Error: "Client TLS handshake failed"

**Cause**: Client doesn't trust the certificate (self-signed) or hostname mismatch

**Solution:**
```bash
# For self-signed certs, add to client trust store:
# On Linux (Ubuntu/Debian)
sudo cp /path/to/cert.pem /usr/local/share/ca-certificates/forge-dev-cert.crt
sudo update-ca-certificates

# On macOS
sudo security add-trusted-cert -d -r trustRoot \
  -k /Library/Keychains/System.keychain /path/to/cert.pem

# On Windows
certutil -addstore -f "ROOT" /path/to/cert.pem

# Verify hostname matches certificate
openssl s_client -connect forge.example.com:8080 -servername forge.example.com
```

#### Error: "Failed to bind to address"

**Cause**: Port already in use or insufficient permissions

**Solution:**
```bash
# Check if port is in use
sudo lsof -i :8080
sudo netstat -tlnp | grep 8080
ss -tlnp | grep 8080

# Kill process using the port
sudo kill -9 <PID>

# Or use different port
forge --server --server-port 8443 --server-tls ...

# For ports < 1024, use authbind or sudo
sudo authbind --deep forge --server --server-port 443 --server-tls ...
```

### How to Check Certificate Expiry

```bash
# Check certificate expiration date
openssl x509 -in /path/to/cert.pem -noout -dates

# Check if certificate expires within 30 days
openssl x509 -in /path/to/cert.pem -noout -checkend 2592000
# Exit code 0 = valid for >30 days
# Exit code 1 = expires within 30 days

# FORGE automatically warns on startup if cert expires within 30 days

# Set up expiry monitoring with cron
# Add to crontab: 0 9 * * * /usr/local/bin/check-cert-expiry.sh
```

**Create `/usr/local/bin/check-cert-expiry.sh`:**

```bash
#!/bin/bash
# Check certificate expiry and send alert if expiring soon

CERT_FILE="/etc/letsencrypt/live/forge.example.com/fullchain.pem"
DAYS_WARNING=30

if openssl x509 -in "$CERT_FILE" -noout -checkend $((DAYS_WARNING * 86400)); then
    # Certificate is valid for more than DAYS_WARNING days
    exit 0
else
    # Certificate expires within DAYS_WARNING days
    EXPIRY_DATE=$(openssl x509 -in "$CERT_FILE" -noout -enddate | cut -d= -f2)
    echo "WARNING: Certificate expires on $EXPIRY_DATE" | \
      mail -s "FORGE TLS Certificate Expiring Soon" admin@example.com
    exit 1
fi
```

### How to Test WSS Connection

```bash
# Install websocat (WebSocket testing tool)
cargo install websocat

# Test WSS connection (ignore self-signed cert warnings)
websocat --insecure wss://localhost:8080/ws

# Test with authentication
websocat --insecure wss://localhost:8080/ws \
  --header "Authorization: Bearer YOUR_TOKEN"

# Send test message after connecting
{"type":"authenticate","user_id":"admin","credentials":"admin123"}

# Test with OpenSSL
openssl s_client -connect localhost:8080 -showcerts

# Test TLS protocol versions
openssl s_client -connect localhost:8080 -tls1_2
openssl s_client -connect localhost:8080 -tls1_3

# Test with curl (HTTP endpoint)
curl -I --insecure https://localhost:8080/health
```

### Browser Console Debugging

**Open browser DevTools (F12) and check:**

**Console Tab:**
```javascript
// Check WebSocket connection status
// Look for errors like:
// - "WebSocket connection to 'wss://...' failed"
// - "CERT_COMMON_NAME_INVALID"
// - "ERR_CERT_AUTHORITY_INVALID"

// Test connection manually
const ws = new WebSocket('wss://forge.example.com:8080/ws');
ws.onopen = () => console.log('WebSocket connected');
ws.onerror = (error) => console.error('WebSocket error:', error);
ws.onclose = (event) => console.log('WebSocket closed:', event);
```

**Network Tab:**
1. Filter by "WS" (WebSocket)
2. Find the WSS connection
3. Check:
   - Status code: 101 (WebSocket Protocol Handshake)
   - Response headers: `Upgrade: websocket`
   - TLS version: TLS 1.2 or TLS 1.3

**Security Tab:**
- Check certificate details
- Verify certificate chain
- Check for mixed content warnings

### FORGE-Specific Validation

FORGE includes built-in TLS validation that runs on startup:

```bash
# FORGE validates:
# - Certificate file exists and is readable
# - Private key file exists and is readable
# - PEM format is correct for both files
# - Certificate is not expired
# - Domain matches certificate CN or SANs

# Run validation manually with FORGE
forge validate --verbose

# FORGE will warn if:
# - Certificate expires within 30 days
# - Domain doesn't match certificate
# - Certificate is expired or invalid
```

## Security Considerations

### Never Commit Private Keys to Git

```bash
# Add to .gitignore
echo "*.pem" >> .gitignore
echo "*.key" >> .gitignore
echo "!scripts/generate-dev-cert.sh" >> .gitignore
echo ".forge/tls/" >> .gitignore

# Check if keys were accidentally committed
git log --all --full-history -- "*.pem" "*.key"

# Remove committed keys (DANGER: rewrites history)
git filter-branch --force --index-filter \
  "git rm --cached --ignore-unmatch *.pem *.key" \
  --prune-empty --tag-name-filter cat -- --all
```

### Use Strong Certificate Signing

For production certificates:
- **RSA**: Use 2048-bit or 4096-bit keys
- **ECDSA**: Use P-256 or P-384 curves
- **Hash**: SHA-256 or higher (no MD5 or SHA-1)

```bash
# Generate strong RSA key (4096-bit)
openssl genrsa -out key.pem 4096

# Generate ECDSA key (P-256)
openssl ecparam -genkey -name prime256v1 -out key.pem

# Generate with SHA-256
openssl req -new -x509 -key key.pem -out cert.pem -days 365 -sha256
```

### Regular Certificate Renewal

```bash
# Check certbot auto-renewal is enabled
systemctl status certbot.timer
systemctl list-timers | grep certbot

# Test renewal process
sudo certbot renew --dry-run

# Check last renewal
sudo cat /var/log/letsencrypt/letsencrypt.log | grep "Renewing"

# Manual renewal if needed
sudo certbot renew --force-renewal
```

### Monitor Certificate Expiry

```bash
# Add to crontab for daily checks
crontab -e

# Check certificate daily at 9 AM
0 9 * * * /usr/local/bin/check-cert-expiry.sh

# Check weekly on Monday
0 9 * * 1 /usr/local/bin/check-cert-expiry.sh

# Use monitoring tools (Prometheus, Nagios, etc.)
# Export certificate expiry as metric
```

### Secure Key Storage

```bash
# Store keys in secure directory
sudo mkdir -p /etc/forge/tls
sudo chmod 700 /etc/forge/tls

# Copy certificates with secure permissions
sudo cp cert.pem /etc/forge/tls/
sudo cp key.pem /etc/forge/tls/
sudo chmod 644 /etc/forge/tls/cert.pem
sudo chmod 600 /etc/forge/tls/key.pem

# Set ownership
sudo chown root:forge /etc/forge/tls/*
sudo chown root:root /etc/forge/tls/key.pem

# Verify setup
ls -la /etc/forge/tls/
```

### Network Security

```bash
# Configure firewall to allow WSS traffic only
sudo ufw allow 8080/tcp   # FORGE WSS port
sudo ufw allow 443/tcp    # HTTPS if using reverse proxy
sudo ufw enable

# Restrict to specific IP addresses (if needed)
sudo ufw allow from 192.168.1.0/24 to any port 8080

# Use fail2ban to block repeated failed authentication
sudo apt-get install fail2ban
```

### OAuth2 Integration

For production deployments, use OAuth2 instead of SimpleAuth:

```bash
# Configure OAuth in ~/.forge/oauth.yaml
provider: GitHub
client_id: "your_github_oauth_client_id"
client_secret: "your_github_oauth_client_secret"

user_roles:
  "github_username": "Admin"
  "team_member": "Operator"
```

## Testing Your TLS Setup

### Complete TLS Test Suite

```bash
#!/bin/bash
# test-tls-setup.sh - Comprehensive TLS testing

echo "🔍 Testing FORGE TLS Configuration..."

# 1. Check certificate files exist
if [ ! -f ~/.forge/tls/cert.pem ]; then
    echo "❌ Certificate not found: ~/.forge/tls/cert.pem"
    exit 1
fi

if [ ! -f ~/.forge/tls/key.pem ]; then
    echo "❌ Private key not found: ~/.forge/tls/key.pem"
    exit 1
fi

echo "✅ Certificate files exist"

# 2. Check certificate validity
if openssl x509 -in ~/.forge/tls/cert.pem -noout -checkend 0 2>/dev/null; then
    echo "✅ Certificate is valid"
else
    echo "❌ Certificate is expired"
    exit 1
fi

# 3. Check certificate expiry warning
if openssl x509 -in ~/.forge/tls/cert.pem -noout -checkend 2592000 2>/dev/null; then
    echo "✅ Certificate valid for >30 days"
else
    echo "⚠️  WARNING: Certificate expires within 30 days"
fi

# 4. Check private key
if openssl rsa -in ~/.forge/tls/key.pem -check >/dev/null 2>&1; then
    echo "✅ Private key is valid"
else
    echo "❌ Private key is invalid"
    exit 1
fi

# 5. Check key matches certificate
CERT_MD5=$(openssl x509 -noout -modulus -in ~/.forge/tls/cert.pem | openssl md5)
KEY_MD5=$(openssl rsa -noout -modulus -in ~/.forge/tls/key.pem | openssl md5)

if [ "$CERT_MD5" = "$KEY_MD5" ]; then
    echo "✅ Certificate and key match"
else
    echo "❌ Certificate and key don't match"
    exit 1
fi

# 6. Test TLS connection
if timeout 5 bash -c "echo | openssl s_client -connect localhost:8080 2>/dev/null" >/dev/null; then
    echo "✅ TLS connection successful"
else
    echo "⚠️  Could not test TLS connection (server may not be running)"
fi

# 7. Check file permissions
CERT_PERMS=$(stat -c %a ~/.forge/tls/cert.pem)
KEY_PERMS=$(stat -c %a ~/.forge/tls/key.pem)

if [ "$CERT_PERMS" = "644" ] || [ "$CERT_PERMS" = "600" ]; then
    echo "✅ Certificate permissions: $CERT_PERMS"
else
    echo "⚠️  WARNING: Certificate permissions: $CERT_PERMS (should be 644 or 600)"
fi

if [ "$KEY_PERMS" = "600" ]; then
    echo "✅ Private key permissions: 600"
else
    echo "⚠️  WARNING: Private key permissions: $KEY_PERMS (should be 600)"
fi

echo ""
echo "🎉 TLS configuration test complete!"
```

## Production Deployment Checklist

Before deploying FORGE with TLS to production, verify:

- [ ] **TLS Enabled**: `--server-tls` flag is set
- [ ] **Valid Certificate**: Certificate from trusted CA or properly configured self-signed
- [ ] **Key Security**: Private key has 600 permissions and is not in git
- [ ] **Certificate Validity**: Certificate is not expired and has >30 days remaining
- [ ] **Domain Match**: Certificate CN or SANs match the server domain
- [ ] **Firewall Configured**: Firewall allows WSS traffic (port 8080 or 443)
- [ ] **Reverse Proxy**: nginx/Traefik configured (recommended)
- [ ] **Auto-Renewal**: Let's Encrypt certbot timer is active
- [ ] **Monitoring**: Certificate expiry alerts configured
- [ ] **Authentication**: OAuth2 configured (not SimpleAuth)
- [ ] **Network Security**: Firewall rules, fail2ban if needed
- [ ] **Backups**: Certificate and key files backed up securely
- [ ] **Testing**: Complete TLS test suite passes
- [ ] **Documentation**: Team knows how to renew and troubleshoot

## Additional Resources

- [Let's Encrypt Documentation](https://letsencrypt.org/docs/)
- [Mozilla SSL Configuration Generator](https://ssl-config.mozilla.org/)
- [OpenSSL Documentation](https://www.openssl.org/docs/)
- [WebSocket Protocol Specification (RFC 6455)](https://tools.ietf.org/html/rfc6455)
- [NGINX WebSocket Proxying](https://nginx.org/en/docs/http/websocket.html)
- [Traefik Documentation](https://doc.traefik.io/traefik/)

## Summary

- **Development**: Use `forge generate-cert localhost` for quick testing
- **Production**: Use Let's Encrypt (free) or commercial CA certificates
- **Recommended**: Deploy behind nginx/Traefik reverse proxy
- **Critical**: Keep certificates renewed and monitor expiration
- **Security**: Never commit private keys, use OAuth2 for authentication

For questions or issues, see the main [FORGE documentation](../README.md) or open an issue on GitHub.
