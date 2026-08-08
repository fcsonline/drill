//! Pure, lazy arrival schedule for the open (arrival-rate) workload model.
//!
//! The closed model (`load_shape` / `rampup`) pre-computes every iteration
//! start offset up front. The open model instead schedules *arrivals* at a
//! configured rate, independent of how fast the server responds. To keep a
//! long or high-rate run from materializing an unbounded schedule, this
//! module yields one arrival offset at a time on demand — never the whole
//! sequence — and is a pure function of a time parameter, so deterministic
//! exact-boundary tests pass without any real wall-clock timing (G6).
//!
//! Two shapes are supported:
//! - **constant**: arrivals spaced `1 / rate` apart.
//! - **ramping**: a list of `(duration, rate)` stages; the rate linearly
//!   interpolates between stages (analogous to k6 `ramping-arrival-rate`).
//!
//! At least one finite budget must be present (`duration` and/or
//! `max_iterations`) so a schedule can never run forever. `duration` stops
//! strictly before reaching the boundary; `max_iterations` stops at exactly
//! that many arrivals. When both are set, the first bound reached wins.

use std::sync::atomic::AtomicUsize;
use std::time::Duration;

/// One ramping stage: hold/ramp to `rate` arrivals/second over `duration`
/// seconds. The rate is reached at the *end* of the stage (linear
/// interpolation from the previous stage, or held flat for the first stage).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArrivalStage {
  /// Stage length in whole seconds.
  pub duration: u64,
  /// Target arrival rate (iterations/second) at the end of the stage.
  pub rate: u64,
}

/// The arrival-rate shape: a single constant rate, or a list of ramping
/// stages (each stage ramps to the next stage's rate over its duration).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArrivalRateShape {
  Constant {
    rate: u64,
  },
  Stages {
    stages: Vec<ArrivalStage>,
  },
}

/// A pure, lazy, exact-boundary arrival schedule.
///
/// [`ArrivalSchedule::next`] returns the *next* arrival offset relative to
/// the schedule's origin (start of the run), or `None` once a budget bound
/// has been reached. It is deterministic: the offsets produced do not depend
/// on wall clock, only on the shape and the budgets.
#[derive(Clone, Debug)]
pub struct ArrivalSchedule {
  shape: ArrivalRateShape,
  /// Budget bound #1: run `duration` seconds, strongly: arrivals after
  /// `>= duration` are not scheduled.
  duration: Option<u64>,
  /// Budget bound #2: at most this many arrivals are scheduled.
  max_iterations: Option<u64>,
  /// Current cumulative time (seconds) since origin, used to fit the next
  /// interval. Always advances forwards.
  elapsed_secs: f64,
  /// Number of arrivals already scheduled.
  scheduled: u64,
}

impl ArrivalSchedule {
  /// Builds a schedule from a shape and budgets. At least one of `duration`
  /// or `max_iterations` is required; returns an [`ArrivalError`] otherwise.
  pub fn new(shape: ArrivalRateShape, duration: Option<u64>, max_iterations: Option<u64>) -> Result<Self, ArrivalError> {
    if duration.is_none() && max_iterations.is_none() {
      return Err(ArrivalError::NoBudget);
    }
    Ok(ArrivalSchedule {
      shape,
      duration,
      max_iterations,
      elapsed_secs: 0.0,
      scheduled: 0,
    })
  }

  /// Returns the next arrival offset relative to the schedule origin, or
  /// `None` when the schedule is exhausted (a budget bound was reached).
  pub fn next(&mut self) -> Option<Duration> {
    // Budget `max_iterations` caps the number of arrivals exactly.
    if let Some(max) = self.max_iterations
      && self.scheduled >= max
    {
      return None;
    }

    // Compute the interval to the next arrival from the instantaneous rate.
    let interval_secs = 1.0 / self.rate_at(self.elapsed_secs);
    if !interval_secs.is_finite() {
      // rate 0 (a zero-rate stage) or overflow => no further arrivals.
      return None;
    }

    // Duration budget stops strictly at the boundary.
    if let Some(dur) = self.duration
      && self.elapsed_secs + interval_secs >= dur as f64
    {
      return None;
    }

    let offset = Duration::from_secs_f64(self.elapsed_secs);
    self.elapsed_secs += interval_secs;
    self.scheduled += 1;
    Some(offset)
  }

