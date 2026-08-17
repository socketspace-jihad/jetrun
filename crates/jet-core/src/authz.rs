//! Authorization value types: permissions, principals, scopes, system roles.
//!
//! These are pure values with no I/O, which is why they live here rather than in
//! `jet-authz`: both the authorizer and the store need to name them, and putting
//! them in either one would create a dependency cycle.
//!
//! # Permissions are an enum, not strings
//!
//! Role definitions below are ordinary Rust data, so adding a permission and
//! forgetting to decide which roles get it is a compile error rather than a
//! silent grant of nothing (or, worse, a typo that grants nothing while looking
//! like it grants something).

use std::fmt;
use std::str::FromStr;

use crate::{OrgId, PipelineId, ProjectId, ServiceAccountId, UserId};

/// A single capability.
///
/// Named `resource.action`. A permission answers exactly one question and never
/// implies another: `PipelineWrite` does not confer `PipelineRun`, because
/// editing a definition and executing it are genuinely different privileges and
/// plenty of teams grant only the first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Permission {
    OrgRead,
    OrgManage,
    MemberInvite,
    MemberRemove,
    TeamManage,
    RoleManage,
    BillingManage,

    ProjectCreate,
    ProjectRead,
    ProjectManage,

    PipelineRead,
    PipelineWrite,
    PipelineRun,

    RunRead,
    RunCancel,
    LogRead,

    SecretWrite,
    SecretRead,

    CacheRead,
    CachePurge,

    WorkerRead,
    WorkerAdmin,
    TokenManage,

    AuditRead,
}

impl Permission {
    /// Every permission. Kept exhaustive by [`Permission::from_str`] round-trip
    /// tests, and asserted against the SQL catalog by a test in `jet-store`, so
    /// the enum and the database cannot drift apart.
    pub const ALL: &'static [Permission] = &[
        Permission::OrgRead,
        Permission::OrgManage,
        Permission::MemberInvite,
        Permission::MemberRemove,
        Permission::TeamManage,
        Permission::RoleManage,
        Permission::BillingManage,
        Permission::ProjectCreate,
        Permission::ProjectRead,
        Permission::ProjectManage,
        Permission::PipelineRead,
        Permission::PipelineWrite,
        Permission::PipelineRun,
        Permission::RunRead,
        Permission::RunCancel,
        Permission::LogRead,
        Permission::SecretWrite,
        Permission::SecretRead,
        Permission::CacheRead,
        Permission::CachePurge,
        Permission::WorkerRead,
        Permission::WorkerAdmin,
        Permission::TokenManage,
        Permission::AuditRead,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Permission::OrgRead => "org.read",
            Permission::OrgManage => "org.manage",
            Permission::MemberInvite => "member.invite",
            Permission::MemberRemove => "member.remove",
            Permission::TeamManage => "team.manage",
            Permission::RoleManage => "role.manage",
            Permission::BillingManage => "billing.manage",
            Permission::ProjectCreate => "project.create",
            Permission::ProjectRead => "project.read",
            Permission::ProjectManage => "project.manage",
            Permission::PipelineRead => "pipeline.read",
            Permission::PipelineWrite => "pipeline.write",
            Permission::PipelineRun => "pipeline.run",
            Permission::RunRead => "run.read",
            Permission::RunCancel => "run.cancel",
            Permission::LogRead => "log.read",
            Permission::SecretWrite => "secret.write",
            Permission::SecretRead => "secret.read",
            Permission::CacheRead => "cache.read",
            Permission::CachePurge => "cache.purge",
            Permission::WorkerRead => "worker.read",
            Permission::WorkerAdmin => "worker.admin",
            Permission::TokenManage => "token.manage",
            Permission::AuditRead => "audit.read",
        }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Permission {
    type Err = UnknownPermission;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Permission::ALL
            .iter()
            .copied()
            .find(|p| p.as_str() == s)
            .ok_or_else(|| UnknownPermission(s.to_owned()))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown permission {0:?}")]
pub struct UnknownPermission(pub String);

/// What kind of thing is acting.
///
/// Machine principals are first-class rather than "a user account we pretend is
/// a robot": runners, the CLI in CI, and the k8s controller all need identities
/// that can be scoped and revoked without disabling a human's login.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PrincipalKind {
    User,
    Team,
    ServiceAccount,
}

impl PrincipalKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            PrincipalKind::User => "user",
            PrincipalKind::Team => "team",
            PrincipalKind::ServiceAccount => "service_account",
        }
    }
}

