-- Group (team) role assignments
-- Each group can have one role that all members inherit
CREATE TABLE IF NOT EXISTS team_roles (
    team_id UUID PRIMARY KEY REFERENCES teams(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE
);
