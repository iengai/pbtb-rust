//! The public showcase: the artifact a bot on the operator's showcase is
//! published as, the listing built from those artifacts, and the port that
//! puts a bot on the page or takes it off.
//!
//! The artifact is a type of its own rather than the private series with
//! fields removed: it has no place for realized PnL or a balance, so a money
//! field added to the private series later cannot leak through it. `cap_usdt`
//! is the one balance-derived figure, rounded to a magnitude by the collector
//! that builds it.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::domain::bot::Bot;
use crate::domain::error::DomainError;

/// One day of a public curve: where the index stands, and nothing about money.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublicPoint {
    pub ts: i64,
    pub index: f64,
    pub return_pct: f64,
}

/// A config switch on a public curve, with the capital the bot ran the new
/// config at: the wallet balance at the close of the last day before the
/// switch, rounded to a magnitude.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublicSwitch {
    pub ts: i64,
    pub template_name: String,
    pub cap_usdt: f64,
}

/// A capital reset on a public curve, with the capital the new era started
/// at: the close of the reset day itself, since the day before held the dust
/// the re-funding replaced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublicReset {
    pub ts: i64,
    pub cap_usdt: f64,
}

/// The showcase artifact for one bot. `id` is the opaque public id, not the
/// bot id: bot ids are per-tenant keys two tenants may share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublicBotSeries {
    pub id: String,
    pub name: String,
    pub exchange: String,
    /// The copy-trading page the showcase links to; `None` on a bot shown
    /// without one.
    pub public_url: Option<String>,
    pub generated_at: i64,
    pub current_return_pct: f64,
    pub points: Vec<PublicPoint>,
    pub config_switches: Vec<PublicSwitch>,
    pub capital_resets: Vec<PublicReset>,
}

impl PublicBotSeries {
    /// Take the bot's current name and link. The curve may be a day old; the
    /// name and the link are the row's, so a rename or a cleared link since
    /// the curve was built is never published from the artifact.
    pub fn refresh_from(&mut self, bot: &Bot) {
        self.name = bot.name.clone();
        self.public_url = bot.public_url.clone();
    }
}

/// The showcase listing: every public bot with what a list row needs, and a
/// 30-day sparkline. Written whenever the published set changes, empty when
/// nothing is public, so the page can tell "nothing public" from "never
/// written".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublicIndex {
    pub generated_at: i64,
    pub bots: Vec<PublicIndexBot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublicIndexBot {
    pub id: String,
    pub name: String,
    pub exchange: String,
    pub public_url: Option<String>,
    pub current_return_pct: f64,
    pub spark: Vec<f64>,
}

const SPARK_DAYS: usize = 30;

impl PublicIndex {
    pub fn new(series: &[PublicBotSeries], generated_at: i64) -> Self {
        let bots = series
            .iter()
            .map(|s| PublicIndexBot {
                id: s.id.clone(),
                name: s.name.clone(),
                exchange: s.exchange.clone(),
                public_url: s.public_url.clone(),
                current_return_pct: s.current_return_pct,
                spark: s
                    .points
                    .iter()
                    .rev()
                    .take(SPARK_DAYS)
                    .map(|p| p.return_pct)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect(),
            })
            .collect();
        Self { generated_at, bots }
    }
}

/// `sha256("{user_id}#{bot_id}")`, twelve hex characters: stable for the life
/// of the bot, distinct across tenants, and safe as an S3 key and a URL
/// segment whatever the bot was named.
pub fn public_id(user_id: &str, bot_id: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(&Sha256::digest(format!("{user_id}#{bot_id}"))[..6])
}

/// Whether a bot is published: it is on the showcase, on the operator's
/// account. The same choice on any other account's row is inert.
pub fn is_published(bot: &Bot, operator: bool) -> bool {
    operator && bot.on_showcase()
}

/// What a publish found for the bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Published {
    /// The bot's artifact is on the public prefix.
    Live,
    /// No curve has been collected for the bot yet (no stored keys, or a bot
    /// added since the last run), so
    /// there is nothing to put on the page.
    NoCurveYet,
}

