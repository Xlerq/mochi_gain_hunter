use std::collections::HashMap;

use anyhow::Result;
use tokio::try_join;

use crate::config::AppConfig;
use crate::domain::{LeaderboardEntry, WalletActivity, WalletReport};
use crate::polymarket::PolymarketClient;
use crate::scoring::score_wallet;
use crate::simulation::simulate_copy_trading;

pub struct WalletAnalysis {
    pub report: WalletReport,
    pub activities: Vec<WalletActivity>,
    pub current_marks: HashMap<String, f64>,
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

    let missing_mark_assets = activities
        .iter()
        .filter(|activity| !activity.asset.is_empty())
        .filter(|activity| !current_marks.contains_key(&activity.asset))
        .map(|activity| activity.asset.clone())
        .collect::<Vec<_>>();

    for asset in missing_mark_assets.iter().take(5) {
        if let Some(price) = client.midpoint_price(asset).await? {
            current_marks.insert(asset.clone(), price);
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
