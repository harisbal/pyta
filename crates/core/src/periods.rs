//! Several time periods in a row, with trips that do not finish loading the next one.
//!
//! Every period is a static multi-class assignment over trips that depart uniformly during
//! the period. A trip on a path of travel time `tau` that departs at `s` in `[0, T]` is still
//! travelling at the end if `s + tau > T`. Departures are uniform, so the trips still
//! travelling are uniform in how far they have got, measured in travel time: on a path with
//! flow `q` the trips on link `j`, whose slot of the path is the travel-time interval
//! `[a_j, b_j]`, number
//!
//! ```text
//!     c_j = q * |[a_j, b_j] ∩ [0, min(tau, T)]| / T
//! ```
//!
//! (Little's law). A link's slot is its travel time plus the fixed penalty and delay of the
//! turn onto the next link. These trips are not assigned again: they follow the rest of their
//! path, so in the next period every link after link `j` of that path starts with `c_j` extra
//! vehicles (times the class's PCE), and so does every turn from link `j` onwards. That
//! *preload* is added to the volume the delay functions see.
//!
//! Approximations: a carried trip is placed at the end of the link it is on, a preload is
//! not itself carried on if its remaining trip is longer than the next period, and it loads
//! the whole next period although in reality it is there at the start.

use crate::{
    Bpr, ClassSpec, GradientProjectionOptions, MultiClassResult, Network, Preloaded, SolverError,
    TurnTable, Vdf, gradient_projection_multiclass,
};

/// One period.
#[derive(Debug, Clone, Copy)]
pub struct Period<'a> {
    /// Length in minutes, the same unit as travel times. Link and turn capacities are per
    /// hour and are multiplied by `duration / 60` for this period.
    pub duration: f64,
    /// The period's own trips and class parameters. Periods may have different classes.
    pub classes: &'a [ClassSpec<'a>],
}

#[derive(Debug)]
pub struct PeriodResult {
    /// The equilibrium of this period. Its volumes do not include the preload, but the
    /// travel times were computed with it.
    pub assignment: MultiClassResult,
    /// PCE-weighted vehicles per link that were already on the network when the period began.
    pub link_preload: Vec<f64>,
    /// The same per turn.
    pub turn_preload: Vec<f64>,
}

/// Assigns `periods` one after the other.
///
/// `link_vdf` and `turn_vdf` have capacities per hour. The same `options` are used for every
/// period, and every period runs to its own gap tolerance.
pub fn assign_periods(
    network: &Network,
    turns: &TurnTable,
    link_vdf: &Bpr,
    turn_vdf: &Bpr,
    periods: &[Period],
    options: GradientProjectionOptions,
) -> Result<Vec<PeriodResult>, SolverError> {
    for (i, period) in periods.iter().enumerate() {
        if !(period.duration.is_finite() && period.duration > 0.0) {
            return Err(SolverError::InvalidPeriod {
                period: i,
                reason: "duration must be positive and finite",
            });
        }
        if period.classes.is_empty() {
            return Err(SolverError::InvalidPeriod {
                period: i,
                reason: "a period needs at least one class",
            });
        }
    }

    let mut results = Vec::with_capacity(periods.len());
    let mut link_preload = vec![0.0; network.num_links()];
    let mut turn_preload = vec![0.0; turns.num_turns()];
    for period in periods {
        let hours = period.duration / 60.0;
        let link_vdf = link_vdf.with_capacity_scaled(hours);
        let turn_vdf = turn_vdf.with_capacity_scaled(hours);
        let assignment = gradient_projection_multiclass(
            network,
            turns,
            &Preloaded::new(&link_vdf, &link_preload),
            &Preloaded::new(&turn_vdf, &turn_preload),
            period.classes,
            options,
        )?;

        // What this period leaves on the network loads the next one.
        let (next_links, next_turns) = leftover(
            network,
            turns,
            &Preloaded::new(&link_vdf, &link_preload),
            &Preloaded::new(&turn_vdf, &turn_preload),
            period,
            &assignment,
        );
        results.push(PeriodResult {
            assignment,
            link_preload: std::mem::replace(&mut link_preload, next_links),
            turn_preload: std::mem::replace(&mut turn_preload, next_turns),
        });
    }
    Ok(results)
}

