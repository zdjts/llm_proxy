//! Budget system — Org→Team→Key three-level spend tracking (v4.0 Track I).
//!
//! Implements T179 (inheritance), T180 (spend accumulation), T181 (over-budget
//! actions). See `docs/BRIEF-v4.0-config-platform-upgrade.md` §4 and
//! `migrations/0016_budget_system.sql`.
//!
//! # Architecture
//!
//! - Budget limits are stored in `organization.budget_usd`, `team.budget_usd`,
//!   and `client_key` entries (via auth_store).
//! - Spend is accumulated in `budget_usage` table, updated atomically after
//!   each request via a SQL UPSERT.
//! - Budget checks happen in middleware: first QuotaTracker (token/request
//!   counts), then BudgetManager (USD spend).

use sqlx::SqlitePool;

use crate::error::AppError;

// ── Budget action enum ────────────────────────────────────────────────────

/// What to do when a budget is exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetAction {
    /// Reject the request with HTTP 429.
    HardStop,
    /// Automatically switch to the cheapest fallback model.
    SoftDowngrade,
    /// Log a warning but allow the request.
    AlertOnly,
}

impl BudgetAction {
    pub fn parse(s: &str) -> Self {
        match s {
            "soft_downgrade" => BudgetAction::SoftDowngrade,
            "alert_only" => BudgetAction::AlertOnly,
            _ => BudgetAction::HardStop,
        }
    }
}

// ── Budget period ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetPeriod {
    Daily,
    Weekly,
    Monthly,
    Unlimited,
}

impl BudgetPeriod {
    pub fn parse(s: &str) -> Self {
        match s {
            "daily" => BudgetPeriod::Daily,
            "weekly" => BudgetPeriod::Weekly,
            "unlimited" => BudgetPeriod::Unlimited,
            _ => BudgetPeriod::Monthly,
        }
    }

    /// Return the start of the current period as unix timestamp (milliseconds).
    pub fn current_period_start(&self) -> i64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        // Simple truncation to period boundary
        let ms_per_day: i64 = 86_400_000;
        match self {
            BudgetPeriod::Daily => now - (now % ms_per_day),
            BudgetPeriod::Weekly => now - (now % (7 * ms_per_day)),
            BudgetPeriod::Monthly => {
                // Approximate: 30-day months
                now - (now % (30 * ms_per_day))
            }
            BudgetPeriod::Unlimited => 0,
        }
    }

    /// Return the end of the current period as unix timestamp (milliseconds).
    pub fn current_period_end(&self) -> i64 {
        let ms_per_day: i64 = 86_400_000;
        match self {
            BudgetPeriod::Daily => self.current_period_start() + ms_per_day,
            BudgetPeriod::Weekly => self.current_period_start() + 7 * ms_per_day,
            BudgetPeriod::Monthly => self.current_period_start() + 30 * ms_per_day,
            BudgetPeriod::Unlimited => i64::MAX,
        }
    }
}

// ── Budget check result ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BudgetCheckResult {
    /// Whether the request is allowed.
    pub allowed: bool,
    /// Which scope triggered the denial (if any).
    pub denied_by: Option<String>,
    /// Current spend in USD.
    pub current_spend: f64,
    /// Budget limit in USD (None = unlimited).
    pub budget_limit: Option<f64>,
    /// Recommended action on over-budget.
    pub action: BudgetAction,
}

// ── Budget manager ────────────────────────────────────────────────────────

pub struct BudgetManager {
    db: SqlitePool,
}

impl BudgetManager {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Check whether a request is within budget for the given tenant.
    ///
    /// Checks at the team level first, then organization. Returns
    /// `BudgetCheckResult` with the appropriate action.
    pub async fn check_budget(
        &self,
        tenant_id: &str,
        cost_usd: f64,
    ) -> Result<BudgetCheckResult, AppError> {
        // Look up team budget via tenant_id (which maps to team_id per T182)
        let team_budget = self.get_team_budget(tenant_id).await?;

        if let Some((budget_usd, period, action)) = team_budget {
            let current_spend = self.get_current_spend("team", tenant_id, &period).await?;
            let projected = current_spend + cost_usd;

            if projected > budget_usd {
                return Ok(BudgetCheckResult {
                    allowed: false,
                    denied_by: Some(format!("team:{tenant_id}")),
                    current_spend,
                    budget_limit: Some(budget_usd),
                    action,
                });
            }
        }

        // Look up organization budget
        let org_id = self.get_org_for_team(tenant_id).await?;
        if let Some(ref org_id) = org_id {
            let org_budget = self.get_org_budget(org_id).await?;
            if let Some((budget_usd, period, action)) = org_budget {
                let current_spend = self
                    .get_current_spend("organization", org_id, &period)
                    .await?;
                let projected = current_spend + cost_usd;

                if projected > budget_usd {
                    return Ok(BudgetCheckResult {
                        allowed: false,
                        denied_by: Some(format!("org:{org_id}")),
                        current_spend,
                        budget_limit: Some(budget_usd),
                        action,
                    });
                }
            }
        }

        Ok(BudgetCheckResult {
            allowed: true,
            denied_by: None,
            current_spend: 0.0,
            budget_limit: None,
            action: BudgetAction::HardStop,
        })
    }

