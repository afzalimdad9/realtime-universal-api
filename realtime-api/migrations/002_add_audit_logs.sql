-- Add audit logs table for tracking billing and administrative actions
CREATE TABLE audit_logs (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    action VARCHAR(255) NOT NULL,
    details JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create indexes for audit logs
CREATE INDEX idx_audit_logs_tenant_id ON audit_logs(tenant_id);
CREATE INDEX idx_audit_logs_action ON audit_logs(action);
CREATE INDEX idx_audit_logs_created_at ON audit_logs(created_at);
CREATE INDEX idx_audit_logs_tenant_action ON audit_logs(tenant_id, action);

-- Add constraint for tenant isolation
ALTER TABLE audit_logs ADD CONSTRAINT chk_audit_logs_tenant_isolation 
    CHECK (tenant_id IS NOT NULL);

-- Enable RLS for audit logs
ALTER TABLE audit_logs ENABLE ROW LEVEL SECURITY;

-- Add unique constraint for usage records to prevent duplicates
ALTER TABLE usage_records ADD CONSTRAINT unique_usage_record 
    UNIQUE (tenant_id, project_id, metric, window_start);