  /// Inststantaneous arrival rate (arrivals/sec) at time `t_secs`, for the
  /// current shape. Exposed for exact-boundary tests and for a single
  /// time-point query by a caller that needs `1/rate` precisely.
  fn rate_at(&self, t_secs: f64) -> f64 {
    match &self.shape {
      ArrivalRateShape::Constant {
        rate,
      } => *rate as f64,
      ArrivalRateShape::Stages {
        stages,
      } => {
        if stages.is_empty() {
          return 0.0;
        }
        // Walk stages accumulating their start time to find `t_secs`.
        let mut stage_start: f64 = 0.0;
        for (i, stage) in stages.iter().enumerate() {
          let stage_end = stage_start + stage.duration as f64;
          let start_rate = if i == 0 {
            stages[0].rate as f64
          } else {
            stages[i - 1].rate as f64
          };
          if t_secs < stage_end {
            let p = ((t_secs - stage_start) / stage.duration as f64).clamp(0.0, 1.0);
            return start_rate + (stage.rate as f64 - start_rate) * p;
          }
          stage_start = stage_end;
        }
        // Beyond all stages: hold the last stage's target rate.
        stages.last().unwrap().rate as f64
      }
    }
  }
}

/// Errors producing an [`ArrivalSchedule`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrivalError {
  /// Neither `duration` nor `max_iterations` was set — a run could otherwise
  /// never stop (RFP budget requirement).
  NoBudget,
}

impl std::fmt::Display for ArrivalError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ArrivalError::NoBudget => write!(f, "arrival_rate requires at least one finite budget: `duration` or `max_iterations`"),
    }
  }
}