    /// Record spend after a successful request (T180).
    pub async fn record_spend(&self, tenant_id: &str, cost_usd: f64) -> Result<(), AppError> {
        if cost_usd <= 0.0 {
            return Ok(());
        }

        let period = BudgetPeriod::Monthly;
        let period_start = period.current_period_start();
        let period_end = period.current_period_end();

        // Upsert spend for team
        sqlx::query(
            "INSERT INTO budget_usage (scope_type, scope_id, period_start, period_end, spend_usd, request_count, last_updated) \
             VALUES ('team', ?1, ?2, ?3, ?4, 1, ?5) \
             ON CONFLICT(scope_type, scope_id, period_start) \
             DO UPDATE SET spend_usd = spend_usd + ?4, request_count = request_count + 1, last_updated = ?5",
        )
        .bind(tenant_id)
        .bind(period_start)
        .bind(period_end)
        .bind(cost_usd)
        .bind(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
        )
        .execute(&self.db)
        .await
        .map_err(|e| AppError::Internal(format!("budget record spend: {e}")))?;

        // Also accumulate for organization
        if let Ok(Some(org_id)) = self.get_org_for_team(tenant_id).await {
            let _ = sqlx::query(
                "INSERT INTO budget_usage (scope_type, scope_id, period_start, period_end, spend_usd, request_count, last_updated) \
                 VALUES ('organization', ?1, ?2, ?3, ?4, 1, ?5) \
                 ON CONFLICT(scope_type, scope_id, period_start) \
                 DO UPDATE SET spend_usd = spend_usd + ?4, request_count = request_count + 1, last_updated = ?5",
            )
            .bind(&org_id)
            .bind(period_start)
            .bind(period_end)
            .bind(cost_usd)
            .bind(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64,
            )
            .execute(&self.db)
            .await;
        }

        Ok(())
    }

    /// Validate that a child budget doesn't exceed the parent's remaining
    /// budget (T179).
    pub async fn validate_budget_inheritance(
        &self,
        parent_scope_type: &str,
        parent_scope_id: &str,
        proposed_child_budget: Option<f64>,
    ) -> Result<(), AppError> {
        let proposed = match proposed_child_budget {
            Some(v) => v,
            None => return Ok(()), // unlimited child is always allowed
        };

        let period = BudgetPeriod::Monthly;
        let parent_spend = self
            .get_current_spend(parent_scope_type, parent_scope_id, &period)
            .await?;

        // Get parent budget limit
        let parent_limit = match parent_scope_type {
            "organization" => self.get_org_budget(parent_scope_id).await?,
            "team" => self.get_team_budget(parent_scope_id).await?,
            _ => None,
        };

        if let Some((limit, _, _)) = parent_limit {
            let parent_remaining = limit - parent_spend;
            if proposed > parent_remaining && parent_remaining > 0.0 {
                return Err(AppError::Config(format!(
                    "proposed budget ${proposed:.2} exceeds parent remaining ${parent_remaining:.2}"
                )));
            }
            if proposed > limit {
                return Err(AppError::Config(format!(
                    "proposed budget ${proposed:.2} exceeds parent limit ${limit:.2}"
                )));
            }
        }

        Ok(())
    }

    // ── Private helpers ───────────────────────────────────────────────────

