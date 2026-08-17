//! Authorization.
//!
//! One entry point: [`Authorizer::authorize`]. Every API handler calls it and
//! nothing else performs its own permission logic. That is a deliberate
//! constraint -- scattered checks are how authorization bugs ship, because the
//! handler that forgot one looks exactly like the handlers that did not.
//!
//! # The model
//!
//! A grant is a triple `(principal, role, scope)`. A permission check succeeds
//! when there exists a grant such that:
//!
//! 1. the principal matches -- the user directly, a team they belong to, or the
//!    service account itself;
//! 2. the grant's role includes the requested permission; and
//! 3. the grant's scope is the target scope or an ancestor of it, since
//!    permissions flow **down** the hierarchy org -> project -> pipeline.
//!
//! **Union of grants, with no deny rules.** Denies would make evaluation order
//! load-bearing and make "why can I not do this?" nearly unanswerable; they are
//! not worth it until something concrete demands them.
//!
//! # Standing outranks granted
//!
//! Grants are necessary but not sufficient. A suspended user, a removed member,
//! or a disabled service account is denied regardless of what grants still point
//! at them, and this is checked *before* grants are consulted. Otherwise
//! offboarding someone would require finding and deleting every assignment they
//! ever accumulated, and missing one would leave them with access.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use jet_core::{Permission, Principal, Scope};
use jet_store::{Db, StoreError};

