use anyhow::Result;
use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use bcrypt::{hash, verify, DEFAULT_COST};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::{debug, error, info, warn};

use crate::models::{ApiKey, Scope};
use crate::Database;

/// Authentication errors
#[derive(Error, Debug)]
pub enum AuthError {
    #[error("Invalid API key")]
    InvalidApiKey,
    #[error("API key expired")]
    ExpiredApiKey,
    #[error("Insufficient scope: required {required}, has {available:?}")]
    InsufficientScope { required: String, available: Vec<String> },
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    #[error("Invalid JWT token")]
    InvalidJwt,
    #[error("Tenant suspended")]
    TenantSuspended,
    #[error("Missing authorization header")]
    MissingAuth,
    #[error("Database error: {0}")]
    Database(#[from] anyhow::Error),
    #[error("Bcrypt error: {0}")]
    Bcrypt(#[from] bcrypt::BcryptError),
    #[error("JWT error: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),
}

/// JWT Claims structure
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,      // Subject (user ID)
    pub tenant_id: String,
    pub project_id: String,
    pub scopes: Vec<String>,
    pub exp: i64,         // Expiration time
    pub iat: i64,         // Issued at
    pub iss: String,      // Issuer
}

/// Authentication context extracted from requests
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub tenant_id: String,
    pub project_id: String,
    pub scopes: Vec<Scope>,
    pub rate_limit_per_sec: i32,
    pub auth_type: AuthType,
}

/// Type of authentication used
#[derive(Debug, Clone)]
pub enum AuthType {
    ApiKey { key_id: String },
    Jwt { user_id: String },
}

/// Rate limiting tracker
#[derive(Debug, Clone)]
struct RateLimitEntry {
    count: u32,
    window_start: DateTime<Utc>,
}

/// Authentication service
#[derive(Debug, Clone)]
pub struct AuthService {
    database: Database,
    jwt_secret: String,
    rate_limits: Arc<Mutex<HashMap<String, RateLimitEntry>>>,
}

impl AuthService {
    /// Create a new authentication service
    pub fn new(database: Database, jwt_secret: String) -> Self {
        Self {
            database,
            jwt_secret,
            rate_limits: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Generate a secure API key
    pub fn generate_api_key() -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        const KEY_LENGTH: usize = 64;
        
        let mut rng = rand::thread_rng();
        let key: String = (0..KEY_LENGTH)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect();
        
        format!("rtp_{}", key) // Prefix for realtime platform
    }

    /// Hash an API key for database lookup (using SHA-256)
    pub fn hash_api_key_for_lookup(key: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Hash an API key for secure storage (using bcrypt)
    pub fn hash_api_key_for_storage(key: &str) -> Result<String, AuthError> {
        let hash = hash(key, DEFAULT_COST)?;
        Ok(hash)
    }

    /// Verify an API key against its bcrypt hash
    pub fn verify_api_key(key: &str, bcrypt_hash: &str) -> Result<bool, AuthError> {
        let is_valid = verify(key, bcrypt_hash)?;
        Ok(is_valid)
    }

    /// Create a new API key in the database
    pub async fn create_api_key(
        &self,
        tenant_id: String,
        project_id: String,
        scopes: Vec<Scope>,
        rate_limit_per_sec: i32,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<(String, ApiKey), AuthError> {
        let raw_key = Self::generate_api_key();
        // Use SHA-256 hash for database lookup
        let lookup_hash = Self::hash_api_key_for_lookup(&raw_key);
        
        let api_key = ApiKey::new(
            tenant_id,
            project_id,
            lookup_hash,
            scopes,
            rate_limit_per_sec,
        );
        
        // Set expiration if provided
        let mut api_key = api_key;
        api_key.expires_at = expires_at;
        
        self.database.create_api_key(&api_key).await?;
        
        info!("Created API key {} for tenant {}", api_key.id, api_key.tenant_id);
        Ok((raw_key, api_key))
    }

    /// Validate an API key and return authentication context
    pub async fn validate_api_key(&self, key: &str) -> Result<AuthContext, AuthError> {
        // Use SHA-256 hash for database lookup
        let lookup_hash = Self::hash_api_key_for_lookup(key);
        
        // Get API key from database by lookup hash
        let api_key = self.database.get_api_key_by_hash(&lookup_hash).await?
            .ok_or(AuthError::InvalidApiKey)?;
        
        // Verify the key is still valid
        if !api_key.is_valid() {
            if !api_key.is_active {
                return Err(AuthError::InvalidApiKey);
            } else {
                return Err(AuthError::ExpiredApiKey);
            }
        }
        
        // Check if tenant is active
        let tenant = self.database.get_tenant(&api_key.tenant_id).await?
            .ok_or(AuthError::InvalidApiKey)?;
        
        if !tenant.is_active() {
            return Err(AuthError::TenantSuspended);
        }
        
        // Check rate limits
        self.check_rate_limit(&api_key.id, api_key.rate_limit_per_sec as u32).await?;
        
        Ok(AuthContext {
            tenant_id: api_key.tenant_id,
            project_id: api_key.project_id,
            scopes: api_key.scopes,
            rate_limit_per_sec: api_key.rate_limit_per_sec,
            auth_type: AuthType::ApiKey { key_id: api_key.id },
        })
    }

    /// Generate a JWT token
    pub fn generate_jwt(
        &self,
        user_id: String,
        tenant_id: String,
        project_id: String,
        scopes: Vec<Scope>,
        expires_in_hours: i64,
    ) -> Result<String, AuthError> {
        let now = Utc::now();
        let exp = now + Duration::hours(expires_in_hours);
        
        let claims = Claims {
            sub: user_id,
            tenant_id,
            project_id,
            scopes: scopes.iter().map(|s| format!("{:?}", s)).collect(),
            exp: exp.timestamp(),
            iat: now.timestamp(),
            iss: "realtime-platform".to_string(),
        };
        
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_ref()),
        )?;
        
        Ok(token)
    }

    /// Validate a JWT token and return authentication context
    pub async fn validate_jwt(&self, token: &str) -> Result<AuthContext, AuthError> {
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_ref()),
            &Validation::default(),
        )?;
        
