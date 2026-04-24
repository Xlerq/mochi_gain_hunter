use std::collections::{HashMap, HashSet};

use anyhow::Result;
use futures_util::{StreamExt, stream};
use tokio::try_join;

use crate::config::AppConfig;
use crate::domain::{LeaderboardEntry, WalletActivity, WalletReport};
use crate::polymarket::{PolymarketClient, ResolvedWallet};
use crate::scoring::score_wallet;
use crate::simulation::simulate_copy_trading;

const MISSING_MARK_LOOKUP_LIMIT: usize = 5;

#[derive(Debug, Clone)]
pub struct WalletAnalysis {
    pub report: WalletReport,
    pub activities: Vec<WalletActivity>,
    pub current_marks: HashMap<String, f64>,
}

#[derive(Debug, Clone)]
pub struct ResolvedWalletAnalysis {
    pub resolved: ResolvedWallet,
    pub analysis: WalletAnalysis,
}

pub async fn build_resolved_wallet_analysis(
    client: &PolymarketClient,
    config: &AppConfig,
    wallet_input: &str,
) -> Result<ResolvedWalletAnalysis> {
    let resolved = client.resolve_wallet_input(wallet_input).await?;
    let analysis = build_wallet_analysis(client, config, &resolved.wallet, None).await?;
    Ok(ResolvedWalletAnalysis { resolved, analysis })
}

pub async fn build_wallet_analysis(
    client: &PolymarketClient,
    config: &AppConfig,
    wallet: &str,
    leaderboard_entry: Option<LeaderboardEntry>,
) -> Result<WalletAnalysis> {
    let (activities, positions, closed_positions) = try_join!(
        client.user_activity(wallet, config.discover.activity_limit, 0),
        client.current_positions(wallet, config.discover.positions_limit),
        client.closed_positions(wallet, config.discover.closed_positions_limit, 0),
    )?;

    let mut current_marks = HashMap::new();
    for position in &positions {
        if position.cur_price > 0.0 {
            current_marks.insert(position.asset.clone(), position.cur_price);
        }
    }

    let mut seen_missing_assets = HashSet::new();
    let missing_mark_assets = activities
        .iter()
        .filter_map(|activity| {
            if activity.asset.is_empty()
                || current_marks.contains_key(&activity.asset)
                || !seen_missing_assets.insert(activity.asset.as_str())
            {
                return None;
            }
            Some(activity.asset.clone())
        })
        .take(MISSING_MARK_LOOKUP_LIMIT)
        .collect::<Vec<_>>();

    let midpoint_results = stream::iter(missing_mark_assets)
        .map(|asset| async move {
            let price = client.midpoint_price(&asset).await?;
            Ok::<_, anyhow::Error>((asset, price))
        })
        .buffer_unordered(config.http.max_concurrent_requests.max(1))
        .collect::<Vec<_>>()
        .await;

    for result in midpoint_results {
        let (asset, price) = result?;
        if let Some(price) = price {
            current_marks.insert(asset, price);
        }
    }

    let scorecard = score_wallet(
        leaderboard_entry.as_ref(),
        &activities,
        &positions,
        &closed_positions,
        &config.scoring,
    );
    let simulation = simulate_copy_trading(wallet, &activities, &current_marks, &config.simulation);

    Ok(WalletAnalysis {
        report: WalletReport {
            scorecard,
            simulation,
        },
        activities,
        current_marks,
    })
}

pub async fn build_wallet_report(
    client: &PolymarketClient,
    config: &AppConfig,
    wallet: &str,
    leaderboard_entry: Option<LeaderboardEntry>,
) -> Result<WalletReport> {
    Ok(
        build_wallet_analysis(client, config, wallet, leaderboard_entry)
            .await?
            .report,
    )
}
