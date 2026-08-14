//! OAuth2-based authentication provider for FORGE server.
//!
//! Provides secure, production-ready authentication using OAuth2 tokens
//! from providers like GitHub, Google, etc.

use crate::ServerError;
use crate::auth::{AuthProvider, AuthResult};
use async_trait::async_trait;
use forge_core::UserRole;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// OAuth2 authentication provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    /// OAuth provider type
    pub provider: OAuthProvider,

    /// Client ID from OAuth provider
    pub client_id: String,

    /// Client secret from OAuth provider (for token validation)
    pub client_secret: Option<String>,

    /// User role mappings (GitHub username -> UserRole)
    pub user_roles: HashMap<String, UserRole>,

    /// Display name mappings (GitHub username -> display name)
    pub display_names: HashMap<String, String>,
}

impl Default for OAuthConfig {
    fn default() -> Self {
        let mut user_roles = HashMap::new();
        user_roles.insert("jedarden".to_string(), UserRole::Admin);

        let mut display_names = HashMap::new();
        display_names.insert("jedarden".to_string(), "Jedarden".to_string());

        Self {
            provider: OAuthProvider::GitHub,
            client_id: String::new(),
            client_secret: None,
            user_roles,
            display_names,
        }
    }
}

impl OAuthConfig {
    /// Validate the OAuth configuration.
    pub fn validate(&self) -> Result<(), ServerError> {
        if self.client_id.is_empty() {
            return Err(ServerError::ServerError(
                "OAuth client_id is required. Set it in your oauth.yaml config file.".to_string()
            ));
        }

        // Validate that user_roles only contains valid roles
        for (_username, role) in &self.user_roles {
            match role {
                UserRole::Admin | UserRole::Operator | UserRole::Viewer => {
                    // Valid role
                }
            }
        }

        // Check if there's at least one admin configured
        let has_admin = self.user_roles.values().any(|role| matches!(role, UserRole::Admin));
        if !has_admin {
            tracing::warn!("No Admin users configured in OAuth config. You may not have full access to all features.");
        }

        Ok(())
    }
}

/// Supported OAuth providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OAuthProvider {
    /// GitHub OAuth
    GitHub,
    /// Google OAuth
    Google,
    /// GitLab OAuth
    GitLab,
}

impl OAuthProvider {
    /// Get the authorization URL for this provider.
    pub fn auth_url(&self, client_id: &str, redirect_uri: &str, state: &str) -> String {
        match self {
            Self::GitHub => format!(
                "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&scope=read:user&state={}",
                client_id, redirect_uri, state
            ),
            Self::Google => format!(
                "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&scope=openid%20email&response_type=code&state={}",
                client_id, redirect_uri, state
            ),
            Self::GitLab => format!(
                "https://gitlab.com/oauth/authorize?client_id={}&redirect_uri={}&scope=read_user&state={}",
                client_id, redirect_uri, state
            ),
        }
    }

    /// Get the token endpoint URL for this provider.
    pub fn token_url(&self) -> &'static str {
        match self {
            Self::GitHub => "https://github.com/login/oauth/access_token",
            Self::Google => "https://oauth2.googleapis.com/token",
            Self::GitLab => "https://gitlab.com/oauth/token",
        }
    }

    /// Get the user info endpoint URL for this provider.
    pub fn user_url(&self) -> &'static str {
        match self {
            Self::GitHub => "https://api.github.com/user",
            Self::Google => "https://www.googleapis.com/oauth2/v2/userinfo",
            Self::GitLab => "https://gitlab.com/api/v4/user",
        }
    }
}

/// OAuth2 authentication provider.
pub struct OAuthAuthProvider {
    config: OAuthConfig,
    /// In-memory token cache (token -> user_id)
    token_cache: Arc<RwLock<HashMap<String, CachedUser>>>,
}

/// Cached user information for validated tokens.
#[derive(Clone)]
struct CachedUser {
    user_id: String,
    display_name: String,
    role: UserRole,
    expires_at: chrono::DateTime<chrono::Utc>,
}

impl CachedUser {
    /// Check if this cached entry has expired.
    fn is_expired(&self) -> bool {
        chrono::Utc::now() > self.expires_at
    }
}

impl OAuthAuthProvider {
    /// Create a new OAuth auth provider.
    pub fn new(config: OAuthConfig) -> Self {
        Self {
            config,
            token_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new OAuth auth provider with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(OAuthConfig::default())
    }

    /// Load OAuth configuration from a YAML file.
    pub fn from_config_file(path: &std::path::Path) -> Result<Self, ServerError> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| ServerError::ServerError(format!("Failed to read OAuth config: {}", e)))?;

        let config: OAuthConfig = serde_yaml::from_str(&contents)
            .map_err(|e| ServerError::ServerError(format!("Failed to parse OAuth config: {}", e)))?;

