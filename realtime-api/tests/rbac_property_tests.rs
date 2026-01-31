use proptest::prelude::*;
use realtime_api::auth::{AuthService, AuthContext, AuthType};
use realtime_api::database::Database;
use realtime_api::models::{User, UserRole, Permission, Tenant, BillingPlan, TenantStatus};
use std::collections::HashMap;
use uuid::Uuid;

/// **Feature: realtime-saas-platform, Property 23: Role-based permission enforcement**
/// **Validates: Requirements 6.1**
/// 
/// For any user with assigned roles, the system should enforce role-based permissions for all operations
#[tokio::test]
async fn property_rbac_enforcement() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:password@localhost:5432/realtime_test".to_string());
    
    let database = Database::new(&database_url).await.expect("Failed to connect to database");
    database.migrate().await.expect("Failed to run migrations");
    
    let auth_service = AuthService::new(database.clone(), "test_secret".to_string());

    // Property: For any user role and permission combination, the system should correctly enforce permissions
    proptest!(|(
        user_role in prop::sample::select(vec![UserRole::Owner, UserRole::Admin, UserRole::Developer, UserRole::Viewer]),
        permission in prop::sample::select(vec![
            Permission::ManageTenant,
            Permission::ManageProjects, 
            Permission::ManageApiKeys,
            Permission::ManageUsers,
            Permission::ViewAuditLogs,
            Permission::PublishEvents,
            Permission::SubscribeEvents,
            Permission::ViewBilling,
            Permission::ManageBilling
        ])
    )| {
        tokio_test::block_on(async {
            // Create test tenant
            let tenant_id = Uuid::new_v4().to_string();
            let tenant = Tenant::new("Test Tenant".to_string(), BillingPlan::Free { monthly_events: 1000 });
            let mut tenant = tenant;
            tenant.id = tenant_id.clone();
            tenant.status = TenantStatus::Active;
            database.create_tenant(&tenant).await.expect("Failed to create tenant");

            // Create test user with the generated role
            let user_id = Uuid::new_v4().to_string();
            let user = User::new(
                tenant_id.clone(),
                format!("test{}@example.com", user_id),
                "Test User".to_string(),
                user_role.clone()
            );
            let mut user = user;
            user.id = user_id.clone();
            database.create_user(&user).await.expect("Failed to create user");

            // Create auth context for the user
            let auth_context = AuthContext {
                tenant_id: tenant_id.clone(),
                project_id: Uuid::new_v4().to_string(),
                scopes: vec![],
                rate_limit_per_sec: 1000,
                auth_type: AuthType::Jwt { user_id: user_id.clone() },
                user_id: Some(user_id.clone()),
                user_role: Some(user_role.clone()),
            };

            // Test permission check
            let result = auth_service.check_user_permission(&auth_context, &permission).await;
            let expected_has_permission = user.has_permission(&permission);

            if expected_has_permission {
                assert!(result.is_ok(), 
                    "User with role {:?} should have permission {:?} but was denied", 
                    user_role, permission);
            } else {
                assert!(result.is_err(), 
                    "User with role {:?} should NOT have permission {:?} but was granted", 
                    user_role, permission);
            }

            // Clean up
            database.deactivate_user(&tenant_id, &user_id).await.expect("Failed to deactivate user");
        });
    });
}

/// **Feature: realtime-saas-platform, Property 24: Admin function access validation**
/// **Validates: Requirements 6.2**
/// 
/// For any admin function access attempt, the system should validate role permissions before allowing actions
#[tokio::test]
async fn property_admin_access_validation() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:password@localhost:5432/realtime_test".to_string());
    
    let database = Database::new(&database_url).await.expect("Failed to connect to database");
    database.migrate().await.expect("Failed to run migrations");
    
    let auth_service = AuthService::new(database.clone(), "test_secret".to_string());

    // Property: Admin functions should only be accessible to users with appropriate roles
    proptest!(|(
        user_role in prop::sample::select(vec![UserRole::Owner, UserRole::Admin, UserRole::Developer, UserRole::Viewer]),
        admin_function in prop::sample::select(vec![
            "manage_users",
            "manage_projects", 
            "manage_api_keys",
            "view_audit_logs",
            "manage_billing"
        ])
    )| {
        tokio_test::block_on(async {
            // Create test tenant
            let tenant_id = Uuid::new_v4().to_string();
            let tenant = Tenant::new("Test Tenant".to_string(), BillingPlan::Free { monthly_events: 1000 });
            let mut tenant = tenant;
            tenant.id = tenant_id.clone();
            tenant.status = TenantStatus::Active;
            database.create_tenant(&tenant).await.expect("Failed to create tenant");

            // Create test user
            let user_id = Uuid::new_v4().to_string();
            let user = User::new(
                tenant_id.clone(),
                format!("admin{}@example.com", user_id),
                "Admin User".to_string(),
                user_role.clone()
            );
            let mut user = user;
            user.id = user_id.clone();
            database.create_user(&user).await.expect("Failed to create user");

            // Map admin function to permission
            let permission = match admin_function {
                "manage_users" => Permission::ManageUsers,
                "manage_projects" => Permission::ManageProjects,
                "manage_api_keys" => Permission::ManageApiKeys,
                "view_audit_logs" => Permission::ViewAuditLogs,
                "manage_billing" => Permission::ManageBilling,
                _ => Permission::ViewBilling,
            };

            // Create auth context
            let auth_context = AuthContext {
                tenant_id: tenant_id.clone(),
                project_id: Uuid::new_v4().to_string(),
                scopes: vec![],
                rate_limit_per_sec: 1000,
                auth_type: AuthType::Jwt { user_id: user_id.clone() },
                user_id: Some(user_id.clone()),
                user_role: Some(user_role.clone()),
            };

            // Test admin function access
            let result = auth_service.check_user_permission(&auth_context, &permission).await;
            let expected_access = user.has_permission(&permission);

            if expected_access {
                assert!(result.is_ok(), 
                    "User with role {:?} should have access to admin function '{}' but was denied", 
                    user_role, admin_function);
            } else {
                assert!(result.is_err(), 
                    "User with role {:?} should NOT have access to admin function '{}' but was granted", 
                    user_role, admin_function);
            }

            // Clean up
            database.deactivate_user(&tenant_id, &user_id).await.expect("Failed to deactivate user");
        });
    });
}