impl fmt::Display for PrincipalKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who is acting, resolved to an identity within one organization.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Principal {
    User { org: OrgId, user: UserId },
    ServiceAccount { org: OrgId, sa: ServiceAccountId },
    /// Internal machinery (the scheduler, migrations, GC). Bypasses grant
    /// lookup; there is no principal to grant *to*. Kept explicit so that
    /// "system did it" is a visible decision in the audit log rather than an
    /// unauthenticated request that happened to work.
    System { org: OrgId },
}

impl Principal {
    pub fn org(&self) -> OrgId {
        match self {
            Principal::User { org, .. }
            | Principal::ServiceAccount { org, .. }
            | Principal::System { org } => *org,
        }
    }

    pub fn kind(&self) -> Option<PrincipalKind> {
        match self {
            Principal::User { .. } => Some(PrincipalKind::User),
            Principal::ServiceAccount { .. } => Some(PrincipalKind::ServiceAccount),
            Principal::System { .. } => None,
        }
    }

    /// Canonical id string, matching `role_assignments.principal_id`.
    pub fn id_string(&self) -> Option<String> {
        match self {
            Principal::User { user, .. } => Some(user.to_canonical()),
            Principal::ServiceAccount { sa, .. } => Some(sa.to_canonical()),
            Principal::System { .. } => None,
        }
    }

    pub fn is_system(&self) -> bool {
        matches!(self, Principal::System { .. })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ScopeKind {
    Org,
    Project,
    Pipeline,
}

impl ScopeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            ScopeKind::Org => "org",
            ScopeKind::Project => "project",
            ScopeKind::Pipeline => "pipeline",
        }
    }
}

impl fmt::Display for ScopeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a permission is being exercised.
///
/// Note that each variant carries its **full ancestor chain**, not just its own
/// id. That is deliberate: authorization then needs no database lookup to
/// discover that a pipeline belongs to a project which belongs to an org, so a
/// check is a pure function of the request. The caller already loaded the
/// pipeline to handle the request at all, so the lineage is free at the point
/// where it is known and expensive anywhere else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    Org(OrgId),
    Project {
        org: OrgId,
        project: ProjectId,
    },
    Pipeline {
        org: OrgId,
        project: ProjectId,
        pipeline: PipelineId,
    },
}

impl Scope {
    pub fn org(&self) -> OrgId {
        match self {
            Scope::Org(org) | Scope::Project { org, .. } | Scope::Pipeline { org, .. } => *org,
        }
    }

    pub fn kind(&self) -> ScopeKind {
        match self {
            Scope::Org(_) => ScopeKind::Org,
            Scope::Project { .. } => ScopeKind::Project,
            Scope::Pipeline { .. } => ScopeKind::Pipeline,
        }
    }

    /// This scope and every scope above it, nearest first.
    ///
    /// A grant matches the target if it sits anywhere in this chain -- which is
    /// exactly what "permissions flow down the hierarchy" means, expressed once
    /// so no call site has to reimplement it.
    pub fn lineage(&self) -> Vec<(ScopeKind, String)> {
        match self {
            Scope::Org(org) => vec![(ScopeKind::Org, org.to_canonical())],
            Scope::Project { org, project } => vec![
                (ScopeKind::Project, project.to_canonical()),
                (ScopeKind::Org, org.to_canonical()),
            ],
            Scope::Pipeline {
                org,
                project,
                pipeline,
            } => vec![
                (ScopeKind::Pipeline, pipeline.to_canonical()),
                (ScopeKind::Project, project.to_canonical()),
                (ScopeKind::Org, org.to_canonical()),
            ],
        }
    }

    /// Whether a grant at `(kind, id)` covers this scope.
    pub fn covered_by(&self, kind: ScopeKind, id: &str) -> bool {
        self.lineage()
            .iter()
            .any(|(k, i)| *k == kind && i.as_str() == id)
    }
}

/// Built-in roles, present in every organization.
///
/// The permission sets are plain data below rather than rows in a fixture, so
/// reviewing a privilege change is reviewing a diff of this file.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SystemRole {
    Owner,
    Admin,
    Maintainer,
    Developer,
    Viewer,
    Billing,
}

