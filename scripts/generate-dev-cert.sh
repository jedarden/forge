#!/usr/bin/env bash
#
# generate-dev-cert.sh - Generate self-signed TLS certificates for FORGE development
#
# This script generates self-signed X.509 certificates and private keys for
# local development and testing of FORGE server with TLS/WSS support.
#
# Usage:
#   ./scripts/generate-dev-cert.sh [options]
#
# Options:
#   -d, --domain <domain>    Domain name for certificate (default: localhost)
#   -o, --output <dir>       Output directory (default: ~/.forge/tls)
#   -v, --valid-days <days>  Certificate validity in days (default: 365)
#   -f, --force              Overwrite existing certificates
#   -h, --help               Show this help message
#
# Example:
#   ./scripts/generate-dev-cert.sh --domain forge.local --valid-days 730
#
# After generation, start FORGE with:
#   forge --server --server-tls \
#     --server-tls-cert ~/.forge/tls/cert.pem \
#     --server-tls-key ~/.forge/tls/key.pem
#
# WARNING: These are self-signed certificates for DEVELOPMENT/TESTING only.
# DO NOT use them in production environments.

set -euo pipefail

# Default values
DOMAIN="localhost"
OUTPUT_DIR=""
VALID_DAYS=365
FORCE_OVERWRITE=false

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Functions
print_header() {
    echo -e "${BLUE}╔════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║     FORGE Development TLS Certificate Generator            ║${NC}"
    echo -e "${BLUE}╚════════════════════════════════════════════════════════════╝${NC}"
    echo ""
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

print_info() {
    echo -e "${BLUE}ℹ${NC} $1"
}

show_help() {
    cat <<EOF
Usage: $(basename "$0") [options]

Generate self-signed TLS certificates for FORGE development/testing.

Options:
  -d, --domain <domain>     Domain name for certificate (default: localhost)
  -o, --output <dir>        Output directory (default: ~/.forge/tls)
  -v, --valid-days <days>   Certificate validity in days (default: 365)
  -f, --force               Overwrite existing certificates without prompting
  -h, --help                Show this help message

Examples:
  # Generate for localhost (default)
  $(basename "$0")

  # Generate for custom domain
  $(basename "$0") --domain forge.local

  # Generate with 2-year validity
  $(basename "$0") --valid-days 730

  # Generate in custom directory
  $(basename "$0") --output /tmp/forge-certs

After generation, start FORGE server with:
  forge --server --server-tls \\
    --server-tls-cert <output>/cert.pem \\
    --server-tls-key <output>/key.pem

WARNING: Self-signed certificates are for development only.
         DO NOT use in production environments.

EOF
}

check_dependencies() {
    print_info "Checking dependencies..."

    if ! command -v openssl &> /dev/null; then
        print_error "OpenSSL is not installed or not in PATH"
        echo ""
        echo "Install OpenSSL:"
        echo "  Ubuntu/Debian: sudo apt-get install openssl"
        echo "  CentOS/RHEL:   sudo yum install openssl"
        echo "  macOS:         brew install openssl"
        exit 1
    fi

    print_success "OpenSSL found: $(openssl version | head -n1 | cut -d' ' -f2)"
    echo ""
}

parse_arguments() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            -d|--domain)
                DOMAIN="$2"
                shift 2
                ;;
            -o|--output)
                OUTPUT_DIR="$2"
                shift 2
                ;;
            -v|--valid-days)
                VALID_DAYS="$2"
                shift 2
                ;;
            -f|--force)
                FORCE_OVERWRITE=true
                shift
                ;;
            -h|--help)
                show_help
                exit 0
                ;;
            *)
                print_error "Unknown option: $1"
                echo ""
                echo "Use -h or --help for usage information"
                exit 1
                ;;
        esac
    done

    # Set default output directory
    if [[ -z "$OUTPUT_DIR" ]]; then
        OUTPUT_DIR="$HOME/.forge/tls"
    fi
}