        let claims = token_data.claims;
        
        // Check if token is expired
        let now = Utc::now().timestamp();
        if claims.exp < now {
            return Err(AuthError::InvalidJwt);
        }
        
        // Check if tenant is active
        let tenant = self.database.get_tenant(&claims.tenant_id).await?
            .ok_or(AuthError::InvalidJwt)?;
        
        if !tenant.is_active() {
            return Err(AuthError::TenantSuspended);
        }
        
        // Convert scope strings back to Scope enum
        let scopes: Vec<Scope> = claims.scopes.iter()
            .filter_map(|s| match s.as_str() {
                "EventsPublish" => Some(Scope::EventsPublish),
                "EventsSubscribe" => Some(Scope::EventsSubscribe),
                "AdminRead" => Some(Scope::AdminRead),
                "AdminWrite" => Some(Scope::AdminWrite),
                "BillingRead" => Some(Scope::BillingRead),
                _ => None,
            })
            .collect();
        
        Ok(AuthContext {
            tenant_id: claims.tenant_id,
            project_id: claims.project_id,
            scopes,
            rate_limit_per_sec: 1000, // Default rate limit for JWT tokens
            auth_type: AuthType::Jwt { user_id: claims.sub },
        })
    }

    /// Check if authentication context has required scope
    pub fn check_scope(&self, auth: &AuthContext, required_scope: &Scope) -> Result<(), AuthError> {
        if auth.scopes.contains(required_scope) {
            Ok(())
        } else {
            Err(AuthError::InsufficientScope {
                required: format!("{:?}", required_scope),
                available: auth.scopes.iter().map(|s| format!("{:?}", s)).collect(),
            })
        }
    }

    /// Check rate limits for a given identifier
    async fn check_rate_limit(&self, identifier: &str, limit_per_sec: u32) -> Result<(), AuthError> {
        let now = Utc::now();
        let mut rate_limits = self.rate_limits.lock().unwrap();
        
        let entry = rate_limits.entry(identifier.to_string()).or_insert(RateLimitEntry {
            count: 0,
            window_start: now,
        });
        
        // Reset window if more than 1 second has passed
        if now.signed_duration_since(entry.window_start).num_seconds() >= 1 {
            entry.count = 0;
            entry.window_start = now;
        }
        
        // Check if limit is exceeded
        if entry.count >= limit_per_sec {
            warn!("Rate limit exceeded for {}: {} requests/sec", identifier, entry.count);
            return Err(AuthError::RateLimitExceeded);
        }
        
        // Increment counter
        entry.count += 1;
        
        debug!("Rate limit check passed for {}: {}/{} requests", identifier, entry.count, limit_per_sec);
        Ok(())
    }

    /// Revoke an API key
    pub async fn revoke_api_key(&self, tenant_id: &str, key_id: &str) -> Result<(), AuthError> {
        self.database.revoke_api_key(tenant_id, key_id).await?;
        info!("Revoked API key {} for tenant {}", key_id, tenant_id);
        Ok(())
    }

    /// Clean up expired rate limit entries (should be called periodically)
    pub fn cleanup_rate_limits(&self) {
        let now = Utc::now();
        let mut rate_limits = self.rate_limits.lock().unwrap();
        
        rate_limits.retain(|_, entry| {
            now.signed_duration_since(entry.window_start).num_seconds() < 60
        });
    }
}