/// Shared, lock-free counters for the open (arrival-rate) workload model,
/// used by the executor (which mutates them) and the stats streams (which
/// read them). Invariant maintained by the executor: `started + dropped ==
/// scheduled`, so the open-model metrics can be derived at any instant.
#[derive(Default)]
pub struct ArrivalCounters {
  /// Arrivals the schedule produced (accepted into the system).
  pub scheduled: AtomicUsize,
  /// Arrivals that were started as iterations (under the concurrency cap).
  pub started: AtomicUsize,
  /// Arrivals dropped because the in-flight cap was already reached.
  pub dropped: AtomicUsize,
  /// Iterations currently in flight (started minus completed).
  pub in_flight: AtomicUsize,
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
  }

  fn constant(rate: u64) -> ArrivalRateShape {
    ArrivalRateShape::Constant {
      rate,
    }
  }

  /// AC1: constant rate, duration budget → exactly `rate * duration` arrivals
  /// at `0, 1/rate, 2/rate, ...` spaced exactly, terminating at the boundary.
  #[test]
  fn constant_rate_exact_boundaries() {
    // rate 10/s, duration 5s => offsets 0, 100ms, ..., 4900ms => 50 arrivals.
    let mut s = ArrivalSchedule::new(constant(10), Some(5), None).unwrap();
    let mut seen = Vec::new();
    while let Some(off) = s.next() {
      seen.push(off);
    }
    assert_eq!(seen.len(), 50);
    for (i, off) in seen.iter().enumerate() {
      assert_eq!(*off, ms(100 * i as u64));
    }
  }

  /// AC2: constant stage only.
  /// AC2: constant stage only (spacing exact between consecutive arrivals).
  #[test]
  fn constant_rate_spacing_is_exact() {
    let mut s = ArrivalSchedule::new(constant(50), Some(10), None).unwrap();
    let mut prev: Option<Duration> = None;
    let mut count = 0;
    while let Some(off) = s.next() {
      if let Some(p) = prev {
        assert_eq!(off - p, ms(20));
      }
      prev = Some(off);
      count += 1;
    }
    // 50/s * 10s = 500 arrivals.
    assert_eq!(count, 500);
  }

  /// AC3: max_iterations budget terminates the run even though the duration
  /// would otherwise continue.
  #[test]
  fn max_iterations_budget_terminates() {
    let mut s = ArrivalSchedule::new(constant(10), None, Some(25)).unwrap();
    let mut seen = Vec::new();
    while let Some(off) = s.next() {
      seen.push(off);
    }
    assert_eq!(seen.len(), 25);
    // Last offset would be 2400ms; 25 arrived (0..=2400ms).
    assert_eq!(*seen.last().unwrap(), ms(2400));
  }

  /// AC9 constants: same spec, same schedule → identical arrival times.
  #[test]
  fn deterministic_across_recreation() {
    let mut a = ArrivalSchedule::new(constant(7), Some(3), None).unwrap();
    let mut b = ArrivalSchedule::new(constant(7), Some(3), None).unwrap();
    let a_offsets: Vec<Duration> = std::iter::from_fn(|| a.next()).collect();
    let b_offsets: Vec<Duration> = std::iter::from_fn(|| b.next()).collect();
    assert_eq!(a_offsets, b_offsets);
  }

  /// Laziness property: `next` only ever produces the single next arrival and
  /// never the whole list; a large/near-infinite budget does not allocate up
  /// front (R6 no-unbounded-precompute).
  #[test]
  fn schedule_is_lazy() {
    let mut s = ArrivalSchedule::new(constant(1_000_000), None, Some(10)).unwrap();
    for _ in 0..10 {
      assert!(s.next().is_some());
    }
    assert!(s.next().is_none());
  }

  /// R4 budget requirement: no budget → error.
  #[test]
  fn no_budget_is_an_error() {
    let err = ArrivalSchedule::new(constant(10), None, None).unwrap_err();
    assert_eq!(err, ArrivalError::NoBudget);
  }

  /// R: a zero-rate stage naturally terminates the schedule (rate 0).
  #[test]
  fn rate_zero_terminates() {
    let mut s = ArrivalSchedule::new(constant(0), Some(10), None).unwrap();
    assert!(s.next().is_none());
  }

  /// Ramping: first stage flat at its own rate; the second stage interpolates
  /// from 10 → 40 (gap 100ms → 25ms) by its end.
  #[test]
  fn ramping_stages_interpolate() {
    let shape = ArrivalRateShape::Stages {
      stages: vec![
        ArrivalStage {
          duration: 10,
          rate: 10,
        },
        ArrivalStage {
          duration: 10,
          rate: 40,
        },
      ],
    };
    let mut s = ArrivalSchedule::new(shape, Some(20), None).unwrap();
    let mut offsets = Vec::new();
    while let Some(off) = s.next() {
      offsets.push(off);
    }
    // First 10s at 10/s => 100 arrivals spaced 100ms; then a ramping second
    // 10s. The total for 10/s over stage 1 (0..=9.9s) is 100; stage 2 ramps
    // 10→40 adding ~ (10+40)/2 * 10 = 250 more, so ~350 total.
    assert!(!offsets.is_empty());
    // The first 100 offsets are flat 100ms.
    for i in 1..100 {
      assert_eq!(offsets[i] - offsets[i - 1], ms(100), "gap at i={i}");
    }
    // By the end the gap has shrunk toward 25ms.
    let len = offsets.len();
    let last_gap = offsets[len - 1] - offsets[len - 2];
    assert!(last_gap <= ms(30), "final gap approaches 25ms, got {last_gap:?}");
  }

  /// Both budgets set: first bound reached wins (duration here stops earlier).
  #[test]
  fn both_budgets_first_bound_wins() {
    // max_iterations = 1000 would allow 1000; duration 5s stops earlier.
    let mut s = ArrivalSchedule::new(constant(100), Some(5), Some(1000)).unwrap();
    let mut count = 0;
    while s.next().is_some() {
      count += 1;
    }
    assert_eq!(count, 100 * 5);
  }
}