/// Vehicles per link and per turn that the trips still travelling at the end of `period`
/// will load in the next one.
fn leftover(
    network: &Network,
    turns: &TurnTable,
    link_vdf: &impl Vdf,
    turn_vdf: &impl Vdf,
    period: &Period,
    result: &MultiClassResult,
) -> (Vec<f64>, Vec<f64>) {
    let mut link_load = vec![0.0; network.num_links()];
    let mut turn_load = vec![0.0; turns.num_turns()];
    for (class, spec) in result.classes.iter().zip(period.classes) {
        for (_, _, links, path_turns, flow) in class.paths.iter_with_turns() {
            debug_assert_eq!(path_turns.len() + 1, links.len());
            // Time of each slot: the link, then the turn onto the next link.
            let slots: Vec<f64> = links
                .iter()
                .enumerate()
                .map(|(i, &link)| {
                    let mut time =
                        link_vdf.cost_at(link as usize, result.link_volume[link as usize]);
                    if let Some(&turn) = path_turns.get(i) {
                        time += turns.penalty(turn as usize)
                            + turn_vdf.cost_at(turn as usize, result.turn_volume[turn as usize]);
                    }
                    time
                })
                .collect();
            let horizon = slots.iter().sum::<f64>().min(period.duration);

            // `ahead` is the number of trips that are on an earlier link of this path: they
            // still have to enter this one. The turn after link `m` is taken by those on
            // links up to `m`.
            let mut start = 0.0;
            let mut ahead = 0.0;
            for (m, (&link, &slot)) in links.iter().zip(&slots).enumerate() {
                link_load[link as usize] += ahead * spec.pce;
                let here =
                    flow * ((start + slot).min(horizon) - start.min(horizon)) / period.duration;
                ahead += here;
                start += slot;
                if let Some(&turn) = path_turns.get(m) {
                    turn_load[turn as usize] += ahead * spec.pce;
                }
            }
        }
    }
    (link_load, turn_load)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Edge, OdDemand, TurnException, TurnOptions, TurnRule};

    /// a(0) -> b(1) -> c(2) with times 10 and 20 minutes and no congestion (b = 0).
    struct Line {
        network: Network,
        turns: TurnTable,
        link_vdf: Bpr,
        turn_vdf: Bpr,
    }

    fn line_with(exceptions: &[TurnException], link_vdf: Bpr) -> Line {
        let edges = [
            Edge {
                link_id: 0,
                from: 0,
                to: 1,
            },
            Edge {
                link_id: 1,
                from: 1,
                to: 2,
            },
        ];
        let network = Network::from_edges(3, &edges).unwrap();
        let turns =
            TurnTable::build(&network, &[false; 3], exceptions, TurnOptions::default()).unwrap();
        let n = turns.num_turns();
        let turn_vdf = Bpr::new(vec![0.0; n], vec![1.0; n], vec![0.0; n], vec![1.0; n]).unwrap();
        Line {
            network,
            turns,
            link_vdf,
            turn_vdf,
        }
    }

    fn line() -> Line {
        let free = Bpr::new(vec![10.0, 20.0], vec![1e6; 2], vec![0.0; 2], vec![4.0; 2]).unwrap();
        line_with(&[], free)
    }

    fn demand(trips: &[(u32, u32, f64)]) -> OdDemand {
        let o: Vec<u32> = trips.iter().map(|t| t.0).collect();
        let d: Vec<u32> = trips.iter().map(|t| t.1).collect();
        let q: Vec<f64> = trips.iter().map(|t| t.2).collect();
        OdDemand::from_triples(3, &o, &d, &q).unwrap()
    }

    fn run(line: &Line, periods: &[Period]) -> Vec<PeriodResult> {
        assign_periods(
            &line.network,
            &line.turns,
            &line.link_vdf,
            &line.turn_vdf,
            periods,
            GradientProjectionOptions::default(),
        )
        .unwrap()
    }

    fn close(a: &[f64], b: &[f64]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn trips_on_earlier_links_preload_the_links_still_ahead_of_them() {
        let line = line();
        let d = demand(&[(0, 2, 60.0)]);
        let none = demand(&[]);
        let (busy, quiet) = ([ClassSpec::new(&d)], [ClassSpec::new(&none)]);
        let r = run(
            &line,
            &[
                Period {
                    duration: 60.0,
                    classes: &busy,
                },
                Period {
                    duration: 60.0,
                    classes: &quiet,
                },
                Period {
                    duration: 60.0,
                    classes: &quiet,
                },
            ],
        );
        // Path time 30 of 60 minutes. A third of the trips still travelling are on the
        // 10-minute link (60 * 10 / 60 = 10) and still have to enter the second link.
        assert!(close(&r[0].link_preload, &[0.0, 0.0]));
        assert!(
            close(&r[1].link_preload, &[0.0, 10.0]),
            "{:?}",
            r[1].link_preload
        );
        // The preload is not carried on again, and the assignment itself ignores it.
        assert!(close(&r[2].link_preload, &[0.0, 0.0]));
        assert!(close(&r[1].assignment.link_volume, &[0.0, 0.0]));
        assert!(close(&r[0].assignment.link_volume, &[60.0, 60.0]));
        // The only turn (link 0 to link 1) is taken by the trips on link 0.
        assert!(
            close(&r[1].turn_preload, &[10.0]),
            "{:?}",
            r[1].turn_preload
        );
    }

    #[test]
    fn a_trip_longer_than_the_period_is_all_still_travelling() {
        let line = line();
        let d = demand(&[(0, 2, 30.0)]);
        let none = demand(&[]);
        let (busy, quiet) = ([ClassSpec::new(&d)], [ClassSpec::new(&none)]);
        // Period of 15 minutes, path of 30: all 30 trips are on the network, spread over the
        // first 15 minutes of the path: 30 * 10/15 = 20 on link 0, 30 * 5/15 = 10 on link 1.
        let r = run(
            &line,
            &[
                Period {
                    duration: 15.0,
                    classes: &busy,
                },
                Period {
                    duration: 15.0,
                    classes: &quiet,
                },
            ],
        );
        assert!(
            close(&r[1].link_preload, &[0.0, 20.0]),
            "{:?}",
            r[1].link_preload
        );
    }

    #[test]
    fn turn_penalties_lengthen_the_slot_of_the_link_before_them() {
        let exceptions = [TurnException {
            from_link: 0,
            to_link: 1,
            rule: TurnRule::Penalty(10.0),
        }];
        let free = Bpr::new(vec![10.0, 20.0], vec![1e6; 2], vec![0.0; 2], vec![4.0; 2]).unwrap();
        let line = line_with(&exceptions, free);
        let d = demand(&[(0, 2, 60.0)]);
        let none = demand(&[]);
        let (busy, quiet) = ([ClassSpec::new(&d)], [ClassSpec::new(&none)]);
        let r = run(
            &line,
            &[
                Period {
                    duration: 60.0,
                    classes: &busy,
                },
                Period {
                    duration: 60.0,
                    classes: &quiet,
                },
            ],
        );
        // Link 0 and its turn take 20 of the 40 minutes of the path: 60 * 20 / 60 = 20.
        assert!(
            close(&r[1].link_preload, &[0.0, 20.0]),
            "{:?}",
            r[1].link_preload
        );
        assert!(close(&r[1].turn_preload, &[20.0]));
    }

    #[test]
    fn preload_is_pce_weighted_and_slows_the_next_period() {
        let cong = Bpr::new(
            vec![10.0, 20.0],
            vec![100.0; 2],
            vec![0.15; 2],
            vec![4.0; 2],
        )
        .unwrap();
        let line = line_with(&[], cong);
        let d = demand(&[(0, 2, 60.0)]);
        let none = demand(&[]);
        let trucks = [ClassSpec {
            pce: 2.0,
            ..ClassSpec::new(&d)
        }];
        let (cars, quiet) = ([ClassSpec::new(&d)], [ClassSpec::new(&none)]);
        let r = run(
            &line,
            &[
                Period {
                    duration: 60.0,
                    classes: &trucks,
                },
                Period {
                    duration: 60.0,
                    classes: &quiet,
                },
                Period {
                    duration: 60.0,
                    classes: &cars,
                },
            ],
        );
        // Link volume is 120 (60 trucks of PCE 2) on a capacity of 100.
        let x: f64 = 1.2;
        let slot0 = 10.0 * (1.0 + 0.15 * x.powi(4));
        let slot1 = 20.0 * (1.0 + 0.15 * x.powi(4));
        let on_link0 = 60.0 * slot0.min(60.0) / 60.0;
        assert!((r[1].link_preload[1] - 2.0 * on_link0).abs() < 1e-9);
        assert!(slot0 + slot1 < 60.0); // so nothing was cut off by the horizon
        // A preload is not carried on again, so a period after a quiet one starts clean.
        assert!(close(&r[2].link_preload, &[0.0, 0.0]));
    }

    #[test]
    fn a_preload_diverts_flow_to_the_other_route() {
        // Two parallel links 0 -> 1 with the same delay function.
        let edges = [
            Edge {
                link_id: 0,
                from: 0,
                to: 1,
            },
            Edge {
                link_id: 1,
                from: 0,
                to: 1,
            },
        ];
        let network = Network::from_edges(2, &edges).unwrap();
        let turns = TurnTable::build(&network, &[false; 2], &[], TurnOptions::default()).unwrap();
        let bpr = Bpr::new(vec![10.0; 2], vec![100.0; 2], vec![0.15; 2], vec![4.0; 2]).unwrap();
        let none = crate::NoDelay::new(0);
        let d = OdDemand::from_triples(2, &[0], &[1], &[100.0]).unwrap();
        let classes = [ClassSpec::new(&d)];
        let solve = |preload: &[f64]| {
            gradient_projection_multiclass(
                &network,
                &turns,
                &Preloaded::new(&bpr, preload),
                &none,
                &classes,
                GradientProjectionOptions::default(),
            )
            .unwrap()
            .link_volume
        };
        let even = solve(&[0.0, 0.0]);
        assert!((even[0] - 50.0).abs() < 1e-3 && (even[1] - 50.0).abs() < 1e-3);
        // 100 vehicles already on link 0: equal costs need 100 + x0 = x1 + 0 with x0 + x1 = 100.
        let shifted = solve(&[100.0, 0.0]);
        assert!(
            shifted[0] < 1e-6 && (shifted[1] - 100.0).abs() < 1e-6,
            "{shifted:?}"
        );
    }

    #[test]
    fn capacity_per_hour_is_scaled_to_the_period() {
        let cong = Bpr::new(
            vec![10.0, 20.0],
            vec![100.0; 2],
            vec![0.15; 2],
            vec![4.0; 2],
        )
        .unwrap();
        let line = line_with(&[], cong);
        let d = demand(&[(0, 2, 300.0)]);
        let none = demand(&[]);
        let (busy, quiet) = ([ClassSpec::new(&d)], [ClassSpec::new(&none)]);
        // 2 hours at 100 per hour is a capacity of 200 for 300 trips.
        let r = run(
            &line,
            &[
                Period {
                    duration: 120.0,
                    classes: &busy,
                },
                Period {
                    duration: 120.0,
                    classes: &quiet,
                },
            ],
        );
        let x: f64 = 300.0 / 200.0;
        let slot0 = 10.0 * (1.0 + 0.15 * x.powi(4));
        assert!((r[1].link_preload[1] - 300.0 * slot0 / 120.0).abs() < 1e-9);
    }

    #[test]
    fn periods_are_validated() {
        let line = line();
        let d = demand(&[(0, 2, 1.0)]);
        let one = [ClassSpec::new(&d)];
        let go = |periods: &[Period]| {
            assign_periods(
                &line.network,
                &line.turns,
                &line.link_vdf,
                &line.turn_vdf,
                periods,
                GradientProjectionOptions::default(),
            )
        };
        assert!(matches!(
            go(&[Period {
                duration: 0.0,
                classes: &one
            }]),
            Err(SolverError::InvalidPeriod { period: 0, .. })
        ));
        assert!(matches!(
            go(&[Period {
                duration: 5.0,
                classes: &[]
            }]),
            Err(SolverError::InvalidPeriod { period: 0, .. })
        ));
        assert!(go(&[]).unwrap().is_empty());
    }
}
