-- Add RBAC support with users and role permissions

-- Create user role and permission enums
CREATE TYPE user_role AS ENUM ('owner', 'admin', 'developer', 'viewer');
CREATE TYPE permission AS ENUM (
    'manage_tenant', 
    'manage_projects', 
    'manage_api_keys', 
    'manage_users', 
    'view_audit_logs', 
    'publish_events', 
    'subscribe_events', 
    'view_billing', 
    'manage_billing'
);

-- Users table for RBAC
CREATE TABLE users (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    email VARCHAR(255) NOT NULL,
    name VARCHAR(255) NOT NULL,
    role user_role NOT NULL DEFAULT 'viewer',
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Role permissions table for configurable RBAC
CREATE TABLE role_permissions (
    id VARCHAR(36) PRIMARY KEY,
    role user_role NOT NULL,
    permission permission NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(role, permission)
);

-- Create indexes for users
CREATE INDEX idx_users_tenant_id ON users(tenant_id);
CREATE INDEX idx_users_email ON users(email);
CREATE INDEX idx_users_tenant_email ON users(tenant_id, email);
CREATE INDEX idx_users_role ON users(role);
CREATE INDEX idx_users_active ON users(is_active);

-- Create indexes for role permissions
CREATE INDEX idx_role_permissions_role ON role_permissions(role);
CREATE INDEX idx_role_permissions_permission ON role_permissions(permission);

-- Add constraint for tenant isolation
ALTER TABLE users ADD CONSTRAINT chk_users_tenant_isolation 
    CHECK (tenant_id IS NOT NULL);

-- Enable RLS for users
ALTER TABLE users ENABLE ROW LEVEL SECURITY;

-- Add unique constraint for user email per tenant
ALTER TABLE users ADD CONSTRAINT unique_user_email_per_tenant 
    UNIQUE (tenant_id, email);

-- Add trigger for users updated_at
CREATE TRIGGER update_users_updated_at BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Insert default role permissions
INSERT INTO role_permissions (id, role, permission, created_at) VALUES
    -- Owner permissions (all permissions)
    (gen_random_uuid()::text, 'owner', 'manage_tenant', NOW()),
    (gen_random_uuid()::text, 'owner', 'manage_projects', NOW()),
    (gen_random_uuid()::text, 'owner', 'manage_api_keys', NOW()),
    (gen_random_uuid()::text, 'owner', 'manage_users', NOW()),
    (gen_random_uuid()::text, 'owner', 'view_audit_logs', NOW()),
    (gen_random_uuid()::text, 'owner', 'publish_events', NOW()),
    (gen_random_uuid()::text, 'owner', 'subscribe_events', NOW()),
    (gen_random_uuid()::text, 'owner', 'view_billing', NOW()),
    (gen_random_uuid()::text, 'owner', 'manage_billing', NOW()),
    
    -- Admin permissions
    (gen_random_uuid()::text, 'admin', 'manage_projects', NOW()),
    (gen_random_uuid()::text, 'admin', 'manage_api_keys', NOW()),
    (gen_random_uuid()::text, 'admin', 'manage_users', NOW()),
    (gen_random_uuid()::text, 'admin', 'view_audit_logs', NOW()),
    (gen_random_uuid()::text, 'admin', 'publish_events', NOW()),
    (gen_random_uuid()::text, 'admin', 'subscribe_events', NOW()),
    (gen_random_uuid()::text, 'admin', 'view_billing', NOW()),
    
    -- Developer permissions
    (gen_random_uuid()::text, 'developer', 'manage_api_keys', NOW()),
    (gen_random_uuid()::text, 'developer', 'publish_events', NOW()),
    (gen_random_uuid()::text, 'developer', 'subscribe_events', NOW()),
    (gen_random_uuid()::text, 'developer', 'view_billing', NOW()),
    
    -- Viewer permissions
    (gen_random_uuid()::text, 'viewer', 'subscribe_events', NOW()),
    (gen_random_uuid()::text, 'viewer', 'view_billing', NOW());