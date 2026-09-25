//! Which coins to collect and which UTC days are still inside Hyperliquid's
//! candle reach. Pure, so the rules are tested without a network.

use std::collections::BTreeSet;
use std::ops::RangeInclusive;

use serde_json::Value;

pub const DAY_MS: i64 = 86_400_000;
const MINUTE_MS: i64 = 60_000;

/// Hyperliquid serves the latest 5000 candles of an interval. The reach used
/// here stops 100 minutes short of that, so a day at its edge is not cut by
/// the time the request lands.
const REACH_MINUTES: i64 = 4_900;

/// The complete UTC days (as days since the epoch) whose every minute is still
/// served at `now_ms`: from the first midnight inside the reach to yesterday.
/// Empty when the reach does not span a whole day.
pub fn reachable_days(now_ms: i64) -> RangeInclusive<i64> {
    let earliest = now_ms - REACH_MINUTES * MINUTE_MS;
    let first = (earliest + DAY_MS - 1).div_euclid(DAY_MS);
    let last = now_ms.div_euclid(DAY_MS) - 1;
    first..=last
}

/// `YYYY-MM-DD` of a day since the epoch (proleptic Gregorian, UTC).
pub fn ymd(day: i64) -> String {
    // Howard Hinnant's days_from_civil inverse.
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Every coin `live.approved_coins` lists, on either side or as a bare list.
/// A name that is not plain ASCII alphanumerics is left out: it becomes part
/// of an S3 key and a Hyperliquid query, and no perp coin is spelled so.
pub fn approved_coins(config: &Value) -> BTreeSet<String> {
    let Some(approved) = config.get("live").and_then(|l| l.get("approved_coins")) else {
        return BTreeSet::new();
    };
    let lists: Vec<&Value> = match approved {
        Value::Object(sides) => sides.values().collect(),
        list => vec![list],
    };
    lists
        .into_iter()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(Value::as_str)
        .filter(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_alphanumeric()))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const H: i64 = 3_600_000;

    #[test]
    fn a_run_just_after_midnight_reaches_three_whole_days() {
        // 2026-09-25 00:15 UTC: the reach starts 2026-09-21 14:35, so the
        // 21st is cut and the 22nd..24th are whole.
        let now = 20_721 * DAY_MS + 15 * 60_000;
        assert_eq!(ymd(20_721), "2026-09-25");
        assert_eq!(reachable_days(now), 20_718..=20_720);
    }

    #[test]
    fn a_later_run_reaches_one_day_fewer() {
        let now = 20_721 * DAY_MS + 12 * H;
        assert_eq!(reachable_days(now), 20_719..=20_720);
    }

    #[test]
    fn a_day_starting_exactly_at_the_reach_is_whole() {
        let now = 20_718 * DAY_MS + REACH_MINUTES * MINUTE_MS;
        assert_eq!(*reachable_days(now).start(), 20_718);
    }

    #[test]
    fn dates_format_across_leap_years_and_the_epoch() {
        assert_eq!(ymd(0), "1970-01-01");
        assert_eq!(ymd(11_016), "2000-02-29");
        assert_eq!(ymd(20_513), "2026-03-01");
        assert_eq!(ymd(-1), "1969-12-31");
    }

    #[test]
    fn coins_come_from_both_sides_and_from_a_bare_list() {
        let sided =
            json!({"live": {"approved_coins": {"long": ["BTC", "ETH"], "short": ["ETH", "SOL"]}}});
        assert_eq!(
            approved_coins(&sided).into_iter().collect::<Vec<_>>(),
            ["BTC", "ETH", "SOL"]
        );
        let bare = json!({"live": {"approved_coins": ["kPEPE"]}});
        assert_eq!(
            approved_coins(&bare).into_iter().collect::<Vec<_>>(),
            ["kPEPE"]
        );
    }

    #[test]
    fn a_name_that_cannot_be_a_key_is_left_out() {
        let config =
            json!({"live": {"approved_coins": {"long": ["BTC", "../x", "BTC/USDC:USDC", "", 3]}}});
        assert_eq!(
            approved_coins(&config).into_iter().collect::<Vec<_>>(),
            ["BTC"]
        );
        assert!(approved_coins(&json!({})).is_empty());
    }
}