        // Validate configuration
        config.validate()?;

        info!("Loaded OAuth configuration for provider {:?}", config.provider);
        Ok(Self::new(config))
    }

    /// Validate an OAuth access token and get user information.
    async fn validate_token(&self, token: &str) -> Result<CachedUser, ServerError> {
        // Check cache first
        {
            let cache = self.token_cache.read().await;
            if let Some(cached) = cache.get(token) {
                if !cached.is_expired() {
                    debug!("Token cache hit for user {}", cached.user_id);
                    return Ok(cached.clone());
                }
            }
        }

        // Validate token with OAuth provider
        let user_info = self.fetch_user_info(token).await?;

        // Map OAuth user to internal user
        let user_id = user_info.login.clone();
        let display_name = self.config.display_names
            .get(&user_id)
            .cloned()
            .unwrap_or_else(|| user_info.clone().name.unwrap_or_else(|| user_id.clone()));
        let role = self.config.user_roles
            .get(&user_id)
            .copied()
            .unwrap_or(UserRole::Viewer); // Default to Viewer for unknown users

        // Cache the result (1 hour TTL)
        let cached_user = CachedUser {
            user_id: user_id.clone(),
            display_name: display_name.clone(),
            role,
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        };

        {
            let mut cache = self.token_cache.write().await;
            cache.insert(token.to_string(), cached_user.clone());
        }

        info!("User {} authenticated via OAuth", user_id);
        Ok(cached_user)
    }

    /// Fetch user information from OAuth provider using access token.
    async fn fetch_user_info(&self, token: &str) -> Result<OAuthUserInfo, ServerError> {
        let client = reqwest::Client::new();
        let url = self.config.provider.user_url();

        let response = client
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
            .header("User-Agent", "FORGE-Server/0.3.0")
            .send()
            .await
            .map_err(|e| ServerError::AuthenticationFailed(format!("HTTP request failed: {}", e)))?;

        if response.status().is_success() {
            let user_info: OAuthUserInfo = response
                .json()
                .await
                .map_err(|e| ServerError::AuthenticationFailed(format!("Failed to parse user info: {}", e)))?;
            Ok(user_info)
        } else {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            Err(ServerError::AuthenticationFailed(
                format!("Token validation failed: {} - {}", status, error_text)
            ))
        }
    }

    /// Clean up expired entries from the token cache.
    pub async fn cleanup_expired_tokens(&self) {
        let mut cache = self.token_cache.write().await;
        cache.retain(|_, cached| !cached.is_expired());
        debug!("Cleaned up expired OAuth tokens");
    }
}

impl Default for OAuthAuthProvider {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[async_trait]
impl AuthProvider for OAuthAuthProvider {
    async fn authenticate(&self, _user_id: &str, credentials: &str) -> Result<AuthResult, ServerError> {
        // For OAuth, we expect:
        // - user_id: ignored (we get user_id from the token)
        // - credentials: OAuth access token

        // Clean up expired tokens periodically
        if rand::random::<f32>() < 0.01 { // 1% chance on each auth call
            self.cleanup_expired_tokens().await;
        }

        let cached_user = self.validate_token(credentials).await?;

        Ok(AuthResult {
            user_id: cached_user.user_id,
            display_name: cached_user.display_name,
            role: cached_user.role,
        })
    }
}

/// User information from OAuth providers.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OAuthUserInfo {
    pub login: String,
    pub name: Option<String>,
    pub email: Option<String>,
}

/// GitHub-specific user info (for debugging/testing).
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubUserInfo {
    pub id: u64,
    pub login: String,
    pub name: Option<String>,
    pub email: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth_config_default() {
        let config = OAuthConfig::default();
        assert_eq!(config.provider, OAuthProvider::GitHub);
        assert!(config.user_roles.contains_key("jedarden"));
        assert_eq!(config.user_roles.get("jedarden"), Some(&UserRole::Admin));
    }

    #[test]
    fn test_github_auth_url() {
        let url = OAuthProvider::GitHub.auth_url(
            "test_client_id",
            "http://localhost:8080/callback",
            "test_state"
        );

        assert!(url.contains("github.com"));
        assert!(url.contains("client_id=test_client_id"));
        assert!(url.contains("redirect_uri=http://localhost:8080/callback"));
        assert!(url.contains("state=test_state"));
    }

    #[test]
    fn test_token_cache_expiration() {
        let cached = CachedUser {
            user_id: "test".to_string(),
            display_name: "Test".to_string(),
            role: UserRole::Viewer,
            expires_at: chrono::Utc::now() - chrono::Duration::seconds(1),
        };

        assert!(cached.is_expired());

        let cached_valid = CachedUser {
            user_id: "test".to_string(),
            display_name: "Test".to_string(),
            role: UserRole::Viewer,
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        };

        assert!(!cached_valid.is_expired());
    }
}
