//! Frank-Wolfe user-equilibrium assignment.
//!
//! Each iteration loads all demand on the current cheapest paths (all-or-nothing),
//! then moves the flows part of the way toward that target. The step is the one that
//! minimises the Beckmann objective along the line. Simple and light on memory, but it
//! converges slowly: it is the reference solver, not the main one.
//!
//! Costs can include turns. A turn costs its fixed penalty from the turn table plus a
//! delay that depends on the turn's own flow (`turn_vdf`). Interacting junction
//! models (delay depending on other flows) are not supported.

use crate::{
    AssignmentError, Network, NoDelay, OdDemand, SolverError, TurnTable, Vdf, all_or_nothing,
};

#[derive(Debug, Clone, Copy)]
pub struct FrankWolfeOptions {
    pub max_iterations: usize,
    /// Stop when the relative gap is at or below this.
    pub gap_tolerance: f64,
}

impl Default for FrankWolfeOptions {
    fn default() -> Self {
        FrankWolfeOptions {
            max_iterations: 1000,
            gap_tolerance: 1e-4,
        }
    }
}

#[derive(Debug)]
pub struct FrankWolfeResult {
    pub link_flow: Vec<f64>,
    pub turn_flow: Vec<f64>,
    /// Number of flow updates performed.
    pub iterations: usize,
    /// Relative gap: `1 - (cost of the all-or-nothing target) / (cost of the current flows)`.
    pub relative_gap: f64,
    /// Beckmann objective: link integrals, turn delay integrals and fixed turn penalties.
    pub objective: f64,
    pub converged: bool,
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Cost of each turn: fixed penalty plus flow-dependent delay.
fn turn_costs<T: Vdf>(turns: &TurnTable, turn_vdf: &T, flow: &[f64], out: &mut [f64]) {
    turn_vdf.cost(flow, out);
    for (t, slot) in out.iter_mut().enumerate() {
        *slot += turns.penalty(t);
    }
}

/// Step size in [0, 1] that minimises the objective along the line, given the slope of
/// the objective at each step. The objective is convex along the line, so the slope
/// increases with the step: if it is still negative at 1, take the whole step;
/// otherwise find its zero by bisection.
fn line_search(mut slope: impl FnMut(f64) -> f64) -> f64 {
    if slope(1.0) <= 0.0 {
        return 1.0;
    }
    if slope(0.0) >= 0.0 {
        return 0.0;
    }
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..60 {
        let mid = 0.5 * (low + high);
        if slope(mid) > 0.0 {
            high = mid;
        } else {
            low = mid;
        }
    }
    0.5 * (low + high)
}

/// Frank-Wolfe with link delays and fixed turn penalties only.
pub fn frank_wolfe<V: Vdf>(
    network: &Network,
    turns: &TurnTable,
    vdf: &V,
    demand: &OdDemand,
    options: FrankWolfeOptions,
) -> Result<FrankWolfeResult, SolverError> {
    let no_delay = NoDelay::new(turns.num_turns());
    frank_wolfe_with_turn_delays(network, turns, vdf, &no_delay, demand, options)
}

/// Frank-Wolfe where turns also have a flow-dependent delay `turn_vdf`, indexed by turn id.
pub fn frank_wolfe_with_turn_delays<V: Vdf, T: Vdf>(
    network: &Network,
    turns: &TurnTable,
    vdf: &V,
    turn_vdf: &T,
    demand: &OdDemand,
    options: FrankWolfeOptions,
) -> Result<FrankWolfeResult, SolverError> {
    let (nl, nt) = (network.num_links(), turns.num_turns());
    if vdf.len() != nl {
        return Err(SolverError::WrongNumberOfLinks {
            expected: nl,
            found: vdf.len(),
        });
    }
    if turn_vdf.len() != nt {
        return Err(SolverError::WrongNumberOfTurns {
            expected: nt,
            found: turn_vdf.len(),
        });
    }

    let target_of = |link_cost: &[f64], turn_cost: &[f64]| -> Result<_, AssignmentError> {
        all_or_nothing(network, turns, link_cost, turn_cost, demand)
    };

    // Start from all-or-nothing on zero-flow costs.
    let mut link_cost = vec![0.0; nl];
    let mut turn_cost = vec![0.0; nt];
    vdf.cost(&vec![0.0; nl], &mut link_cost);
    turn_costs(turns, turn_vdf, &vec![0.0; nt], &mut turn_cost);
    let start = target_of(&link_cost, &turn_cost)?;
    let (mut x_link, mut x_turn) = (start.link_flow, start.turn_flow);

    let mut d_link = vec![0.0; nl];
    let mut d_turn = vec![0.0; nt];
    let mut trial_link = vec![0.0; nl];
    let mut trial_turn = vec![0.0; nt];
    let mut trial_link_cost = vec![0.0; nl];
    let mut trial_turn_cost = vec![0.0; nt];
    let mut iterations = 0;

    loop {
        vdf.cost(&x_link, &mut link_cost);
        turn_costs(turns, turn_vdf, &x_turn, &mut turn_cost);
        let target = target_of(&link_cost, &turn_cost)?;

        let current_total = dot(&link_cost, &x_link) + dot(&turn_cost, &x_turn);
        let target_total = dot(&link_cost, &target.link_flow) + dot(&turn_cost, &target.turn_flow);
        let gap = if current_total > 0.0 {
            ((current_total - target_total) / current_total).max(0.0)
        } else {
            0.0 // no demand
        };

        let converged = gap <= options.gap_tolerance;
        if converged || iterations >= options.max_iterations {
            let penalties: f64 = (0..nt).map(|t| turns.penalty(t) * x_turn[t]).sum();
            return Ok(FrankWolfeResult {
                objective: vdf.integral(&x_link) + turn_vdf.integral(&x_turn) + penalties,
                link_flow: x_link,
                turn_flow: x_turn,
                iterations,
                relative_gap: gap,
                converged,
            });
        }

        for i in 0..nl {
            d_link[i] = target.link_flow[i] - x_link[i];
        }
        for t in 0..nt {
            d_turn[t] = target.turn_flow[t] - x_turn[t];
        }
        let step = line_search(|step| {
            for i in 0..nl {
                trial_link[i] = x_link[i] + step * d_link[i];
            }
            for t in 0..nt {
                trial_turn[t] = x_turn[t] + step * d_turn[t];
            }
            vdf.cost(&trial_link, &mut trial_link_cost);
            turn_costs(turns, turn_vdf, &trial_turn, &mut trial_turn_cost);
            dot(&trial_link_cost, &d_link) + dot(&trial_turn_cost, &d_turn)
        });
        for i in 0..nl {
            x_link[i] += step * d_link[i];
        }
        for t in 0..nt {
            x_turn[t] += step * d_turn[t];
        }
        iterations += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bpr, Edge, TurnException, TurnOptions, TurnRule};

    // Two parallel routes from node 0 to node 1.
    fn two_routes() -> (Network, TurnTable, OdDemand) {
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
        let net = Network::from_edges(2, &edges).unwrap();
        let turns = TurnTable::build(&net, &[false; 2], &[], TurnOptions::default()).unwrap();
        let demand = OdDemand::from_triples(2, &[0], &[1], &[30.0]).unwrap();
        (net, turns, demand)
    }

    fn bpr(t0: [f64; 2], cap: [f64; 2], power: f64) -> Bpr {
        Bpr::new(t0.to_vec(), cap.to_vec(), vec![1.0; 2], vec![power; 2]).unwrap()
    }

    #[test]
    fn linear_costs_have_a_closed_form_solution() {
        // c1 = 10 + v1, c2 = 20 + v2, total demand 30:
        // equal costs at v1 = 20, v2 = 10, both costing 30.
        let (net, turns, demand) = two_routes();
        let vdf = bpr([10.0, 20.0], [10.0, 20.0], 1.0);
        let opts = FrankWolfeOptions {
            max_iterations: 100,
            gap_tolerance: 1e-10,
        };
        let result = frank_wolfe(&net, &turns, &vdf, &demand, opts).unwrap();
        assert!(result.converged);
        assert!(
            (result.link_flow[0] - 20.0).abs() < 1e-6,
            "{:?}",
            result.link_flow
        );
        assert!((result.link_flow[1] - 10.0).abs() < 1e-6);
        assert!(result.relative_gap <= 1e-10);
    }

    #[test]
    fn nonlinear_costs_reach_equal_route_costs() {
        let (net, turns, demand) = two_routes();
        let vdf = bpr([10.0, 14.0], [12.0, 18.0], 4.0);
        let opts = FrankWolfeOptions {
            max_iterations: 500,
            gap_tolerance: 1e-9,
        };
        let result = frank_wolfe(&net, &turns, &vdf, &demand, opts).unwrap();
        assert!(result.converged);
        assert!((result.link_flow.iter().sum::<f64>() - 30.0).abs() < 1e-9);
        let mut cost = [0.0; 2];
        vdf.cost(&result.link_flow, &mut cost);
        assert!((cost[0] - cost[1]).abs() < 1e-5 * cost[0], "{cost:?}");
    }

    #[test]
    fn objective_never_increases_and_gap_shrinks_with_more_iterations() {
        let (net, turns, demand) = two_routes();
        let vdf = bpr([10.0, 14.0], [12.0, 18.0], 4.0);
        let run = |max_iterations| {
            let opts = FrankWolfeOptions {
                max_iterations,
                gap_tolerance: 0.0,
            };
            frank_wolfe(&net, &turns, &vdf, &demand, opts).unwrap()
        };
        let (a, b, c) = (run(0), run(1), run(5));
        assert!(b.objective <= a.objective + 1e-9);
        assert!(c.objective <= b.objective + 1e-9);
        assert!(c.relative_gap <= a.relative_gap);
        assert_eq!(a.iterations, 0);
        assert!(!a.converged || a.relative_gap == 0.0);
    }

    #[test]
    fn no_demand_converges_immediately() {
        let (net, turns, _) = two_routes();
        let demand = OdDemand::from_triples(2, &[], &[], &[]).unwrap();
        let vdf = bpr([10.0, 20.0], [10.0, 20.0], 4.0);
        let result =
            frank_wolfe(&net, &turns, &vdf, &demand, FrankWolfeOptions::default()).unwrap();
        assert!(result.converged);
        assert_eq!(result.link_flow, [0.0, 0.0]);
        assert_eq!(result.iterations, 0);
    }

    // Routes 0->1->3 (links 0, 1) and 0->2->3 (links 2, 3); links cost 3 each.
    // Turn (0,1) has delay 2 + 0.2 v; turn (2,3) has a constant delay of 5.
    // Equal costs when 6 + 2 + 0.2 vA = 6 + 5, so vA = 15.
    fn two_turn_routes() -> (Network, Bpr, OdDemand, Bpr) {
        let edges: Vec<Edge> = [(0, 1), (1, 3), (0, 2), (2, 3)]
            .iter()
            .enumerate()
            .map(|(i, &(from, to))| Edge {
                link_id: i as u32,
                from,
                to,
            })
            .collect();
        let net = Network::from_edges(4, &edges).unwrap();
        let links = Bpr::new(vec![3.0; 4], vec![1.0; 4], vec![0.0; 4], vec![1.0; 4]).unwrap();
        let demand = OdDemand::from_triples(4, &[0], &[3], &[30.0]).unwrap();
        // Turn ids follow the table: (0,1) is turn 0 and (2,3) is turn 1.
        let turn_vdf = Bpr::new(
            vec![2.0, 5.0],
            vec![10.0, 1.0],
            vec![1.0, 0.0],
            vec![1.0, 1.0],
        )
        .unwrap();
        (net, links, demand, turn_vdf)
    }

    #[test]
    fn turn_delays_shift_flow_between_routes() {
        let (net, links, demand, turn_vdf) = two_turn_routes();
        let turns = TurnTable::build(&net, &[false; 4], &[], TurnOptions::default()).unwrap();
        assert_eq!(
            (turns.turn_between(0, 1), turns.turn_between(2, 3)),
            (Some(0), Some(1))
        );
        let opts = FrankWolfeOptions {
            max_iterations: 200,
            gap_tolerance: 1e-10,
        };
        let r =
            frank_wolfe_with_turn_delays(&net, &turns, &links, &turn_vdf, &demand, opts).unwrap();
        assert!(r.converged);
        assert!((r.turn_flow[0] - 15.0).abs() < 1e-6, "{:?}", r.turn_flow);
        assert!((r.turn_flow[1] - 15.0).abs() < 1e-6);
        assert_eq!(r.link_flow[0], r.turn_flow[0]);
    }

    #[test]
    fn fixed_turn_penalties_count_as_cost() {
        // A penalty of 2 on turn (2,3) makes route B cost 13: 8 + 0.2 vA = 13, vA = 25.
        let (net, links, demand, turn_vdf) = two_turn_routes();
        let penalty = TurnException {
            from_link: 2,
            to_link: 3,
            rule: TurnRule::Penalty(2.0),
        };
        let turns =
            TurnTable::build(&net, &[false; 4], &[penalty], TurnOptions::default()).unwrap();
        let opts = FrankWolfeOptions {
            max_iterations: 200,
            gap_tolerance: 1e-10,
        };
        let r =
            frank_wolfe_with_turn_delays(&net, &turns, &links, &turn_vdf, &demand, opts).unwrap();
        assert!((r.turn_flow[0] - 25.0).abs() < 1e-6, "{:?}", r.turn_flow);
        assert!((r.turn_flow[1] - 5.0).abs() < 1e-6);
        // The penalty is part of the objective: 2 per vehicle on 5 vehicles.
        let base = links.integral(&r.link_flow) + turn_vdf.integral(&r.turn_flow);
        assert!((r.objective - base - 10.0).abs() < 1e-6);
    }

    #[test]
    fn rejects_wrong_sizes() {
        let (net, turns, demand) = two_routes();
        let small = Bpr::new(vec![1.0], vec![1.0], vec![0.15], vec![4.0]).unwrap();
        assert_eq!(
            frank_wolfe(&net, &turns, &small, &demand, FrankWolfeOptions::default()).unwrap_err(),
            SolverError::WrongNumberOfLinks {
                expected: 2,
                found: 1
            }
        );
        let vdf = bpr([10.0, 20.0], [10.0, 20.0], 4.0);
        let extra = NoDelay::new(3);
        assert_eq!(
            frank_wolfe_with_turn_delays(
                &net,
                &turns,
                &vdf,
                &extra,
                &demand,
                FrankWolfeOptions::default()
            )
            .unwrap_err(),
            SolverError::WrongNumberOfTurns {
                expected: 0,
                found: 3
            }
        );
    }
}
