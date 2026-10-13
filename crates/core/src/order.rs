//! The order in which a Gauss-Seidel sweep visits origins.

/// How origins are ordered in each sweep. All of them visit every origin exactly once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OriginOrder {
    /// Ascending node id, every sweep. The baseline.
    Fixed,
    /// A new random permutation each sweep. The seed makes runs repeatable.
    Shuffle {
        seed: u64,
    },
    /// Ascending on even sweeps, descending on odd ones (a symmetric Gauss-Seidel sweep).
    Alternate,
    /// Origins with the largest remaining gap (as measured in the previous sweep) first.
    LargestGapFirst,
    LargestDemandFirst,
    SmallestDemandFirst,
}

/// SplitMix64: a tiny seedable generator, enough for shuffling.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform-ish integer in `0..bound`.
    fn below(&mut self, bound: usize) -> usize {
        ((self.next() as u128 * bound as u128) >> 64) as usize
    }
}

/// Produces the origin order for each sweep.
pub struct OriginSequencer {
    order: OriginOrder,
    rng: SplitMix64,
    sweep: usize,
    /// Origins with demand, ascending, with their total demand.
    origins: Vec<(u32, f64)>,
    /// Gap contribution per origin from the last sweep, indexed by node id.
    gap: Vec<f64>,
}

impl OriginSequencer {
    /// `origins` are the origins that have demand, as `(node, total demand)`.
    pub fn new(order: OriginOrder, num_nodes: usize, mut origins: Vec<(u32, f64)>) -> Self {
        origins.sort_by_key(|&(node, _)| node);
        let mut gap = vec![0.0; num_nodes];
        // Before any sweep, treat demand as the gap so the first LargestGapFirst sweep is sensible.
        for &(node, demand) in &origins {
            gap[node as usize] = demand;
        }
        let seed = match order {
            OriginOrder::Shuffle { seed } => seed,
            _ => 0,
        };
        OriginSequencer {
            order,
            rng: SplitMix64(seed),
            sweep: 0,
            origins,
            gap,
        }
    }

    /// The origins in the order to visit them in the next sweep.
    pub fn next_sweep(&mut self) -> Vec<u32> {
        let mut entries = self.origins.clone();
        match self.order {
            OriginOrder::Fixed => {}
            OriginOrder::Shuffle { .. } => {
                for i in (1..entries.len()).rev() {
                    entries.swap(i, self.rng.below(i + 1));
                }
            }
            OriginOrder::Alternate => {
                if self.sweep % 2 == 1 {
                    entries.reverse();
                }
            }
            OriginOrder::LargestGapFirst => {
                entries.sort_by(|a, b| self.gap[b.0 as usize].total_cmp(&self.gap[a.0 as usize]));
            }
            OriginOrder::LargestDemandFirst => entries.sort_by(|a, b| b.1.total_cmp(&a.1)),
            OriginOrder::SmallestDemandFirst => entries.sort_by(|a, b| a.1.total_cmp(&b.1)),
        }
        self.sweep += 1;
        entries.into_iter().map(|(node, _)| node).collect()
    }

    /// Records how far `origin` still was from equilibrium when it was last visited.
    pub fn record_gap(&mut self, origin: u32, gap: f64) {
        self.gap[origin as usize] = gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origins() -> Vec<(u32, f64)> {
        vec![(4, 10.0), (1, 30.0), (7, 20.0), (2, 5.0), (9, 25.0)]
    }

    fn sequencer(order: OriginOrder) -> OriginSequencer {
        OriginSequencer::new(order, 10, origins())
    }

    fn sorted(mut v: Vec<u32>) -> Vec<u32> {
        v.sort();
        v
    }

    #[test]
    fn every_order_visits_every_origin_once() {
        for order in [
            OriginOrder::Fixed,
            OriginOrder::Shuffle { seed: 3 },
            OriginOrder::Alternate,
            OriginOrder::LargestGapFirst,
            OriginOrder::LargestDemandFirst,
            OriginOrder::SmallestDemandFirst,
        ] {
            let mut seq = sequencer(order);
            for _ in 0..4 {
                assert_eq!(sorted(seq.next_sweep()), [1, 2, 4, 7, 9], "{order:?}");
            }
        }
    }

    #[test]
    fn fixed_is_ascending_every_sweep() {
        let mut seq = sequencer(OriginOrder::Fixed);
        assert_eq!(seq.next_sweep(), [1, 2, 4, 7, 9]);
        assert_eq!(seq.next_sweep(), [1, 2, 4, 7, 9]);
    }

    #[test]
    fn alternate_flips_direction() {
        let mut seq = sequencer(OriginOrder::Alternate);
        assert_eq!(seq.next_sweep(), [1, 2, 4, 7, 9]);
        assert_eq!(seq.next_sweep(), [9, 7, 4, 2, 1]);
        assert_eq!(seq.next_sweep(), [1, 2, 4, 7, 9]);
    }

    #[test]
    fn shuffle_is_repeatable_per_seed_and_changes_between_sweeps() {
        let run = |seed| {
            let mut seq = sequencer(OriginOrder::Shuffle { seed });
            (0..6).map(|_| seq.next_sweep()).collect::<Vec<_>>()
        };
        assert_eq!(run(7), run(7));
        assert_ne!(run(7), run(8));
        let sweeps = run(7);
        assert!(sweeps.windows(2).any(|w| w[0] != w[1]));
    }

    #[test]
    fn demand_orders() {
        assert_eq!(
            sequencer(OriginOrder::LargestDemandFirst).next_sweep(),
            [1, 9, 7, 4, 2]
        );
        assert_eq!(
            sequencer(OriginOrder::SmallestDemandFirst).next_sweep(),
            [2, 4, 7, 9, 1]
        );
    }

    #[test]
    fn largest_gap_first_uses_recorded_gaps() {
        let mut seq = sequencer(OriginOrder::LargestGapFirst);
        assert_eq!(seq.next_sweep(), [1, 9, 7, 4, 2]); // demand stands in for the gap at first
        seq.record_gap(2, 100.0);
        seq.record_gap(1, 0.0);
        assert_eq!(seq.next_sweep()[0], 2);
        assert_eq!(*seq.next_sweep().last().unwrap(), 1);
    }
}