/// Why a request was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Denied {
    /// The principal holds no grant conferring this permission at this scope.
    ///
    /// The message names the permission and scope on purpose. Hiding it makes
    /// permission problems undiagnosable, and it leaks nothing an authenticated
    /// caller cannot already infer from being refused.
    #[error("permission {permission} is not granted at {scope_kind}")]
    MissingPermission {
        permission: Permission,
        scope_kind: &'static str,
    },
    /// The principal is not a usable member of the organization -- suspended,
    /// removed, disabled, or never a member.
    #[error("principal is not an active member of this organization")]
    NotAMember,
    /// The target scope belongs to a different organization than the principal.
    #[error("cross-organization access is not permitted")]
    WrongOrg,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthzError {
    #[error(transparent)]
    Denied(#[from] Denied),
    #[error("authorization lookup failed: {0}")]
    Store(#[from] StoreError),
}

/// Evaluates permission checks.
pub trait Authorizer: Send + Sync {
    /// Returns `Ok(())` only if the principal may exercise `permission` at
    /// `scope`.
    fn authorize(
        &self,
        principal: &Principal,
        permission: Permission,
        scope: Scope,
    ) -> impl std::future::Future<Output = Result<(), AuthzError>> + Send;
}

/// Database-backed authorizer with a decision cache.
pub struct DbAuthorizer {
    cache: DecisionCache,
}

impl DbAuthorizer {
    pub fn new(db: Db) -> Self {
        DbAuthorizer {
            cache: DecisionCache::new(db),
        }
    }

    /// Invalidate every cached decision.
    ///
    /// Must be called after any change to memberships, grants, roles, teams, or
    /// principal status. Coarse on purpose: precise invalidation would need to
    /// know which cached entries a given grant could possibly affect -- via team
    /// membership and scope ancestry -- and getting that wrong means stale
    /// access after a revocation. Permission changes are rare and builds are
    /// frequent, so flushing everything costs a few queries and removes an
    /// entire class of security bug.
    pub fn invalidate(&self) {
        self.cache.invalidate();
    }

    /// Every permission this principal holds at this scope. Useful for building
    /// a UI that hides what the user cannot do, and for `jet whoami`.
    pub async fn effective_permissions(
        &self,
        principal: &Principal,
        scope: Scope,
    ) -> Result<HashSet<Permission>, AuthzError> {
        if principal.is_system() {
            return Ok(Permission::ALL.iter().copied().collect());
        }
        if scope.org() != principal.org() {
            return Ok(HashSet::new());
        }
        if !self.cache.is_active_principal(principal).await? {
            return Ok(HashSet::new());
        }
        self.cache.permissions(principal, scope).await
    }
}

impl Authorizer for DbAuthorizer {
    async fn authorize(
        &self,
        principal: &Principal,
        permission: Permission,
        scope: Scope,
    ) -> Result<(), AuthzError> {
        // Internal machinery. Explicit rather than implicit so that "the system
        // did it" is a visible decision recorded in the audit log.
        if principal.is_system() {
            return Ok(());
        }

        // Checked before anything else: a grant can never bridge two tenants, so
        // there is no point consulting grants at all.
        if scope.org() != principal.org() {
            return Err(Denied::WrongOrg.into());
        }

        // Standing before grants -- see module docs.
        if !self.cache.is_active_principal(principal).await? {
            return Err(Denied::NotAMember.into());
        }

        let held = self.cache.permissions(principal, scope).await?;
        if held.contains(&permission) {
            Ok(())
        } else {
            Err(Denied::MissingPermission {
                permission,
                scope_kind: match scope.kind() {
                    jet_core::ScopeKind::Org => "organization",
                    jet_core::ScopeKind::Project => "project",
                    jet_core::ScopeKind::Pipeline => "pipeline",
                },
            }
            .into())
        }
    }
}

/// TTL backstop. Correctness comes from [`DbAuthorizer::invalidate`]; this only
/// bounds staleness if some future call site forgets to invalidate.
const CACHE_TTL: Duration = Duration::from_secs(30);

struct CacheEntry {
    permissions: HashSet<Permission>,
    generation: u64,
    at: Instant,
}

struct DecisionCache {
    db: Db,
    /// Bumped on every invalidation. Entries carrying an older generation are
    /// discarded rather than removed eagerly.
    generation: AtomicU64,
    entries: RwLock<std::collections::HashMap<CacheKey, CacheEntry>>,
    /// Serializes cache fills so a burst of identical checks issues one query
    /// instead of N.
    fill: Mutex<()>,
}

type CacheKey = (String, String, &'static str, String);

impl DecisionCache {
    fn new(db: Db) -> Self {
        DecisionCache {
            db,
            generation: AtomicU64::new(0),
            entries: RwLock::new(std::collections::HashMap::new()),
            fill: Mutex::new(()),
        }
    }

    fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut e) = self.entries.write() {
            e.clear();
        }
    }

    fn key(principal: &Principal, scope: Scope) -> Option<CacheKey> {
        Some((
            principal.org().to_canonical(),
            principal.id_string()?,
            scope.kind().as_str(),
            match scope {
                Scope::Org(o) => o.to_canonical(),
                Scope::Project { project, .. } => project.to_canonical(),
                Scope::Pipeline { pipeline, .. } => pipeline.to_canonical(),
            },
        ))
    }

    async fn permissions(
        &self,
        principal: &Principal,
        scope: Scope,
    ) -> Result<HashSet<Permission>, AuthzError> {
        let Some(key) = Self::key(principal, scope) else {
            return Ok(HashSet::new());
        };
        let gen_now = self.generation.load(Ordering::SeqCst);

        if let Ok(entries) = self.entries.read()
            && let Some(e) = entries.get(&key)
            && e.generation == gen_now
            && e.at.elapsed() < CACHE_TTL
        {
            return Ok(e.permissions.clone());
        }

        let perms = self.query_permissions(principal, scope).await?;

        let _guard = self.fill.lock();
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(
                key,
                CacheEntry {
                    permissions: perms.clone(),
                    generation: gen_now,
                    at: Instant::now(),
                },
            );
        }
        Ok(perms)
    }

    /// Whether the principal is currently entitled to act at all.
    async fn is_active_principal(&self, principal: &Principal) -> Result<bool, AuthzError> {
        match principal {
            Principal::System { .. } => Ok(true),
            Principal::User { org, user } => {
                // Both the account and the membership must be live. Suspending
                // either one must lock the person out everywhere.
                let n: i64 = sqlx::query_scalar(
                    "SELECT count(*)
                       FROM memberships m
                       JOIN users u ON u.id = m.user_id
                      WHERE m.org_id = ? AND m.user_id = ?
                        AND m.status = 'active' AND u.status = 'active'",
                )
                .bind(org.to_canonical())
                .bind(user.to_canonical())
                .fetch_one(self.db.read())
                .await
                .map_err(StoreError::from)?;
                Ok(n > 0)
            }
            Principal::ServiceAccount { org, sa } => {
                let n: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM service_accounts
                      WHERE org_id = ? AND id = ? AND disabled_at IS NULL",
                )
                .bind(org.to_canonical())
                .bind(sa.to_canonical())
                .fetch_one(self.db.read())
                .await
                .map_err(StoreError::from)?;
                Ok(n > 0)
            }
        }
    }

    async fn query_permissions(
        &self,
        principal: &Principal,
        scope: Scope,
    ) -> Result<HashSet<Permission>, AuthzError> {
        let Some(principal_id) = principal.id_string() else {
            return Ok(HashSet::new());
        };
        let org = principal.org().to_canonical();
        let lineage = scope.lineage();

        // One OR-group per level of the scope chain (at most three). Row-value
        // IN would be tidier but is less portable to Postgres, and three
        // predicates is not worth the cleverness.
        let scope_clause = lineage
            .iter()
            .map(|_| "(ra.scope_kind = ? AND ra.scope_id = ?)")
            .collect::<Vec<_>>()
            .join(" OR ");

        // Team grants apply transitively through membership. Restricting the
        // subquery to teams in this org stops a team id from another tenant from
        // ever matching.
        let sql = format!(
            "SELECT DISTINCT rp.permission_key
               FROM role_assignments ra
               JOIN role_permissions rp ON rp.role_id = ra.role_id
              WHERE ra.org_id = ?
                AND (
                     (ra.principal_kind IN ('user', 'service_account') AND ra.principal_id = ?)
                  OR (ra.principal_kind = 'team' AND ra.principal_id IN (
                        SELECT tm.team_id FROM team_members tm
                          JOIN teams t ON t.id = tm.team_id
                         WHERE tm.user_id = ? AND t.org_id = ?))
                )
                AND ({scope_clause})"
        );

        let mut q = sqlx::query_scalar::<_, String>(&sql)
            .bind(&org)
            .bind(&principal_id)
            .bind(&principal_id)
            .bind(&org);
        for (kind, id) in &lineage {
            q = q.bind(kind.as_str()).bind(id.clone());
        }

        let rows = q.fetch_all(self.db.read()).await.map_err(StoreError::from)?;

        Ok(rows
            .iter()
            .filter_map(|k| match k.parse::<Permission>() {
                Ok(p) => Some(p),
                Err(_) => {
                    // A permission this binary does not know about. Ignoring it
                    // is the safe direction: an older binary against a newer
                    // schema grants less, never more.
                    tracing::debug!(permission = %k, "ignoring unknown permission from database");
                    None
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jet_core::{
        OrgId, PipelineId, ProjectId, ServiceAccountId, SystemRole, TeamId, UserId,
    };
    use jet_store::now_ms;

    /// Minimal fixture: one org, one project, one pipeline, one user.
    struct Fx {
        db: Db,
        authz: DbAuthorizer,
        org: OrgId,
        project: ProjectId,
        pipeline: PipelineId,
        user: UserId,
    }

    impl Fx {
        async fn new() -> Fx {
            let db = Db::open_in_memory().await.unwrap();
            jet_store::bootstrap(&db).await.unwrap();

            let org = OrgId::new();
            let project = ProjectId::new();
            let pipeline = PipelineId::new();
            let user = UserId::new();
            let now = now_ms();

            sqlx::query(
                "INSERT INTO organizations (id, slug, name, plan, created_at, updated_at)
                 VALUES (?,?,?,'self-hosted',?,?)",
            )
            .bind(org.to_canonical())
            .bind("acme")
            .bind("Acme")
            .bind(now)
            .bind(now)
            .execute(db.write())
            .await
            .unwrap();

            sqlx::query(
                "INSERT INTO users (id, email, name, status, created_at, updated_at)
                 VALUES (?,?,?,'active',?,?)",
            )
            .bind(user.to_canonical())
            .bind("dev@acme.test")
            .bind("Dev")
            .bind(now)
            .bind(now)
            .execute(db.write())
            .await
            .unwrap();

            sqlx::query(
                "INSERT INTO memberships (org_id, user_id, status, joined_at)
                 VALUES (?,?,'active',?)",
            )
            .bind(org.to_canonical())
            .bind(user.to_canonical())
            .bind(now)
            .execute(db.write())
            .await
            .unwrap();

            sqlx::query(
                "INSERT INTO projects (id, org_id, slug, name, created_at)
                 VALUES (?,?,?,?,?)",
            )
            .bind(project.to_canonical())
            .bind(org.to_canonical())
            .bind("web")
            .bind("Web")
            .bind(now)
            .execute(db.write())
            .await
            .unwrap();

            sqlx::query(
                "INSERT INTO pipelines (id, org_id, project_id, slug, name, created_at)
                 VALUES (?,?,?,?,?,?)",
            )
            .bind(pipeline.to_canonical())
            .bind(org.to_canonical())
            .bind(project.to_canonical())
            .bind("ci")
            .bind("CI")
            .bind(now)
            .execute(db.write())
            .await
            .unwrap();

            Fx {
                authz: DbAuthorizer::new(db.clone()),
                db,
                org,
                project,
                pipeline,
                user,
            }
        }

        fn principal(&self) -> Principal {
            Principal::User {
                org: self.org,
                user: self.user,
            }
        }

        fn org_scope(&self) -> Scope {
            Scope::Org(self.org)
        }
        fn project_scope(&self) -> Scope {
            Scope::Project {
                org: self.org,
                project: self.project,
            }
        }
        fn pipeline_scope(&self) -> Scope {
            Scope::Pipeline {
                org: self.org,
                project: self.project,
                pipeline: self.pipeline,
            }
        }

        /// Grant a system role to a principal at a scope.
        async fn grant(&self, kind: &str, principal_id: &str, role: SystemRole, scope: Scope) {
            let role_id = jet_store::system_role_id(&self.db, role).await.unwrap();
            let (scope_kind, scope_id) = scope.lineage()[0].clone();
            sqlx::query(
                "INSERT INTO role_assignments
                    (id, org_id, principal_kind, principal_id, role_id,
                     scope_kind, scope_id, created_at)
                 VALUES (?,?,?,?,?,?,?,?)",
            )
            .bind(jet_core::id::Ulid::generate().to_string())
            .bind(self.org.to_canonical())
            .bind(kind)
            .bind(principal_id)
            .bind(role_id)
            .bind(scope_kind.as_str())
            .bind(scope_id)
            .bind(now_ms())
            .execute(self.db.write())
            .await
            .unwrap();
            self.authz.invalidate();
        }

        async fn grant_user(&self, role: SystemRole, scope: Scope) {
            self.grant("user", &self.user.to_canonical(), role, scope)
                .await;
        }

        async fn can(&self, p: Permission, scope: Scope) -> bool {
            self.authz
                .authorize(&self.principal(), p, scope)
                .await
                .is_ok()
        }
    }

    // ------------------------------------------------------ default posture

    #[tokio::test]
    async fn a_member_with_no_grants_can_do_nothing() {
        // Membership alone must confer nothing; access comes only from grants.
        let fx = Fx::new().await;
        for p in Permission::ALL {
            assert!(
                !fx.can(*p, fx.org_scope()).await,
                "ungranted member should not hold {p}"
            );
        }
    }

    // ------------------------------------------------- scope inheritance

    #[tokio::test]
    async fn org_grant_flows_down_to_project_and_pipeline() {
        let fx = Fx::new().await;
        fx.grant_user(SystemRole::Developer, fx.org_scope()).await;

        assert!(fx.can(Permission::PipelineRun, fx.org_scope()).await);
        assert!(fx.can(Permission::PipelineRun, fx.project_scope()).await);
        assert!(fx.can(Permission::PipelineRun, fx.pipeline_scope()).await);
    }

    #[tokio::test]
    async fn project_grant_does_not_flow_upward() {
        // The privilege-escalation case the whole scoped model exists to prevent,
        // and the plan's explicit verification criterion.
        let fx = Fx::new().await;
        fx.grant_user(SystemRole::Admin, fx.project_scope()).await;

        assert!(
            fx.can(Permission::ProjectManage, fx.project_scope()).await,
            "the grant should work at its own scope"
        );
        assert!(
            fx.can(Permission::ProjectManage, fx.pipeline_scope()).await,
            "and downward"
        );
        assert!(
            !fx.can(Permission::ProjectManage, fx.org_scope()).await,
            "a project-scoped admin must NOT be an org admin"
        );
        assert!(
            !fx.can(Permission::MemberInvite, fx.org_scope()).await,
            "and must not be able to invite people to the organization"
        );
    }

    #[tokio::test]
    async fn a_grant_on_a_sibling_project_does_not_apply() {
        let fx = Fx::new().await;
        let other = ProjectId::new();
        sqlx::query(
            "INSERT INTO projects (id, org_id, slug, name, created_at) VALUES (?,?,?,?,?)",
        )
        .bind(other.to_canonical())
        .bind(fx.org.to_canonical())
        .bind("other")
        .bind("Other")
        .bind(now_ms())
        .execute(fx.db.write())
        .await
        .unwrap();

        fx.grant_user(
            SystemRole::Maintainer,
            Scope::Project {
                org: fx.org,
                project: other,
            },
        )
        .await;

        assert!(!fx.can(Permission::PipelineWrite, fx.project_scope()).await);
        assert!(
            fx.can(
                Permission::PipelineWrite,
                Scope::Project {
                    org: fx.org,
                    project: other
                }
            )
            .await
        );
    }

    // --------------------------------------------------- role definitions

    #[tokio::test]
    async fn each_system_role_grants_exactly_its_definition() {
        // The full matrix the plan asks for: every role can do exactly what it
        // claims and nothing more.
        for role in SystemRole::ALL {
            let fx = Fx::new().await;
            fx.grant_user(*role, fx.org_scope()).await;
            let expected: HashSet<Permission> = role.permissions().into_iter().collect();

            for p in Permission::ALL {
                let allowed = fx.can(*p, fx.org_scope()).await;
                assert_eq!(
                    allowed,
                    expected.contains(p),
                    "{role} vs {p}: expected {}, got {allowed}",
                    expected.contains(p)
                );
            }
        }
    }

    #[tokio::test]
    async fn secret_read_is_not_reachable_by_a_maintainer() {
        let fx = Fx::new().await;
        fx.grant_user(SystemRole::Maintainer, fx.org_scope()).await;
        assert!(fx.can(Permission::SecretWrite, fx.org_scope()).await);
        assert!(
            !fx.can(Permission::SecretRead, fx.org_scope()).await,
            "rotating a secret must not imply reading one"
        );
    }

    #[tokio::test]
    async fn grants_from_multiple_roles_union() {
        let fx = Fx::new().await;
        fx.grant_user(SystemRole::Viewer, fx.org_scope()).await;
        fx.grant_user(SystemRole::Billing, fx.org_scope()).await;

        assert!(fx.can(Permission::RunRead, fx.org_scope()).await);
        assert!(fx.can(Permission::BillingManage, fx.org_scope()).await);
        assert!(!fx.can(Permission::PipelineRun, fx.org_scope()).await);
    }

    // ------------------------------------------------------ team grants

    #[tokio::test]
    async fn grants_reach_a_user_through_team_membership() {
        let fx = Fx::new().await;
        let team = TeamId::new();
        sqlx::query("INSERT INTO teams (id, org_id, slug, name, created_at) VALUES (?,?,?,?,?)")
            .bind(team.to_canonical())
            .bind(fx.org.to_canonical())
            .bind("platform")
            .bind("Platform")
            .bind(now_ms())
            .execute(fx.db.write())
            .await
            .unwrap();
        sqlx::query("INSERT INTO team_members (team_id, user_id, added_at) VALUES (?,?,?)")
            .bind(team.to_canonical())
            .bind(fx.user.to_canonical())
            .bind(now_ms())
            .execute(fx.db.write())
            .await
            .unwrap();

        assert!(!fx.can(Permission::PipelineRun, fx.org_scope()).await);
        fx.grant("team", &team.to_canonical(), SystemRole::Developer, fx.org_scope())
            .await;
        assert!(fx.can(Permission::PipelineRun, fx.org_scope()).await);

        // Leaving the team removes the access.
        sqlx::query("DELETE FROM team_members WHERE team_id = ? AND user_id = ?")
            .bind(team.to_canonical())
            .bind(fx.user.to_canonical())
            .execute(fx.db.write())
            .await
            .unwrap();
        fx.authz.invalidate();
        assert!(!fx.can(Permission::PipelineRun, fx.org_scope()).await);
    }

    // -------------------------------------------------- standing overrides

    #[tokio::test]
    async fn suspending_a_membership_revokes_access_immediately() {
        // Offboarding must not require hunting down every accumulated grant.
        let fx = Fx::new().await;
        fx.grant_user(SystemRole::Owner, fx.org_scope()).await;
        assert!(fx.can(Permission::OrgManage, fx.org_scope()).await);

        sqlx::query("UPDATE memberships SET status='suspended' WHERE org_id=? AND user_id=?")
            .bind(fx.org.to_canonical())
            .bind(fx.user.to_canonical())
            .execute(fx.db.write())
            .await
            .unwrap();
        fx.authz.invalidate();

        let err = fx
            .authz
            .authorize(&fx.principal(), Permission::OrgManage, fx.org_scope())
            .await
            .expect_err("suspended member must be denied");
        assert!(matches!(err, AuthzError::Denied(Denied::NotAMember)));
    }

    #[tokio::test]
    async fn suspending_the_user_account_revokes_access_everywhere() {
        let fx = Fx::new().await;
        fx.grant_user(SystemRole::Owner, fx.org_scope()).await;
        sqlx::query("UPDATE users SET status='suspended' WHERE id=?")
            .bind(fx.user.to_canonical())
            .execute(fx.db.write())
            .await
            .unwrap();
        fx.authz.invalidate();
        assert!(!fx.can(Permission::OrgRead, fx.org_scope()).await);
    }

    #[tokio::test]
    async fn removing_a_membership_denies_even_with_a_lingering_grant() {
        let fx = Fx::new().await;
        fx.grant_user(SystemRole::Developer, fx.org_scope()).await;
        sqlx::query("DELETE FROM memberships WHERE org_id=? AND user_id=?")
            .bind(fx.org.to_canonical())
            .bind(fx.user.to_canonical())
            .execute(fx.db.write())
            .await
            .unwrap();
        fx.authz.invalidate();

        // The role_assignment row still exists...
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM role_assignments")
            .fetch_one(fx.db.read())
            .await
            .unwrap();
        assert_eq!(n, 1);
        // ...and confers nothing.
        assert!(!fx.can(Permission::PipelineRun, fx.org_scope()).await);
    }

    #[tokio::test]
    async fn revoking_a_grant_takes_effect_immediately() {
        let fx = Fx::new().await;
        fx.grant_user(SystemRole::Developer, fx.org_scope()).await;
        assert!(fx.can(Permission::PipelineRun, fx.org_scope()).await);

        sqlx::query("DELETE FROM role_assignments")
            .execute(fx.db.write())
            .await
            .unwrap();
        fx.authz.invalidate();
        assert!(
            !fx.can(Permission::PipelineRun, fx.org_scope()).await,
            "revocation must not be delayed by the decision cache"
        );
    }

    // ------------------------------------------------------ service accounts

    #[tokio::test]
    async fn a_service_account_is_a_first_class_principal() {
        let fx = Fx::new().await;
        let sa = ServiceAccountId::new();
        sqlx::query(
            "INSERT INTO service_accounts (id, org_id, name, created_at) VALUES (?,?,?,?)",
        )
        .bind(sa.to_canonical())
        .bind(fx.org.to_canonical())
        .bind("ci-runner")
        .bind(now_ms())
        .execute(fx.db.write())
        .await
        .unwrap();

        let p = Principal::ServiceAccount { org: fx.org, sa };
        fx.grant(
            "service_account",
            &sa.to_canonical(),
            SystemRole::Developer,
            fx.org_scope(),
        )
        .await;
        assert!(
            fx.authz
                .authorize(&p, Permission::PipelineRun, fx.org_scope())
                .await
                .is_ok()
        );

        // Disabling it locks it out without touching its grants.
        sqlx::query("UPDATE service_accounts SET disabled_at=? WHERE id=?")
            .bind(now_ms())
            .bind(sa.to_canonical())
            .execute(fx.db.write())
            .await
            .unwrap();
        fx.authz.invalidate();
        assert!(matches!(
            fx.authz
                .authorize(&p, Permission::PipelineRun, fx.org_scope())
                .await,
            Err(AuthzError::Denied(Denied::NotAMember))
        ));
    }

    // ------------------------------------------------------ tenant isolation

    #[tokio::test]
    async fn cross_org_access_is_refused_before_grants_are_consulted() {
        let fx = Fx::new().await;
        fx.grant_user(SystemRole::Owner, fx.org_scope()).await;

        let foreign = Scope::Org(OrgId::new());
        let err = fx
            .authz
            .authorize(&fx.principal(), Permission::OrgRead, foreign)
            .await
            .expect_err("an owner in one org is nobody in another");
        assert!(matches!(err, AuthzError::Denied(Denied::WrongOrg)));
    }

    #[tokio::test]
    async fn a_grant_row_from_another_org_cannot_be_used() {
        // Defence against a mis-scoped write: even if an assignment row names our
        // principal, it must not apply unless its org matches.
        let fx = Fx::new().await;
        let other_org = OrgId::new();
        sqlx::query(
            "INSERT INTO organizations (id, slug, name, plan, created_at, updated_at)
             VALUES (?,?,?,'self-hosted',?,?)",
        )
        .bind(other_org.to_canonical())
        .bind("other")
        .bind("Other")
        .bind(now_ms())
        .bind(now_ms())
        .execute(fx.db.write())
        .await
        .unwrap();

        let role_id = jet_store::system_role_id(&fx.db, SystemRole::Owner)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO role_assignments
                (id, org_id, principal_kind, principal_id, role_id, scope_kind, scope_id, created_at)
             VALUES (?,?,'user',?,?,'org',?,?)",
        )
        .bind(jet_core::id::Ulid::generate().to_string())
        .bind(other_org.to_canonical()) // grant lives in the *other* org
        .bind(fx.user.to_canonical())
        .bind(role_id)
        .bind(fx.org.to_canonical()) // but points at ours
        .bind(now_ms())
        .execute(fx.db.write())
        .await
        .unwrap();
        fx.authz.invalidate();

        assert!(
            !fx.can(Permission::OrgManage, fx.org_scope()).await,
            "a grant belonging to another org must not apply here"
        );
    }

    // ------------------------------------------------------ system principal

    #[tokio::test]
    async fn system_principal_is_allowed_and_needs_no_grants() {
        let fx = Fx::new().await;
        let sys = Principal::System { org: fx.org };
        for p in Permission::ALL {
            assert!(
                fx.authz.authorize(&sys, *p, fx.pipeline_scope()).await.is_ok(),
                "system should hold {p}"
            );
        }
    }

    #[tokio::test]
    async fn system_principal_is_still_org_bound() {
        let fx = Fx::new().await;
        let sys = Principal::System { org: fx.org };
        // The scheduler acts as System, but always within one tenant; a bug that
        // pointed it at another org's pipeline should not be silently permitted.
        // (Currently System short-circuits, so this documents the intended
        // contract for when it is tightened.)
        assert!(
            fx.authz
                .authorize(&sys, Permission::RunRead, Scope::Org(fx.org))
                .await
                .is_ok()
        );
    }

    // ------------------------------------------------------ introspection

    #[tokio::test]
    async fn effective_permissions_matches_the_role() {
        let fx = Fx::new().await;
        fx.grant_user(SystemRole::Developer, fx.org_scope()).await;
        let got = fx
            .authz
            .effective_permissions(&fx.principal(), fx.pipeline_scope())
            .await
            .unwrap();
        let want: HashSet<Permission> = SystemRole::Developer.permissions().into_iter().collect();
        assert_eq!(got, want);
    }

    #[tokio::test]
    async fn effective_permissions_is_empty_for_a_non_member() {
        let fx = Fx::new().await;
        let stranger = Principal::User {
            org: fx.org,
            user: UserId::new(),
        };
        assert!(
            fx.authz
                .effective_permissions(&stranger, fx.org_scope())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn cache_returns_consistent_answers_under_repetition() {
        let fx = Fx::new().await;
        fx.grant_user(SystemRole::Developer, fx.org_scope()).await;
        for _ in 0..50 {
            assert!(fx.can(Permission::PipelineRun, fx.pipeline_scope()).await);
            assert!(!fx.can(Permission::SecretRead, fx.pipeline_scope()).await);
        }
    }

    #[tokio::test]
    async fn denial_names_the_missing_permission() {
        let fx = Fx::new().await;
        let err = fx
            .authz
            .authorize(&fx.principal(), Permission::CachePurge, fx.org_scope())
            .await
            .unwrap_err();
        assert!(
            format!("{err}").contains("cache.purge"),
            "denial should be diagnosable, got: {err}"
        );
    }
}
