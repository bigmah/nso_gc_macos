//! Measuring what the link actually delivers.
//!
//! Over BLE a report can only reach the host on a connection event, so the gap
//! between consecutive reports *is* the connection interval — and a histogram of
//! those gaps identifies it without any privileged API. That matters because the
//! documented ways to read the interval back are all closed to an unentitled
//! process, so this is the measurement of record.
//!
//! The bins are placed on the intervals BLE actually uses (`bluetoothd` bins its
//! own telemetry the same way: 7.5 ms, 11.25 ms, 15 ms), with generous edges so
//! scheduling jitter does not smear a link across two rows.
//!
//! It works over USB too, where it measures poll spacing instead — which is the
//! comparison worth having, since wired is the target being chased.

use std::time::Instant;

/// Upper edge, in milliseconds, and what a cluster there means.
const BINS: &[(f64, &str)] = &[
    (5.0, "under 5 ms   several reports per connection event"),
    (9.4, "~7.5 ms      7.5 ms interval — the BLE floor"),
    (13.1, "~11.25 ms    11.25 ms interval"),
    (22.5, "~15 ms       15 ms interval — macOS default for a GATT accessory"),
    (37.5, "~30 ms       30 ms interval"),
    (f64::INFINITY, "over 37.5 ms stalls and dropped events"),
];

/// Records the gap between consecutive reports.
pub struct Histogram {
    enabled: bool,
    last: Option<Instant>,
    /// Gaps in microseconds. Bounded so a long session cannot grow without end;
    /// once full we stop recording rather than bias the tail by evicting.
    gaps: Vec<u32>,
}

/// Samples past this point are redundant — the distribution has long converged.
const MAX_SAMPLES: usize = 200_000;

impl Histogram {
    pub fn new(enabled: bool) -> Self {
        Self { enabled, last: None, gaps: Vec::new() }
    }

    /// Call once per report received.
    pub fn tick(&mut self) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        if let Some(prev) = self.last.replace(now)
            && self.gaps.len() < MAX_SAMPLES
        {
            self.gaps.push(now.duration_since(prev).as_micros().min(u32::MAX.into()) as u32);
        }
    }

    /// Which bin a gap of `ms` falls in.
    fn bin_for(ms: f64) -> usize {
        BINS.iter().position(|&(hi, _)| ms < hi).unwrap_or(BINS.len() - 1)
    }

    fn percentile(sorted: &[u32], p: f64) -> f64 {
        if sorted.is_empty() {
            return 0.0;
        }
        let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
        f64::from(sorted[idx]) / 1000.0
    }

    /// Prints the distribution and names the interval it implies.
    pub fn report(&self) {
        if !self.enabled {
            return;
        }
        if self.gaps.len() < 2 {
            println!("\nnot enough reports to measure the interval");
            return;
        }

        let mut sorted = self.gaps.clone();
        sorted.sort_unstable();
        let total = sorted.len();

        let mut counts = vec![0usize; BINS.len()];
        for &g in &sorted {
            counts[Self::bin_for(f64::from(g) / 1000.0)] += 1;
        }

        let median = Self::percentile(&sorted, 0.50);
        println!("\ninter-arrival gaps over {total} reports");
        println!(
            "  median {median:.2} ms   p95 {:.2} ms   p99 {:.2} ms   max {:.2} ms",
            Self::percentile(&sorted, 0.95),
            Self::percentile(&sorted, 0.99),
            Self::percentile(&sorted, 1.0),
        );

        for (i, &(_, label)) in BINS.iter().enumerate() {
            let n = counts[i];
            if n == 0 {
                continue;
            }
            let frac = n as f64 / total as f64;
            let bar = "█".repeat((frac * 40.0).round() as usize);
            println!("  {label:<58} {:5.1}% {bar}", frac * 100.0);
        }

        // The modal bin is the link's operating point; the median confirms it.
        let modal = counts.iter().enumerate().max_by_key(|&(_, n)| *n).map(|(i, _)| i);
        if let Some(i) = modal {
            println!("\n  operating point: {}", BINS[i].1.trim_start());
            println!(
                "  mean added latency from the link alone: about {:.1} ms",
                median / 2.0
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of this module is telling 7.5 from 11.25 from 15 ms, so
    /// each nominal interval must land in its own bin with room for jitter.
    #[test]
    fn each_ble_interval_lands_in_its_own_bin() {
        let bins: Vec<usize> = [7.5, 11.25, 15.0, 30.0].iter().map(|&ms| Histogram::bin_for(ms)).collect();
        assert_eq!(bins, vec![1, 2, 3, 4]);
        // Distinct, or the histogram cannot distinguish the cases it exists for.
        let mut sorted = bins.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), bins.len());
    }

    #[test]
    fn jitter_does_not_smear_an_interval_across_bins() {
        // A 15 ms link wobbling by a millisecond either way stays put.
        for ms in [13.6, 14.5, 15.0, 15.5, 16.4] {
            assert_eq!(Histogram::bin_for(ms), 3, "{ms} ms escaped the 15 ms bin");
        }
        // Same for the fast case we are chasing.
        for ms in [6.6, 7.5, 8.4] {
            assert_eq!(Histogram::bin_for(ms), 1, "{ms} ms escaped the 7.5 ms bin");
        }
    }

    #[test]
    fn batched_reports_and_stalls_are_called_out_separately() {
        assert_eq!(Histogram::bin_for(0.4), 0, "sub-event batching");
        assert_eq!(Histogram::bin_for(120.0), BINS.len() - 1, "a stall");
    }

    /// Nearest-rank, and reported in milliseconds from microsecond samples.
    #[test]
    fn percentiles_use_nearest_rank_and_convert_to_milliseconds() {
        let sorted: Vec<u32> = (1..=100).map(|n| n * 1000).collect();
        // round(99 * 0.5) = 50, so the median is the 51st sample.
        assert_eq!(Histogram::percentile(&sorted, 0.5), 51.0);
        assert_eq!(Histogram::percentile(&sorted, 0.95), 95.0);
        assert_eq!(Histogram::percentile(&sorted, 1.0), 100.0);
        assert_eq!(Histogram::percentile(&[], 0.5), 0.0);
    }

    #[test]
    fn a_disabled_histogram_records_nothing() {
        let mut h = Histogram::new(false);
        h.tick();
        h.tick();
        assert!(h.gaps.is_empty());
    }

    #[test]
    fn the_first_report_starts_the_clock_rather_than_recording_a_gap() {
        let mut h = Histogram::new(true);
        h.tick();
        assert!(h.gaps.is_empty(), "there is no gap before the first report");
        h.tick();
        assert_eq!(h.gaps.len(), 1);
    }
}
