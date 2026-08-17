-- The permission catalog.
--
-- Seeded in SQL rather than at runtime because the primary key is the permission
-- string itself -- no id generation needed -- and because `role_permissions`
-- references it with ON DELETE RESTRICT, so a permission cannot vanish from
-- under a role that grants it.
--
-- System *roles* are seeded in Rust instead (see `bootstrap`), since they need
-- generated ULIDs. The Rust-side catalog in `permissions.rs` is asserted to
-- match this list by test, so the two cannot drift.
--
-- Naming: `resource.action`, always singular resource. A permission answers one
-- question and never implies another -- `pipeline.write` does not confer
-- `pipeline.run`, because editing a definition and executing it are different
-- privileges and plenty of teams grant only the first.

INSERT INTO permissions (key, description) VALUES
    -- organization
    ('org.read',        'View organization settings and membership'),
    ('org.manage',      'Rename the organization and change its settings'),
    ('member.invite',   'Invite people to the organization'),
    ('member.remove',   'Remove members from the organization'),
    ('team.manage',     'Create, delete, and change the membership of teams'),
    ('role.manage',     'Create custom roles and assign roles to principals'),
    ('billing.manage',  'View and change billing details and plan'),

    -- projects
    ('project.create',  'Create new projects'),
    ('project.read',    'View a project and its pipelines'),
    ('project.manage',  'Rename, archive, and configure a project'),

    -- pipelines
    ('pipeline.read',   'View a pipeline definition'),
    ('pipeline.write',  'Create and edit pipeline definitions'),
    ('pipeline.run',    'Trigger a pipeline run'),

    -- runs
    ('run.read',        'View runs, their steps, and their status'),
    ('run.cancel',      'Cancel an in-flight run'),
    ('log.read',        'Read step logs'),

    -- secrets
    ('secret.write',    'Create and rotate secrets'),
    -- Separate from secret.write, and rarely granted to humans: writing a secret
    -- does not require the ability to read existing ones back out.
    ('secret.read',     'Reveal secret values'),

    -- cache
    ('cache.read',      'Read cached objects and action results'),
    ('cache.purge',     'Invalidate or delete cache entries'),

    -- infrastructure
    ('worker.read',     'View workers and their status'),
    ('worker.admin',    'Register, drain, and remove workers'),
    ('token.manage',    'Create and revoke API tokens and service accounts'),

    -- compliance
    ('audit.read',      'Read the organization audit log');