impl SystemRole {
    pub const ALL: &'static [SystemRole] = &[
        SystemRole::Owner,
        SystemRole::Admin,
        SystemRole::Maintainer,
        SystemRole::Developer,
        SystemRole::Viewer,
        SystemRole::Billing,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            SystemRole::Owner => "owner",
            SystemRole::Admin => "admin",
            SystemRole::Maintainer => "maintainer",
            SystemRole::Developer => "developer",
            SystemRole::Viewer => "viewer",
            SystemRole::Billing => "billing",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            SystemRole::Owner => "Owner",
            SystemRole::Admin => "Admin",
            SystemRole::Maintainer => "Maintainer",
            SystemRole::Developer => "Developer",
            SystemRole::Viewer => "Viewer",
            SystemRole::Billing => "Billing",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            SystemRole::Owner => "Full control, including billing and secret values",
            SystemRole::Admin => "Full control except billing and revealing secret values",
            SystemRole::Maintainer => "Manage projects, pipelines, secrets and caches",
            SystemRole::Developer => "Run pipelines and read results",
            SystemRole::Viewer => "Read-only access",
            SystemRole::Billing => "Manage billing only",
        }
    }

    /// Permissions this role grants.
    ///
    /// Two decisions worth noting:
    ///
    /// * **`SecretRead` belongs to `Owner` alone.** Secrets are injected into
    ///   steps by the executor; almost nobody needs to read a value back out,
    ///   and `SecretWrite` deliberately does not imply it, so rotating a
    ///   credential is not the same privilege as exfiltrating one.
    /// * **`Admin` is not `Owner`.** It omits billing and secret reads, so the
    ///   day-to-day administrative role is not also the most dangerous one.
    pub fn permissions(self) -> Vec<Permission> {
        use Permission as P;
        match self {
            SystemRole::Owner => P::ALL.to_vec(),
            SystemRole::Admin => P::ALL
                .iter()
                .copied()
                .filter(|p| !matches!(p, P::BillingManage | P::SecretRead))
                .collect(),
            SystemRole::Maintainer => vec![
                P::OrgRead,
                P::ProjectCreate,
                P::ProjectRead,
                P::ProjectManage,
                P::PipelineRead,
                P::PipelineWrite,
                P::PipelineRun,
                P::RunRead,
                P::RunCancel,
                P::LogRead,
                P::SecretWrite,
                P::CacheRead,
                P::CachePurge,
                P::WorkerRead,
            ],
            SystemRole::Developer => vec![
                P::OrgRead,
                P::ProjectRead,
                P::PipelineRead,
                P::PipelineRun,
                P::RunRead,
                P::RunCancel,
                P::LogRead,
                P::CacheRead,
            ],
            SystemRole::Viewer => vec![
                P::OrgRead,
                P::ProjectRead,
                P::PipelineRead,
                P::RunRead,
                P::LogRead,
            ],
            SystemRole::Billing => vec![P::OrgRead, P::BillingManage],
        }
    }
}

