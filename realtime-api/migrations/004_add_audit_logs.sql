-- Add audit logs table for RBAC operations
CREATE TABLE IF NOT EXISTS audit_logs (
    id VARCHAR(255) PRIMARY KEY,
    tenant_id VARCHAR(255) NOT NULL,
    operation VARCHAR(255) NOT NULL,
    details TEXT NOT NULL,
    performed_by VARCHAR(255) NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- Foreign key constraint
    CONSTRAINT fk_audit_logs_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(id) ON DELETE CASCADE
);

-- Index for efficient querying
CREATE INDEX IF NOT EXISTS idx_audit_logs_tenant_id ON audit_logs(tenant_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at ON audit_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_audit_logs_operation ON audit_logs(operation);
CREATE INDEX IF NOT EXISTS idx_audit_logs_performed_by ON audit_logs(performed_by);

-- Add role permissions table for flexible RBAC
CREATE TABLE IF NOT EXISTS role_permissions (
    id VARCHAR(255) PRIMARY KEY,
    role VARCHAR(50) NOT NULL,
    permission VARCHAR(100) NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- Unique constraint to prevent duplicate role-permission mappings
    CONSTRAINT unique_role_permission UNIQUE (role, permission)
);

-- Insert default role permissions
INSERT INTO role_permissions (id, role, permission) VALUES
    -- Owner permissions (all permissions)
    (gen_random_uuid()::text, 'owner', 'manage_tenant'),
    (gen_random_uuid()::text, 'owner', 'manage_projects'),
    (gen_random_uuid()::text, 'owner', 'manage_api_keys'),
    (gen_random_uuid()::text, 'owner', 'manage_users'),
    (gen_random_uuid()::text, 'owner', 'view_audit_logs'),
    (gen_random_uuid()::text, 'owner', 'publish_events'),
    (gen_random_uuid()::text, 'owner', 'subscribe_events'),
    (gen_random_uuid()::text, 'owner', 'view_billing'),
    (gen_random_uuid()::text, 'owner', 'manage_billing'),
    
    -- Admin permissions (most permissions except tenant management)
    (gen_random_uuid()::text, 'admin', 'manage_projects'),
    (gen_random_uuid()::text, 'admin', 'manage_api_keys'),
    (gen_random_uuid()::text, 'admin', 'manage_users'),
    (gen_random_uuid()::text, 'admin', 'view_audit_logs'),
    (gen_random_uuid()::text, 'admin', 'publish_events'),
    (gen_random_uuid()::text, 'admin', 'subscribe_events'),
    (gen_random_uuid()::text, 'admin', 'view_billing'),
    
    -- Developer permissions (API and event operations)
    (gen_random_uuid()::text, 'developer', 'manage_api_keys'),
    (gen_random_uuid()::text, 'developer', 'publish_events'),
    (gen_random_uuid()::text, 'developer', 'subscribe_events'),
    (gen_random_uuid()::text, 'developer', 'view_billing'),
    
    -- Viewer permissions (read-only access)
    (gen_random_uuid()::text, 'viewer', 'subscribe_events'),
    (gen_random_uuid()::text, 'viewer', 'view_billing')
ON CONFLICT (role, permission) DO NOTHING;