check_existing_certs() {
    local cert_file="$OUTPUT_DIR/cert.pem"
    local key_file="$OUTPUT_DIR/key.pem"

    if [[ -f "$cert_file" ]] || [[ -f "$key_file" ]]; then
        if [[ "$FORCE_OVERWRITE" == "true" ]]; then
            print_warning "Overwriting existing certificates"
            rm -f "$cert_file" "$key_file"
        else
            print_warning "Existing certificates found:"
            [[ -f "$cert_file" ]] && echo "  - Certificate: $cert_file"
            [[ -f "$key_file" ]] && echo "  - Private key: $key_file"
            echo ""
            read -p "Overwrite? (y/N): " -n 1 -r
            echo ""

            if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                print_info "Aborted"
                exit 0
            fi

            rm -f "$cert_file" "$key_file"
        fi
    fi
}

generate_private_key() {
    print_info "Generating 2048-bit RSA private key..."

    openssl genrsa -out "$OUTPUT_DIR/key.pem" 2048 2>/dev/null

    if [[ $? -eq 0 ]]; then
        chmod 600 "$OUTPUT_DIR/key.pem"
        print_success "Private key generated: $OUTPUT_DIR/key.pem"
    else
        print_error "Failed to generate private key"
        exit 1
    fi
    echo ""
}

generate_certificate() {
    print_info "Generating self-signed certificate..."

    local subject="/C=US/ST=Development/L=Development/O=FORGE/OU=Development/CN=$DOMAIN"

    # Generate certificate with proper extensions for SAN (Subject Alternative Name)
    openssl req -new -x509 \
        -key "$OUTPUT_DIR/key.pem" \
        -out "$OUTPUT_DIR/cert.pem" \
        -days "$VALID_DAYS" \
        -sha256 \
        -subj "$subject" \
        -addext "subjectAltName=DNS:$DOMAIN,DNS:localhost,DNS:*.localhost,IP:127.0.0.1" \
        2>/dev/null

    if [[ $? -eq 0 ]]; then
        chmod 644 "$OUTPUT_DIR/cert.pem"
        print_success "Certificate generated: $OUTPUT_DIR/cert.pem"
    else
        print_error "Failed to generate certificate"
        exit 1
    fi
    echo ""
}

display_certificate_info() {
    print_info "Certificate details:"
    echo ""

    openssl x509 -in "$OUTPUT_DIR/cert.pem" -noout -text | grep -A 2 "Validity"
    echo ""

    openssl x509 -in "$OUTPUT_DIR/cert.pem" -noout -text | grep -A 10 "Subject Alternative Name"
    echo ""
}

display_usage_instructions() {
    print_success "Certificate generation complete!"
    echo ""

    cat <<EOF
${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}
Files Generated:
  • Certificate: ${GREEN}$OUTPUT_DIR/cert.pem${NC}
  • Private Key:  ${GREEN}$OUTPUT_DIR/key.pem${NC}
${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}

${BLUE}Start FORGE Server with TLS:${NC}
  forge --server \\
    --server-tls \\
    --server-tls-cert $OUTPUT_DIR/cert.pem \\
    --server-tls-key $OUTPUT_DIR/key.pem \\
    --server-bind 0.0.0.0 \\
    --server-port 8080

${BLUE}Connect Client (WSS):${NC}
  forge --connect wss://$DOMAIN:8080/ws \\
    --user admin \\
    --password "***"

${BLUE}Quick Test with OpenSSL:${NC}
  openssl s_client -connect $DOMAIN:8080 -showcerts

${BLUE}Verify Certificate:${NC}
  openssl x509 -in $OUTPUT_DIR/cert.pem -text -noout

EOF

    print_warning "⚠ WARNING: Self-signed certificates are for DEVELOPMENT ONLY"
    print_warning "           DO NOT use these certificates in production!"
    echo ""
    print_info "For production certificates, see:"
    echo "  https://letsencrypt.org/ (free, automated)"
    echo "  https://docs/tls-setup.md (setup guide)"
    echo ""
}

# Main execution
main() {
    print_header

    parse_arguments "$@"
    check_dependencies
    check_existing_certs

    # Create output directory if it doesn't exist
    if [[ ! -d "$OUTPUT_DIR" ]]; then
        mkdir -p "$OUTPUT_DIR"
        print_info "Created output directory: $OUTPUT_DIR"
        echo ""
    fi

    generate_private_key
    generate_certificate
    display_certificate_info
    display_usage_instructions
}

# Run main function
main "$@"