impl fmt::Display for SystemRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_strings_roundtrip() {
        for p in Permission::ALL {
            assert_eq!(p.as_str().parse::<Permission>().unwrap(), *p);
        }
        assert!("not.a.permission".parse::<Permission>().is_err());
    }

    #[test]
    fn permission_list_is_complete_and_unique() {
        // ALL is maintained by hand; a forgotten entry would silently make a
        // permission ungrantable.
        let mut seen = std::collections::HashSet::new();
        for p in Permission::ALL {
            assert!(seen.insert(p.as_str()), "duplicate permission {p}");
        }
        assert_eq!(seen.len(), Permission::ALL.len());
        assert_eq!(
            Permission::ALL.len(),
            24,
            "ALL is out of sync with the enum; update the SQL catalog too"
        );
    }

    #[test]
    fn permission_names_are_resource_dot_action() {
        for p in Permission::ALL {
            let s = p.as_str();
            assert_eq!(s.matches('.').count(), 1, "{s} should be resource.action");
            assert_eq!(s, s.to_lowercase(), "{s} should be lowercase");
        }
    }

    // --- scope lineage: the core of "grants flow down" ---

    fn scopes() -> (OrgId, ProjectId, PipelineId) {
        (OrgId::new(), ProjectId::new(), PipelineId::new())
    }

    #[test]
    fn org_grant_covers_everything_below_it() {
        let (org, project, pipeline) = scopes();
        let pipe_scope = Scope::Pipeline {
            org,
            project,
            pipeline,
        };
        assert!(pipe_scope.covered_by(ScopeKind::Org, &org.to_canonical()));
        assert!(pipe_scope.covered_by(ScopeKind::Project, &project.to_canonical()));
        assert!(pipe_scope.covered_by(ScopeKind::Pipeline, &pipeline.to_canonical()));
    }

    #[test]
    fn project_grant_does_not_reach_upward() {
        // The property a flat user_roles table cannot express, and the one most
        // likely to be a privilege-escalation bug if it regresses.
        let (org, project, _) = scopes();
        let org_scope = Scope::Org(org);
        assert!(
            !org_scope.covered_by(ScopeKind::Project, &project.to_canonical()),
            "a project-scoped grant must not confer org-wide access"
        );
    }

    #[test]
    fn sibling_scopes_are_isolated() {
        let (org, project_a, pipeline) = scopes();
        let project_b = ProjectId::new();
        let scope_in_a = Scope::Pipeline {
            org,
            project: project_a,
            pipeline,
        };
        assert!(!scope_in_a.covered_by(ScopeKind::Project, &project_b.to_canonical()));
    }

    #[test]
    fn a_grant_in_another_org_never_matches() {
        let (org, project, pipeline) = scopes();
        let other_org = OrgId::new();
        let scope = Scope::Pipeline {
            org,
            project,
            pipeline,
        };
        assert!(!scope.covered_by(ScopeKind::Org, &other_org.to_canonical()));
    }

    #[test]
    fn lineage_is_ordered_nearest_first() {
        let (org, project, pipeline) = scopes();
        let l = Scope::Pipeline {
            org,
            project,
            pipeline,
        }
        .lineage();
        assert_eq!(
            l.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
            vec![ScopeKind::Pipeline, ScopeKind::Project, ScopeKind::Org]
        );
    }

    #[test]
    fn scope_kind_matters_not_just_the_id() {
        // Guards against comparing ids alone: ULIDs are structurally identical
        // across entity types, so ignoring the kind would let a project id match
        // an org-scoped grant.
        let org = OrgId::new();
        let confusable = ProjectId::from_ulid(org.as_ulid());
        let scope = Scope::Project {
            org,
            project: confusable,
        };
        assert!(scope.covered_by(ScopeKind::Org, &org.to_canonical()));
        // ...but a grant of the same string at the wrong level does not leak
        // upward.
        assert!(!Scope::Org(org).covered_by(ScopeKind::Project, &confusable.to_canonical()));
    }

    // --- role definitions ---

    #[test]
    fn owner_has_everything_and_viewer_is_read_only() {
        assert_eq!(SystemRole::Owner.permissions().len(), Permission::ALL.len());

        for p in SystemRole::Viewer.permissions() {
            assert!(
                p.as_str().ends_with(".read"),
                "viewer should not hold {p}, which is not a read"
            );
        }
    }

    #[test]
    fn secret_read_is_owner_only() {
        // Deliberate posture: writing a secret must not imply exfiltrating one.
        for r in SystemRole::ALL {
            let has = r.permissions().contains(&Permission::SecretRead);
            assert_eq!(
                has,
                *r == SystemRole::Owner,
                "{r} should{} hold secret.read",
                if *r == SystemRole::Owner { "" } else { " not" }
            );
        }
    }

    #[test]
    fn secret_write_does_not_imply_secret_read() {
        let m = SystemRole::Maintainer.permissions();
        assert!(m.contains(&Permission::SecretWrite));
        assert!(!m.contains(&Permission::SecretRead));
    }

    #[test]
    fn billing_is_isolated_from_everything_else() {
        let b = SystemRole::Billing.permissions();
        assert!(b.contains(&Permission::BillingManage));
        assert!(!b.contains(&Permission::PipelineRun));
        assert!(!b.contains(&Permission::SecretRead));

        // And the day-to-day admin role is not the most dangerous one.
        let a = SystemRole::Admin.permissions();
        assert!(!a.contains(&Permission::BillingManage));
        assert!(!a.contains(&Permission::SecretRead));
    }

    #[test]
    fn roles_are_ordered_by_strictly_increasing_power() {
        // Each tier should be a superset of the one below, or the model is not a
        // ladder and "promote to maintainer" could silently remove access.
        let ladder = [
            SystemRole::Viewer,
            SystemRole::Developer,
            SystemRole::Maintainer,
            SystemRole::Admin,
            SystemRole::Owner,
        ];
        for pair in ladder.windows(2) {
            let (lower, higher) = (pair[0].permissions(), pair[1].permissions());
            for p in &lower {
                assert!(
                    higher.contains(p),
                    "{} holds {p} but {} does not",
                    pair[0],
                    pair[1]
                );
            }
        }
    }

    #[test]
    fn role_keys_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for r in SystemRole::ALL {
            assert!(seen.insert(r.key()), "duplicate role key {r}");
        }
    }

    #[test]
    fn system_principal_has_no_grantable_identity() {
        let org = OrgId::new();
        let sys = Principal::System { org };
        assert!(sys.is_system());
        assert_eq!(sys.kind(), None);
        assert_eq!(sys.id_string(), None, "System must not be grantable to");
        assert_eq!(sys.org(), org);
    }
}
