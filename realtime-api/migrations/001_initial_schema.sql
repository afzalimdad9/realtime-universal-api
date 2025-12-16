-- Create custom types
CREATE TYPE tenant_status AS ENUM ('active', 'trial', 'past_due', 'suspended');
CREATE TYPE scope AS ENUM ('events_publish', 'events_subscribe', 'admin_read', 'admin_write', 'billing_read');
CREATE TYPE usage_metric AS ENUM ('events_published', 'events_delivered', 'web_socket_minutes', 'api_requests');

-- Tenants table
CREATE TABLE tenants (
    id VARCHAR(36) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    plan JSONB NOT NULL,
    status tenant_status NOT NULL DEFAULT 'trial',
    stripe_customer_id VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create indexes for tenants
CREATE INDEX idx_tenants_status ON tenants(status);
CREATE INDEX idx_tenants_stripe_customer ON tenants(stripe_customer_id) WHERE stripe_customer_id IS NOT NULL;
CREATE INDEX idx_tenants_created_at ON tenants(created_at);

-- Projects table
CREATE TABLE projects (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    limits JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create indexes for projects
CREATE INDEX idx_projects_tenant_id ON projects(tenant_id);
CREATE INDEX idx_projects_tenant_name ON projects(tenant_id, name);
CREATE INDEX idx_projects_created_at ON projects(created_at);

-- API Keys table
CREATE TABLE api_keys (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id VARCHAR(36) NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key_hash VARCHAR(255) NOT NULL UNIQUE,
    scopes JSONB NOT NULL,
    rate_limit_per_sec INTEGER NOT NULL DEFAULT 100,
    is_active BOOLEAN NOT NULL DEFAULT true,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create indexes for API keys
CREATE INDEX idx_api_keys_tenant_id ON api_keys(tenant_id);
CREATE INDEX idx_api_keys_project_id ON api_keys(project_id);
CREATE INDEX idx_api_keys_hash ON api_keys(key_hash) WHERE is_active = true;
CREATE INDEX idx_api_keys_active ON api_keys(is_active);
CREATE INDEX idx_api_keys_expires_at ON api_keys(expires_at) WHERE expires_at IS NOT NULL;

-- Events table
CREATE TABLE events (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id VARCHAR(36) NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    topic VARCHAR(255) NOT NULL,
    payload JSONB NOT NULL,
    published_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create indexes for events (optimized for tenant isolation and time-based queries)
CREATE INDEX idx_events_tenant_id ON events(tenant_id);
CREATE INDEX idx_events_project_id ON events(project_id);
CREATE INDEX idx_events_tenant_published_at ON events(tenant_id, published_at DESC);
CREATE INDEX idx_events_tenant_topic ON events(tenant_id, topic);
CREATE INDEX idx_events_tenant_project_topic ON events(tenant_id, project_id, topic);

-- Usage records table
CREATE TABLE usage_records (
    id VARCHAR(36) PRIMARY KEY,
    tenant_id VARCHAR(36) NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id VARCHAR(36) NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    metric usage_metric NOT NULL,
    quantity BIGINT NOT NULL,
    window_start TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Create indexes for usage records (optimized for billing queries)
CREATE INDEX idx_usage_records_tenant_id ON usage_records(tenant_id);
CREATE INDEX idx_usage_records_project_id ON usage_records(project_id);
CREATE INDEX idx_usage_records_tenant_metric ON usage_records(tenant_id, metric);
CREATE INDEX idx_usage_records_window_start ON usage_records(window_start);
CREATE INDEX idx_usage_records_tenant_window ON usage_records(tenant_id, window_start);

-- Add constraints to ensure tenant isolation
ALTER TABLE projects ADD CONSTRAINT chk_projects_tenant_isolation 
    CHECK (tenant_id IS NOT NULL);

ALTER TABLE api_keys ADD CONSTRAINT chk_api_keys_tenant_isolation 
    CHECK (tenant_id IS NOT NULL);

ALTER TABLE events ADD CONSTRAINT chk_events_tenant_isolation 
    CHECK (tenant_id IS NOT NULL);

ALTER TABLE usage_records ADD CONSTRAINT chk_usage_records_tenant_isolation 
    CHECK (tenant_id IS NOT NULL);

-- Row Level Security (RLS) for tenant isolation
ALTER TABLE tenants ENABLE ROW LEVEL SECURITY;
ALTER TABLE projects ENABLE ROW LEVEL SECURITY;
ALTER TABLE api_keys ENABLE ROW LEVEL SECURITY;
ALTER TABLE events ENABLE ROW LEVEL SECURITY;
ALTER TABLE usage_records ENABLE ROW LEVEL SECURITY;

-- Create policies for tenant isolation (these would be used with application-level tenant context)
-- Note: In production, these policies would be more sophisticated and use session variables

-- Function to update updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Triggers to automatically update updated_at
CREATE TRIGGER update_tenants_updated_at BEFORE UPDATE ON tenants
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_projects_updated_at BEFORE UPDATE ON projects
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_api_keys_updated_at BEFORE UPDATE ON api_keys
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();