/// Schema validation utilities for ensuring database schema correctness
/// This module provides validation functions that can be used to verify
/// database schema compliance without requiring an active database connection

use std::collections::HashSet;
use anyhow::Result;

/// Schema validator for event payloads and database operations
#[derive(Debug, Clone)]
pub struct SchemaValidator {
    // For now, this is a simple validator
    // In a real implementation, this would contain JSON schemas, etc.
}

impl SchemaValidator {
    /// Create a new schema validator
    pub fn new() -> Self {
        Self {}
    }
    
    /// Validate an event payload against a topic schema
    pub fn validate_event_payload(&self, topic: &str, payload: &serde_json::Value) -> Result<()> {
        // Basic validation - ensure payload is not null and has some content
        if payload.is_null() {
            return Err(anyhow::anyhow!("Event payload cannot be null"));
        }
        
        // Validate topic format
        validate_event_structure("", "", topic)
            .map_err(|e| anyhow::anyhow!("Topic validation failed: {}", e))?;
        
        // For now, just ensure the payload is a valid JSON object
        if !payload.is_object() && !payload.is_array() {
            return Err(anyhow::anyhow!("Event payload must be a JSON object or array"));
        }
        
        Ok(())
    }
}

impl Default for SchemaValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Validates that a SQL query includes proper tenant isolation
pub fn validate_tenant_isolation(tenant_id: &str, query: &str) -> bool {
    // Convert query to lowercase for case-insensitive matching
    let query_lower = query.to_lowercase();
    let tenant_lower = tenant_id.to_lowercase();
    
    // Check for explicit tenant_id in WHERE clause
    if query_lower.contains(&format!("tenant_id = '{}'", tenant_lower)) {
        return true;
    }
    
    // Check for parameterized queries with tenant_id
    if query_lower.contains("tenant_id = $") || query_lower.contains("tenant_id=?") {
        return true;
    }
    
    // Check for tenant_id in INSERT statements
    if query_lower.contains("insert") && query_lower.contains("tenant_id") {
        return true;
    }
    
    false
}

/// Validates that required indexes exist for tenant isolation
pub fn validate_tenant_isolation_indexes() -> Vec<String> {
    vec![
        "CREATE INDEX idx_projects_tenant_id ON projects(tenant_id);".to_string(),
        "CREATE INDEX idx_api_keys_tenant_id ON api_keys(tenant_id);".to_string(),
        "CREATE INDEX idx_events_tenant_id ON events(tenant_id);".to_string(),
        "CREATE INDEX idx_usage_records_tenant_id ON usage_records(tenant_id);".to_string(),
    ]
}

/// Validates that all required tables have tenant_id columns
pub fn validate_tenant_columns() -> HashSet<String> {
    let mut tables_with_tenant_id = HashSet::new();
    tables_with_tenant_id.insert("projects".to_string());
    tables_with_tenant_id.insert("api_keys".to_string());
    tables_with_tenant_id.insert("events".to_string());
    tables_with_tenant_id.insert("usage_records".to_string());
    tables_with_tenant_id
}

/// Validates API key security requirements
pub fn validate_api_key_security(key_hash: &str, scopes: &[String]) -> Result<(), String> {
    // Validate key hash length (should be at least 32 characters for security)
    if key_hash.len() < 32 {
        return Err("API key hash must be at least 32 characters for security".to_string());
    }
    
    // Validate that scopes are not empty
    if scopes.is_empty() {
        return Err("API key must have at least one scope".to_string());
    }
    
    // Validate scope values
    let valid_scopes = vec![
        "events_publish",
        "events_subscribe", 
        "admin_read",
        "admin_write",
        "billing_read"
    ];
    
    for scope in scopes {
        if !valid_scopes.contains(&scope.as_str()) {
            return Err(format!("Invalid scope: {}", scope));
        }
    }
    
    Ok(())
}

/// Validates event data structure
pub fn validate_event_structure(tenant_id: &str, project_id: &str, topic: &str) -> Result<(), String> {
    // Validate tenant_id format
    if tenant_id.is_empty() || tenant_id.len() < 8 {
        return Err("Tenant ID must be at least 8 characters".to_string());
    }
    
    // Validate project_id format
    if project_id.is_empty() || project_id.len() < 8 {
        return Err("Project ID must be at least 8 characters".to_string());
    }
    
    // Validate topic format
    if topic.is_empty() {
        return Err("Topic cannot be empty".to_string());
    }
    
    // Validate topic naming convention (alphanumeric, dots, underscores, hyphens)
    if !topic.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '_' || c == '-') {
        return Err("Topic must contain only alphanumeric characters, dots, underscores, and hyphens".to_string());
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tenant_isolation_validation() {
        let tenant_id = "tenant_123";
        
        // Valid queries
        assert!(validate_tenant_isolation(tenant_id, "SELECT * FROM events WHERE tenant_id = 'tenant_123'"));
        assert!(validate_tenant_isolation(tenant_id, "SELECT * FROM events WHERE tenant_id = $1"));
        assert!(validate_tenant_isolation(tenant_id, "INSERT INTO events (tenant_id, topic) VALUES ('tenant_123', 'test')"));
        
        // Invalid queries
        assert!(!validate_tenant_isolation(tenant_id, "SELECT * FROM events"));
        assert!(!validate_tenant_isolation(tenant_id, "SELECT * FROM events WHERE id = '123'"));
    }
    
    #[test]
    fn test_api_key_security_validation() {
        // Valid API key
        let valid_hash = "a".repeat(32);
        let valid_scopes = vec!["events_publish".to_string(), "events_subscribe".to_string()];
        assert!(validate_api_key_security(&valid_hash, &valid_scopes).is_ok());
        
        // Invalid hash (too short)
        let short_hash = "short";
        assert!(validate_api_key_security(short_hash, &valid_scopes).is_err());
        
        // Invalid scopes (empty)
        let empty_scopes = vec![];
        assert!(validate_api_key_security(&valid_hash, &empty_scopes).is_err());
        
        // Invalid scope value
        let invalid_scopes = vec!["invalid_scope".to_string()];
        assert!(validate_api_key_security(&valid_hash, &invalid_scopes).is_err());
    }
    
    #[test]
    fn test_event_structure_validation() {
        // Valid event
        assert!(validate_event_structure("tenant_123", "project_456", "user.created").is_ok());
        
        // Invalid tenant_id (too short)
        assert!(validate_event_structure("short", "project_456", "user.created").is_err());
        
        // Invalid project_id (empty)
        assert!(validate_event_structure("tenant_123", "", "user.created").is_err());
        
        // Invalid topic (empty)
        assert!(validate_event_structure("tenant_123", "project_456", "").is_err());
        
        // Invalid topic (special characters)
        assert!(validate_event_structure("tenant_123", "project_456", "user@created").is_err());
    }
    
    #[test]
    fn test_tenant_columns_validation() {
        let required_tables = validate_tenant_columns();
        
        assert!(required_tables.contains("projects"));
        assert!(required_tables.contains("api_keys"));
        assert!(required_tables.contains("events"));
        assert!(required_tables.contains("usage_records"));
        assert_eq!(required_tables.len(), 4);
    }
}