    async fn get_team_budget(
        &self,
        team_id: &str,
    ) -> Result<Option<(f64, BudgetPeriod, BudgetAction)>, AppError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            budget_usd: Option<f64>,
            budget_period: String,
            budget_action: String,
        }

        let row: Option<Row> = sqlx::query_as(
            "SELECT budget_usd, budget_period, budget_action FROM team WHERE id = ?1",
        )
        .bind(team_id)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| AppError::Internal(format!("budget query team: {e}")))?;

        Ok(row.and_then(|r| {
            r.budget_usd.map(|budget| {
                (
                    budget,
                    BudgetPeriod::parse(&r.budget_period),
                    BudgetAction::parse(&r.budget_action),
                )
            })
        }))
    }

    async fn get_org_budget(
        &self,
        org_id: &str,
    ) -> Result<Option<(f64, BudgetPeriod, BudgetAction)>, AppError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            budget_usd: Option<f64>,
            budget_period: String,
        }

        let row: Option<Row> = sqlx::query_as(
            "SELECT budget_usd, budget_period FROM organization WHERE id = ?1 AND enabled = 1",
        )
        .bind(org_id)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| AppError::Internal(format!("budget query org: {e}")))?;

        Ok(row.and_then(|r| {
            r.budget_usd.map(|budget| {
                (
                    budget,
                    BudgetPeriod::parse(&r.budget_period),
                    BudgetAction::HardStop, // org-level always hard stop
                )
            })
        }))
    }

    async fn get_org_for_team(&self, team_id: &str) -> Result<Option<String>, AppError> {
        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT organization_id FROM team WHERE id = ?1")
                .bind(team_id)
                .fetch_optional(&self.db)
                .await
                .map_err(|e| AppError::Internal(format!("budget query org_for_team: {e}")))?;

        Ok(row.and_then(|r| r.0))
    }

    async fn get_current_spend(
        &self,
        scope_type: &str,
        scope_id: &str,
        period: &BudgetPeriod,
    ) -> Result<f64, AppError> {
        let period_start = period.current_period_start();

        let row: Option<(f64,)> = sqlx::query_as(
            "SELECT spend_usd FROM budget_usage \
             WHERE scope_type = ?1 AND scope_id = ?2 AND period_start = ?3",
        )
        .bind(scope_type)
        .bind(scope_id)
        .bind(period_start)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| AppError::Internal(format!("budget query spend: {e}")))?;

        Ok(row.map(|r| r.0).unwrap_or(0.0))
    }

    /// Get current spend for a tenant (team-level), used by admin API.
    pub async fn get_tenant_spend(&self, tenant_id: &str) -> Result<f64, AppError> {
        let period = BudgetPeriod::Monthly;
        self.get_current_spend("team", tenant_id, &period).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup_test_db() -> (SqlitePool, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test_budget.db");
        let path_str = db_path.to_str().unwrap();

        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(path_str)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

        let pool = SqlitePool::connect_with(options).await.unwrap();

        let migrator = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
            .await
            .unwrap();
        migrator.run(&pool).await.unwrap();

        // Seed test data
        sqlx::query("INSERT INTO organization (id, name, budget_usd, budget_period) VALUES ('test-org', 'Test Org', 100.0, 'monthly')")
            .execute(&pool).await.unwrap();

        // The team table exists from migration 0009
        sqlx::query("INSERT INTO team (id, name, slug, organization_id, budget_usd, budget_period, budget_action) VALUES ('test-team', 'Test Team', 'test-team', 'test-org', 50.0, 'monthly', 'hard_stop')")
            .execute(&pool).await.unwrap();

        (pool, dir)
    }

    #[tokio::test]
    async fn it_allows_request_under_budget() {
        let (pool, _dir) = setup_test_db().await;
        let mgr = BudgetManager::new(pool);

        let result = mgr.check_budget("test-team", 10.0).await.unwrap();
        assert!(result.allowed);
    }

    #[tokio::test]
    async fn it_rejects_request_over_team_budget() {
        let (pool, _dir) = setup_test_db().await;
        let mgr = BudgetManager::new(pool);

        // First, record 45.0 of spend
        mgr.record_spend("test-team", 45.0).await.unwrap();

        // Next request of 10.0 would push to 55.0 > 50.0 budget
        let result = mgr.check_budget("test-team", 10.0).await.unwrap();
        assert!(!result.allowed);
        assert!(result.denied_by.unwrap().contains("test-team"));
        assert_eq!(result.action, BudgetAction::HardStop);
    }

    #[tokio::test]
    async fn it_rejects_request_over_org_budget() {
        let (pool, _dir) = setup_test_db().await;
        let mgr = BudgetManager::new(pool.clone());

        // Record 95.0 at org level (simulating other teams' spend)
        let period_start = BudgetPeriod::Monthly.current_period_start();
        let period_end = BudgetPeriod::Monthly.current_period_end();
        sqlx::query(
            "INSERT INTO budget_usage (scope_type, scope_id, period_start, period_end, spend_usd, request_count) \
             VALUES ('organization', 'test-org', ?1, ?2, 95.0, 100)",
        )
        .bind(period_start)
        .bind(period_end)
        .execute(&pool)
        .await
        .unwrap();

        let result = mgr.check_budget("test-team", 10.0).await.unwrap();
        assert!(!result.allowed);
        assert!(result.denied_by.unwrap().contains("test-org"));
    }

    #[tokio::test]
    async fn it_accumulates_spend_correctly() {
        let (pool, _dir) = setup_test_db().await;
        let mgr = BudgetManager::new(pool);

        mgr.record_spend("test-team", 1.5).await.unwrap();
        mgr.record_spend("test-team", 2.5).await.unwrap();

        let spend = mgr.get_tenant_spend("test-team").await.unwrap();
        // Allow small floating-point tolerance
        assert!((spend - 4.0).abs() < 0.01, "expected ~4.0, got {spend}");
    }

    #[tokio::test]
    async fn it_validates_child_budget_not_exceeding_parent() {
        let (pool, _dir) = setup_test_db().await;
        let mgr = BudgetManager::new(pool);

        // Child proposes 60.0 but parent org has 100.0 limit (with 0 spend) → ok
        let result = mgr
            .validate_budget_inheritance("organization", "test-org", Some(60.0))
            .await;
        assert!(
            result.is_ok(),
            "child budget within parent limit should be allowed"
        );

        // Child proposes 150.0 but parent org has 100.0 limit → rejected
        let result = mgr
            .validate_budget_inheritance("organization", "test-org", Some(150.0))
            .await;
        assert!(
            result.is_err(),
            "child budget exceeding parent limit should be rejected"
        );
    }
}
