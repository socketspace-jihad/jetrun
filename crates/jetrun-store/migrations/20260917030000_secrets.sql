-- Secrets vault (org-level, encrypted at rest)
CREATE TABLE IF NOT EXISTS secrets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    secret_type TEXT NOT NULL,
    encrypted_value TEXT NOT NULL,
    ssh_public_key TEXT,
    created_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(org_id, name)
);

ALTER TABLE projects ADD COLUMN IF NOT EXISTS credential_id UUID REFERENCES secrets(id) ON DELETE SET NULL;