/// Extract authentication from request headers
pub fn extract_auth_header(headers: &HeaderMap) -> Result<String, AuthError> {
    let auth_header = headers
        .get("authorization")
        .ok_or(AuthError::MissingAuth)?
        .to_str()
        .map_err(|_| AuthError::MissingAuth)?;
    
    if let Some(token) = auth_header.strip_prefix("Bearer ") {
        Ok(token.to_string())
    } else if let Some(key) = auth_header.strip_prefix("ApiKey ") {
        Ok(key.to_string())
    } else {
        Err(AuthError::MissingAuth)
    }
}

/// Middleware for API key authentication
pub async fn api_key_auth_middleware(
    State(auth_service): State<AuthService>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let headers = request.headers();
    
    match extract_auth_header(headers) {
        Ok(auth_value) => {
            // Try to validate as API key first
            match auth_service.validate_api_key(&auth_value).await {
                Ok(auth_context) => {
                    // Insert auth context into request extensions
                    request.extensions_mut().insert(auth_context);
                    Ok(next.run(request).await)
                }
                Err(AuthError::InvalidApiKey) => {
                    // Try JWT validation as fallback
                    match auth_service.validate_jwt(&auth_value).await {
                        Ok(auth_context) => {
                            request.extensions_mut().insert(auth_context);
                            Ok(next.run(request).await)
                        }
                        Err(e) => {
                            error!("Authentication failed: {}", e);
                            Err(StatusCode::UNAUTHORIZED)
                        }
                    }
                }
                Err(AuthError::RateLimitExceeded) => {
                    warn!("Rate limit exceeded");
                    Err(StatusCode::TOO_MANY_REQUESTS)
                }
                Err(AuthError::TenantSuspended) => {
                    warn!("Tenant suspended");
                    Err(StatusCode::FORBIDDEN)
                }
                Err(e) => {
                    error!("Authentication error: {}", e);
                    Err(StatusCode::UNAUTHORIZED)
                }
            }
        }
        Err(_) => {
            error!("Missing or invalid authorization header");
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

/// Middleware for scope-based authorization
pub fn require_scope(required_scope: Scope) -> impl Clone + Fn(AuthContext, Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, StatusCode>> + Send>> {
    move |auth_context: AuthContext, request: Request, next: Next| {
        let scope = required_scope.clone();
        Box::pin(async move {
            if auth_context.scopes.contains(&scope) {
                Ok(next.run(request).await)
            } else {
                warn!(
                    "Insufficient scope: required {:?}, has {:?}",
                    scope, auth_context.scopes
                );
                Err(StatusCode::FORBIDDEN)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{BillingPlan, ProjectLimits};

    #[test]
    fn test_generate_api_key() {
        let key = AuthService::generate_api_key();
        assert!(key.starts_with("rtp_"));
        assert_eq!(key.len(), 68); // "rtp_" + 64 characters
    }

    #[test]
    fn test_hash_and_verify_api_key() {
        let key = "test_api_key_12345";
        
        // Test lookup hash
        let lookup_hash = AuthService::hash_api_key_for_lookup(key);
        assert_eq!(lookup_hash.len(), 64); // SHA-256 produces 64 character hex string
        
        // Test storage hash
        let storage_hash = AuthService::hash_api_key_for_storage(key).unwrap();
        assert!(AuthService::verify_api_key(key, &storage_hash).unwrap());
        assert!(!AuthService::verify_api_key("wrong_key", &storage_hash).unwrap());
    }

    #[test]
    fn test_scope_checking_logic() {
        let scopes = vec![Scope::EventsPublish, Scope::EventsSubscribe];
        
        // Test scope checking logic without needing AuthService instance
        assert!(scopes.contains(&Scope::EventsPublish));
        assert!(scopes.contains(&Scope::EventsSubscribe));
        assert!(!scopes.contains(&Scope::AdminWrite));
        assert!(!scopes.contains(&Scope::BillingRead));
    }
}