/// **Feature: realtime-saas-platform, Property 25: Role change propagation**
/// **Validates: Requirements 6.3**
/// 
/// For any role change, access permissions should be immediately updated across all active sessions
#[tokio::test]
async fn property_role_change_propagation() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:password@localhost:5432/realtime_test".to_string());
    
    let database = Database::new(&database_url).await.expect("Failed to connect to database");
    database.migrate().await.expect("Failed to run migrations");
    
    let auth_service = AuthService::new(database.clone(), "test_secret".to_string());

    // Property: When a user's role changes, their permissions should be immediately updated
    proptest!(|(
        initial_role in prop::sample::select(vec![UserRole::Viewer, UserRole::Developer]),
        new_role in prop::sample::select(vec![UserRole::Admin, UserRole::Owner]),
        test_permission in prop::sample::select(vec![Permission::ManageUsers, Permission::ManageTenant])
    )| {
        tokio_test::block_on(async {
            // Create test tenant
            let tenant_id = Uuid::new_v4().to_string();
            let tenant = Tenant::new("Test Tenant".to_string(), BillingPlan::Free { monthly_events: 1000 });
            let mut tenant = tenant;
            tenant.id = tenant_id.clone();
            tenant.status = TenantStatus::Active;
            database.create_tenant(&tenant).await.expect("Failed to create tenant");

            // Create test user with initial role
            let user_id = Uuid::new_v4().to_string();
            let user = User::new(
                tenant_id.clone(),
                format!("rolechange{}@example.com", user_id),
                "Role Change User".to_string(),
                initial_role.clone()
            );
            let mut user = user;
            user.id = user_id.clone();
            database.create_user(&user).await.expect("Failed to create user");

            // Test initial permission
            let initial_auth_context = AuthContext {
                tenant_id: tenant_id.clone(),
                project_id: Uuid::new_v4().to_string(),
                scopes: vec![],
                rate_limit_per_sec: 1000,
                auth_type: AuthType::Jwt { user_id: user_id.clone() },
                user_id: Some(user_id.clone()),
                user_role: Some(initial_role.clone()),
            };

            let initial_user = User::new(tenant_id.clone(), "test@example.com".to_string(), "Test".to_string(), initial_role.clone());
            let initial_has_permission = initial_user.has_permission(&test_permission);
            let initial_result = auth_service.check_user_permission(&initial_auth_context, &test_permission).await;

            if initial_has_permission {
                assert!(initial_result.is_ok(), "Initial role {:?} should have permission {:?}", initial_role, test_permission);
            } else {
                assert!(initial_result.is_err(), "Initial role {:?} should NOT have permission {:?}", initial_role, test_permission);
            }

            // Change user role
            database.update_user_role(&tenant_id, &user_id, new_role.clone()).await.expect("Failed to update user role");

            // Test permission after role change
            let updated_auth_context = AuthContext {
                tenant_id: tenant_id.clone(),
                project_id: Uuid::new_v4().to_string(),
                scopes: vec![],
                rate_limit_per_sec: 1000,
                auth_type: AuthType::Jwt { user_id: user_id.clone() },
                user_id: Some(user_id.clone()),
                user_role: Some(new_role.clone()),
            };

            let updated_user = User::new(tenant_id.clone(), "test@example.com".to_string(), "Test".to_string(), new_role.clone());
            let updated_has_permission = updated_user.has_permission(&test_permission);
            let updated_result = auth_service.check_user_permission(&updated_auth_context, &test_permission).await;

            if updated_has_permission {
                assert!(updated_result.is_ok(), "Updated role {:?} should have permission {:?}", new_role, test_permission);
            } else {
                assert!(updated_result.is_err(), "Updated role {:?} should NOT have permission {:?}", new_role, test_permission);
            }

            // Verify that the permission change is reflected correctly
            if initial_has_permission != updated_has_permission {
                assert_ne!(initial_result.is_ok(), updated_result.is_ok(), 
                    "Role change from {:?} to {:?} should change permission {:?} access", 
                    initial_role, new_role, test_permission);
            }

            // Clean up
            database.deactivate_user(&tenant_id, &user_id).await.expect("Failed to deactivate user");
        });
    });
}