/// Puts a bot on the public showcase page or takes it off, and keeps the
/// listing in step with what is published. The caller has already decided the
/// bot belongs there; this only moves artifacts.
#[async_trait]
pub trait ShowcasePublisher: Send + Sync {
    async fn publish(&self, bot: &Bot) -> Result<Published, DomainError>;
    async fn withdraw(&self, bot: &Bot) -> Result<(), DomainError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::exchange::Exchange;

    const LINK: &str = "https://www.bybit.com/copyTrade/x";

    fn a_public_bot() -> Bot {
        let mut bot = Bot::create(
            "u-1".into(),
            Exchange::Bybit,
            "shown".into(),
            "ak".into(),
            "sk".into(),
            1,
        );
        bot.set_public_url(Some(LINK.into()), 1).unwrap();
        bot
    }

    fn series_with(id: &str, days: usize) -> PublicBotSeries {
        PublicBotSeries {
            id: id.into(),
            name: "old name".into(),
            exchange: "bybit".into(),
            public_url: Some(LINK.into()),
            generated_at: 0,
            current_return_pct: days as f64,
            points: (0..days)
                .map(|d| PublicPoint {
                    ts: d as i64 * 86_400,
                    index: 100.0 + d as f64,
                    return_pct: d as f64,
                })
                .collect(),
            config_switches: vec![PublicSwitch {
                ts: 0,
                template_name: "tpl-a".into(),
                cap_usdt: 700.0,
            }],
            capital_resets: vec![],
        }
    }

    #[test]
    fn public_id_is_twelve_hex_of_user_and_bot() {
        let id = public_id("u-1", "shown");
        assert_eq!(id.len(), 12);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(id, public_id("u-1", "shown"), "stable");
        assert_ne!(id, public_id("u-2", "shown"), "per tenant");
    }

    #[test]
    fn a_bot_is_published_when_it_is_on_the_operators_showcase() {
        let private = Bot::create(
            "u-1".into(),
            Exchange::Bybit,
            "quiet".into(),
            "ak".into(),
            "sk".into(),
            1,
        );
        assert!(!is_published(&private, true));
        assert!(is_published(&a_public_bot(), true));
        assert!(!is_published(&a_public_bot(), false), "not an operator");

        let mut hidden = a_public_bot();
        hidden.set_showcase(false, 2);
        assert!(
            !is_published(&hidden, true),
            "hidden keeps the link, not the page"
        );

        let mut unlinked = Bot::create(
            "u-1".into(),
            Exchange::Bybit,
            "bare".into(),
            "ak".into(),
            "sk".into(),
            1,
        );
        unlinked.set_showcase(true, 2);
        assert!(is_published(&unlinked, true));
    }

    #[test]
    fn the_index_lists_each_artifact_with_its_last_thirty_days_oldest_first() {
        let index = PublicIndex::new(&[series_with("a", 40), series_with("b", 3)], 7);
        assert_eq!(index.generated_at, 7);
        assert_eq!(
            index.bots[0].spark,
            (10..40).map(|d| d as f64).collect::<Vec<_>>()
        );
        assert_eq!(index.bots[1].spark, vec![0.0, 1.0, 2.0]);
        assert_eq!(index.bots[1].id, "b");
    }

    #[test]
    fn refreshing_takes_the_rows_name_and_link_and_keeps_the_curve() {
        let mut series = series_with("a", 3);
        let mut bot = Bot::create(
            "u-1".into(),
            Exchange::Bybit,
            "renamed".into(),
            "ak".into(),
            "sk".into(),
            1,
        );
        bot.set_showcase(true, 2);
        series.refresh_from(&bot);
        assert_eq!(series.name, "renamed");
        assert_eq!(series.public_url, None, "a cleared link is not republished");
        assert_eq!(series.points.len(), 3);
    }

    #[test]
    fn an_artifact_reads_back_as_it_was_written() {
        let series = series_with("a", 2);
        let read: PublicBotSeries =
            serde_json::from_slice(&serde_json::to_vec(&series).unwrap()).unwrap();
        assert_eq!(read, series);
    }
}
