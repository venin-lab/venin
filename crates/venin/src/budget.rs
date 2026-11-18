//! The budget governor. Each period adds a 24th of the daily cap to an allowance and pays the spend its record shows;
//! what is left carries forward, up to one day's cap. Holder strains are cut first; a period is void (`budget`) only
//! when the allowance is gone.

use venin_core::hour::HourRecord;
use venin_core::lab::roster;

/// Periods whose runs set the expected cost of a run.
const COST_WINDOW: usize = 24;

#[derive(Debug, PartialEq, Eq)]
pub enum Allowance {
    /// Nothing left: the period is void (`budget`).
    Gone,
    /// The period runs its base runs and at most this many holder strains.
    Slots(usize),
}

/// The allowance of the next period in micro-USD, from the spend of every earlier period in order from genesis. It
/// goes below zero when runs cost more than it held, and the debt carries forward.
pub fn allowance(daily_cap: u64, spends: impl IntoIterator<Item = u64>) -> i64 {
    let cap = i64::try_from(daily_cap).unwrap_or(i64::MAX);
    let hourly = cap / 24;
    let spend = |s: u64| i64::try_from(s).unwrap_or(i64::MAX);
    let mut balance = 0i64;
    for s in spends {
        balance = balance
            .saturating_add(hourly)
            .min(cap)
            .saturating_sub(spend(s));
    }
    balance.saturating_add(hourly).min(cap)
}

/// What `allowance` buys: the base runs first, then whole holder strains, each `holder_runs` runs, up to `slots`.
pub fn govern(
    allowance: i64,
    per_run: u64,
    base_runs: usize,
    holder_runs: usize,
    slots: usize,
) -> Allowance {
    let Ok(left) = u64::try_from(allowance) else {
        return Allowance::Gone;
    };
    if left == 0 {
        return Allowance::Gone;
    }
    let after_base = left.saturating_sub(per_run.saturating_mul(base_runs as u64));
    let per_strain = per_run.saturating_mul(holder_runs as u64);
    let fit = after_base
        .checked_div(per_strain)
        .map_or(slots, |k| usize::try_from(k).unwrap_or(usize::MAX));
    Allowance::Slots(fit.min(slots))
}

/// The next period's allowance after `records` (every hour from genesis, in order): a run is expected to cost the
/// mean of the published runs of the last 24 hours; a ward of 20 bodies has 60 base runs and a holder strain runs on
/// the 8 Venin bodies.
pub fn plan(daily_cap: u64, records: &[HourRecord], slots: usize) -> Allowance {
    let left = allowance(daily_cap, records.iter().map(|r| r.spend_micro));
    let recent = &records[records.len().saturating_sub(COST_WINDOW)..];
    let runs: u64 = recent.iter().map(|r| r.runs.len() as u64).sum();
    let spent: u64 = recent.iter().map(|r| r.spend_micro).sum();
    let bodies = roster();
    let venin = bodies.iter().filter(|b| b.row.is_venin()).count();
    govern(
        left,
        spent.checked_div(runs).unwrap_or(0),
        bodies.len() * 3,
        venin,
        slots,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_allowance_accrues_carries_forward_up_to_a_day_and_pays_spend() {
        let cap = 2_400_000;
        assert_eq!(allowance(cap, []), 100_000);
        assert_eq!(allowance(cap, [0, 0]), 300_000);
        assert_eq!(allowance(cap, [40_000]), 160_000);
        assert_eq!(allowance(cap, vec![0; 100]), cap as i64, "one day at most");
        assert_eq!(
            allowance(cap, [500_000]),
            -300_000,
            "a debt carries forward"
        );
        assert_eq!(allowance(cap, [500_000, 0, 0, 0]), 0);
        assert!(allowance(cap, [u64::MAX]) < 0);
    }

    #[test]
    fn holder_slots_shrink_before_the_base_runs_go() {
        assert_eq!(govern(100_000, 1_000, 60, 8, 6), Allowance::Slots(5));
        assert_eq!(govern(200_000, 1_000, 60, 8, 6), Allowance::Slots(6));
        assert_eq!(govern(60_000, 1_000, 60, 8, 6), Allowance::Slots(0));
        assert_eq!(
            govern(10, 1_000, 60, 8, 6),
            Allowance::Slots(0),
            "base runs go on while anything is left"
        );
        assert_eq!(govern(0, 1_000, 60, 8, 6), Allowance::Gone);
        assert_eq!(govern(-5, 1_000, 60, 8, 6), Allowance::Gone);
        assert_eq!(govern(1, 0, 60, 8, 6), Allowance::Slots(6), "free models");
    }
}
