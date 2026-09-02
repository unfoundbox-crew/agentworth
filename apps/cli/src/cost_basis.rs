//! Honest cost-basis labelling for every dollar figure this CLI prints or serves.
//!
//! Every `estimated_cost_usd` on this machine (`usage`, `--pacing`, `/api/usage`, the MCP
//! `usage_summary` tool) is computed from `agentworth_storage::pricing`'s public API list --
//! never what the account actually paid. A Claude subscriber on a flat-rate plan sees a
//! token-derived dollar figure with no relationship to their bill unless something says so.

use serde::Serialize;

/// `cost_basis` value for every JSON payload that carries an estimated cost.
pub const COST_BASIS_API_LIST_PRICE: &str = "api_list_price_equivalent";

#[derive(Debug, Clone, Serialize)]
pub struct CostBasis {
    pub cost_basis: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription_tier: Option<String>,
}

impl CostBasis {
    /// Best-effort detection from the local Claude Code credentials file. Never fails --
    /// this is a label, not a control path, and any absence just means the tier-less
    /// message below.
    pub fn detect() -> Self {
        Self {
            cost_basis: COST_BASIS_API_LIST_PRICE,
            subscription_tier: read_subscription_tier(),
        }
    }

    /// Full sentence for a human screen's header/footer.
    pub fn label_long(&self) -> String {
        match &self.subscription_tier {
            Some(tier) => format!(
                "cost: API-equivalent at list prices; your account is on {tier}, so this is not what you paid"
            ),
            None => "cost: API-equivalent at list prices".to_string(),
        }
    }

    /// Column-header short form, used wherever a table has a `COST` column.
    pub const fn label_short() -> &'static str {
        "COST (API-eq)"
    }
}

/// Reads `~/.claude.json`'s `oauthAccount.organizationRateLimitTier` (verified shape, e.g.
/// `"default_claude_max_20x"` for a Max subscriber), falling back to `billingType` (e.g.
/// `"stripe_subscription"`) when the rate-limit tier is absent -- an org-scoped account
/// carries the former, an individual subscriber the latter. Absence of the file, of
/// `oauthAccount`, or of both fields is not an error; it just means no tier to report.
fn read_subscription_tier() -> Option<String> {
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    let raw = std::fs::read_to_string(home.join(".claude.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let oauth = v.get("oauthAccount")?;
    oauth
        .get("organizationRateLimitTier")
        .and_then(|x| x.as_str())
        .or_else(|| oauth.get("billingType").and_then(|x| x.as_str()))
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_long_without_tier_never_claims_to_know_a_price() {
        let cb = CostBasis {
            cost_basis: COST_BASIS_API_LIST_PRICE,
            subscription_tier: None,
        };
        assert_eq!(cb.label_long(), "cost: API-equivalent at list prices");
    }

    #[test]
    fn label_long_with_tier_names_it_and_disclaims_the_bill() {
        let cb = CostBasis {
            cost_basis: COST_BASIS_API_LIST_PRICE,
            subscription_tier: Some("default_claude_max_20x".to_string()),
        };
        assert_eq!(
            cb.label_long(),
            "cost: API-equivalent at list prices; your account is on default_claude_max_20x, \
             so this is not what you paid"
        );
    }
}
