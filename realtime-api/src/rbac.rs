use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info, warn};

use crate::auth::{AuthContext, AuthService, AuthError};
use crate::models::{Permission, UserRole};
use crate::Database;

/// RBAC middleware for role-based access control
#[derive(Clone)]
pub struct RbacMiddleware {
    auth_service: AuthService,
    database: Database,
    // Cache for active user sessions to enable immediate permission updates
    active_sessions: Arc<Mutex<HashMap<String, UserRole>>>,
}

impl RbacMiddleware {
    pub fn new(auth_service: AuthService, database: Database) -> Self {
        Self {
            auth_service,
            database,
            active_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Update user role in active sessions (for immediate propagation)
    pub fn update_user_role_in_sessions(&self, user_id: &str, new_role: UserRole) {
        let mut sessions = self.active_sessions.lock().unwrap();
        sessions.insert(user_id.to_string(), new_role);
        info!("Updated role for user {} in active sessions", user_id);
    }

    /// Remove user from active sessions (on logout/deactivation)
    pub fn remove_user_from_sessions(&self, user_id: &str) {
        let mut sessions = self.active_sessions.lock().unwrap();
        sessions.remove(user_id);
        info!("Removed user {} from active sessions", user_id);
    }

    /// Get current role for user (checks active sessions first, then database)
    pub async fn get_current_user_role(&self, user_id: &str) -> Result<UserRole, AuthError> {
        // Check active sessions first for immediate updates
        {
            let sessions = self.active_sessions.lock().unwrap();
            if let Some(role) = sessions.get(user_id) {
                debug!("Found role {:?} for user {} in active sessions", role, user_id);
                return Ok(role.clone());
            }
        }

        // Fall back to database
        let user = self
            .database
            .get_user(user_id)
            .await?
            .ok_or(AuthError::InvalidJwt)?;

        if !user.is_active {
            return Err(AuthError::TenantSuspended);
        }

        // Cache the role in active sessions
        {
            let mut sessions = self.active_sessions.lock().unwrap();
            sessions.insert(user_id.to_string(), user.role.clone());
        }

        Ok(user.role)
    }
}

/// Middleware function that requires a specific permission
pub fn require_permission(
    required_permission: Permission,
) -> impl Clone
       + Fn(
    State<RbacMiddleware>,
    Request,
    Next,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<Response, StatusCode>> + Send>,
> {
    move |State(rbac): State<RbacMiddleware>, mut request: Request, next: Next| {
        let permission = required_permission.clone();
        Box::pin(async move {
            // Extract auth context from request extensions
            let auth_context = request
                .extensions()
                .get::<AuthContext>()
                .ok_or(StatusCode::UNAUTHORIZED)?
                .clone();

            // Check permission based on auth type
            match &auth_context.user_id {
                Some(user_id) => {
                    // JWT-based authentication - check user role permissions
                    let user_role = rbac
                        .get_current_user_role(user_id)
                        .await
                        .map_err(|e| {
                            error!("Failed to get user role: {}", e);
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;

                    // Create a temporary user to check permissions
                    let temp_user = crate::models::User::new(
                        auth_context.tenant_id.clone(),
                        "temp@example.com".to_string(),
                        "Temp User".to_string(),
                        user_role,
                    );

                    if !temp_user.has_permission(&permission) {
                        warn!(
                            "User {} with role {:?} denied access to permission {:?}",
                            user_id, temp_user.role, permission
                        );
                        return Err(StatusCode::FORBIDDEN);
                    }

                    // Update auth context with current role
                    let mut updated_auth = auth_context.clone();
                    updated_auth.user_role = Some(temp_user.role);
                    request.extensions_mut().insert(updated_auth);

                    debug!(
                        "User {} granted access to permission {:?}",
                        user_id, permission
                    );
                }
                None => {
                    // API key-based authentication - use scope-based permission check
                    let result = rbac
                        .auth_service
                        .check_user_permission(&auth_context, &permission)
                        .await;

                    if result.is_err() {
                        warn!(
                            "API key denied access to permission {:?}: {:?}",
                            permission, result
                        );
                        return Err(StatusCode::FORBIDDEN);
                    }

                    debug!("API key granted access to permission {:?}", permission);
                }
            }

            Ok(next.run(request).await)
        })
    }
}

/// Audit logging for RBAC operations
pub struct AuditLogger {
    database: Database,
}

impl AuditLogger {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    /// Log a role change operation
    pub async fn log_role_change(
        &self,
        tenant_id: &str,
        user_id: &str,
        old_role: UserRole,
        new_role: UserRole,
        changed_by: &str,
    ) -> Result<(), anyhow::Error> {
        let audit_entry = format!(
            "Role changed for user {} in tenant {}: {:?} -> {:?} (changed by: {})",
            user_id, tenant_id, old_role, new_role, changed_by
        );

        // In a real implementation, this would write to an audit_logs table
        info!("AUDIT: {}", audit_entry);

        // For now, we'll just log it. In a full implementation, you would:
        // self.database.create_audit_log(tenant_id, "role_change", &audit_entry, changed_by).await?;

        Ok(())
    }

    /// Log an admin operation
    pub async fn log_admin_operation(
        &self,
        tenant_id: &str,
        operation: &str,
        details: &str,
        performed_by: &str,
    ) -> Result<(), anyhow::Error> {
        let audit_entry = format!(
            "Admin operation '{}' in tenant {}: {} (performed by: {})",
            operation, tenant_id, details, performed_by
        );

        info!("AUDIT: {}", audit_entry);

        // In a real implementation:
        // self.database.create_audit_log(tenant_id, operation, details, performed_by).await?;

        Ok(())
    }

    /// Log a permission check
    pub async fn log_permission_check(
        &self,
        tenant_id: &str,
        user_id: &str,
        permission: &Permission,
        granted: bool,
    ) -> Result<(), anyhow::Error> {
        let audit_entry = format!(
            "Permission check for user {} in tenant {}: {:?} - {}",
            user_id,
            tenant_id,
            permission,
            if granted { "GRANTED" } else { "DENIED" }
        );

        debug!("AUDIT: {}", audit_entry);

        // In a real implementation:
        // self.database.create_audit_log(tenant_id, "permission_check", &audit_entry, user_id).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Tenant, BillingPlan, TenantStatus, User};
    use uuid::Uuid;

    #[tokio::test]
    async fn test_rbac_middleware_creation() {
        // This is a basic test to ensure the RBAC middleware can be created
        // In a real test environment, we would set up proper database connections

        // For now, just test that the structures can be instantiated
        // let database = Database::new("test_url").await.unwrap();
        // let auth_service = AuthService::new(database.clone(), "test_secret".to_string());
        // let rbac = RbacMiddleware::new(auth_service, database);

        // assert!(rbac.active_sessions.lock().unwrap().is_empty());

        println!("RBAC middleware test placeholder - requires test infrastructure");
    }

    #[test]
    fn test_permission_checking_logic() {
        // Test the permission checking logic without requiring database
        let owner = User::new(
            "tenant_123".to_string(),
            "owner@example.com".to_string(),
            "Owner User".to_string(),
            UserRole::Owner,
        );

        let admin = User::new(
            "tenant_123".to_string(),
            "admin@example.com".to_string(),
            "Admin User".to_string(),
            UserRole::Admin,
        );

        let viewer = User::new(
            "tenant_123".to_string(),
            "viewer@example.com".to_string(),
            "Viewer User".to_string(),
            UserRole::Viewer,
        );

        // Test owner permissions
        assert!(owner.has_permission(&Permission::ManageTenant));
        assert!(owner.has_permission(&Permission::ManageUsers));
        assert!(owner.has_permission(&Permission::ViewAuditLogs));

        // Test admin permissions
        assert!(!admin.has_permission(&Permission::ManageTenant));
        assert!(admin.has_permission(&Permission::ManageUsers));
        assert!(admin.has_permission(&Permission::ViewAuditLogs));

        // Test viewer permissions
        assert!(!viewer.has_permission(&Permission::ManageTenant));
        assert!(!viewer.has_permission(&Permission::ManageUsers));
        assert!(!viewer.has_permission(&Permission::ViewAuditLogs));
        assert!(viewer.has_permission(&Permission::SubscribeEvents));
    }
}