use crate::{Network, TurnTable, dijkstra_turns};

#[derive(Debug, PartialEq)]
pub enum AssignmentError {
    MismatchedLengths,
    NodeOutOfRange {
        node: u32,
        num_nodes: usize,
    },
    /// Negative, NaN or infinite flow.
    InvalidFlow {
        origin: u32,
        destination: u32,
    },
    /// No allowed path from `origin` to `destination`.
    Unreachable {
        origin: u32,
        destination: u32,
    },
}

/// Errors shared by the equilibrium solvers.
#[derive(Debug, PartialEq)]
pub enum SolverError {
    Assignment(AssignmentError),
    /// The cost function was built for a different number of links.
    WrongNumberOfLinks {
        expected: usize,
        found: usize,
    },
    /// The turn cost function was built for a different number of turns.
    WrongNumberOfTurns {
        expected: usize,
        found: usize,
    },
    /// A traffic class is malformed (`class` is its index).
    InvalidClass {
        class: usize,
        reason: &'static str,
    },
    /// A period is malformed (`period` is its index).
    InvalidPeriod {
        period: usize,
        reason: &'static str,
    },
    /// A class has demand between two nodes its allowed links cannot connect.
    ClassUnreachable {
        class: usize,
        origin: u32,
        destination: u32,
    },
}

impl From<AssignmentError> for SolverError {
    fn from(e: AssignmentError) -> Self {
        SolverError::Assignment(e)
    }
}

/// Demand grouped by origin, in CSR form. Origins and destinations are node ids.
///
/// Flows are in trips per assignment period. Repeated `(origin, destination)`
/// pairs are allowed and add up.
#[derive(Debug)]
pub struct OdDemand {
    offsets: Vec<usize>,
    destinations: Vec<u32>,
    flows: Vec<f64>,
}

impl OdDemand {
    /// Groups long-format demand (entry `i` is `flows[i]` trips from
    /// `origins[i]` to `destinations[i]`) by origin.
    pub fn from_triples(
        num_nodes: usize,
        origins: &[u32],
        destinations: &[u32],
        flows: &[f64],
    ) -> Result<OdDemand, AssignmentError> {
        if origins.len() != destinations.len() || origins.len() != flows.len() {
            return Err(AssignmentError::MismatchedLengths);
        }
        // Same counting sort as the network CSR: count, prefix sum, place.
        let mut offsets = vec![0usize; num_nodes + 1];
        for ((&o, &d), &q) in origins.iter().zip(destinations).zip(flows) {
            for node in [o, d] {
                if node as usize >= num_nodes {
                    return Err(AssignmentError::NodeOutOfRange { node, num_nodes });
                }
            }
            if !q.is_finite() || q < 0.0 {
                return Err(AssignmentError::InvalidFlow {
                    origin: o,
                    destination: d,
                });
            }
            offsets[o as usize + 1] += 1;
        }
        for n in 0..num_nodes {
            offsets[n + 1] += offsets[n];
        }

        let mut next = offsets.clone();
        let mut dest_sorted = vec![0u32; flows.len()];
        let mut flow_sorted = vec![0.0; flows.len()];
        for ((&o, &d), &q) in origins.iter().zip(destinations).zip(flows) {
            let pos = next[o as usize];
            dest_sorted[pos] = d;
            flow_sorted[pos] = q;
            next[o as usize] += 1;
        }
        Ok(OdDemand {
            offsets,
            destinations: dest_sorted,
            flows: flow_sorted,
        })
    }

    pub fn num_nodes(&self) -> usize {
        self.offsets.len() - 1
    }

    /// Destinations and flows of `origin`, as parallel slices.
    pub fn from_origin(&self, origin: u32) -> (&[u32], &[f64]) {
        let range = self.offsets[origin as usize]..self.offsets[origin as usize + 1];
        (&self.destinations[range.clone()], &self.flows[range])
    }

    pub fn total(&self) -> f64 {
        self.flows.iter().sum()
    }
}

/// Link and turn flows from an assignment.
#[derive(Debug)]
pub struct Flows {
    /// Indexed by link id.
    pub link_flow: Vec<f64>,
    /// Indexed by turn id.
    pub turn_flow: Vec<f64>,
}

/// All-or-nothing assignment: every OD pair puts its whole flow on one
/// cheapest path, found with the turn-aware search.
///
/// Trips with `origin == destination` use no network and are skipped.
/// A positive flow to an unreachable destination is an error.
pub fn all_or_nothing(
    network: &Network,
    turns: &TurnTable,
    link_costs: &[f64],
    turn_costs: &[f64],
    demand: &OdDemand,
) -> Result<Flows, AssignmentError> {
    assert_eq!(demand.num_nodes(), network.num_nodes());
    let mut flows = Flows {
        link_flow: vec![0.0; network.num_links()],
        turn_flow: vec![0.0; turns.num_turns()],
    };

    for origin in 0..network.num_nodes() as u32 {
        let (destinations, quantities) = demand.from_origin(origin);
        if destinations.is_empty() {
            continue;
        }
        let tree = dijkstra_turns(network, turns, link_costs, turn_costs, origin);
        for (&destination, &q) in destinations.iter().zip(quantities) {
            if destination == origin || q == 0.0 {
                continue;
            }
            let path = tree
                .path_to(destination)
                .ok_or(AssignmentError::Unreachable {
                    origin,
                    destination,
                })?;
            for &link in &path {
                flows.link_flow[link as usize] += q;
            }
            // The search only follows turns from the table, so this cannot fail.
            let path_turns = turns
                .path_turns(&path)
                .expect("tree paths follow table turns");
            for t in path_turns {
                flows.turn_flow[t] += q;
            }
        }
    }
    Ok(flows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Edge, TurnException, TurnOptions, TurnRule};

    // A=0 X=1 J=2 K=3. 0: A->J (5), 1: A->X (3), 2: X->J (3), 3: J->K (1).
    fn graph() -> Network {
        let edges: Vec<Edge> = [(0, 2), (0, 1), (1, 2), (2, 3)]
            .iter()
            .enumerate()
            .map(|(i, &(from, to))| Edge {
                link_id: i as u32,
                from,
                to,
            })
            .collect();
        Network::from_edges(4, &edges).unwrap()
    }

    const COSTS: [f64; 4] = [5.0, 3.0, 3.0, 1.0];

    fn table(net: &Network, exceptions: &[TurnException]) -> TurnTable {
        TurnTable::build(net, &[false; 4], exceptions, TurnOptions::default()).unwrap()
    }

    fn demand(od: &[(u32, u32, f64)]) -> Result<OdDemand, AssignmentError> {
        let o: Vec<u32> = od.iter().map(|x| x.0).collect();
        let d: Vec<u32> = od.iter().map(|x| x.1).collect();
        let q: Vec<f64> = od.iter().map(|x| x.2).collect();
        OdDemand::from_triples(4, &o, &d, &q)
    }

    #[test]
    fn groups_demand_by_origin() {
        let od = demand(&[(2, 3, 1.0), (0, 3, 2.0), (0, 1, 3.0), (2, 0, 4.0)]).unwrap();
        assert_eq!(od.from_origin(0), (&[3, 1][..], &[2.0, 3.0][..]));
        assert_eq!(od.from_origin(1), (&[][..], &[][..]));
        assert_eq!(od.from_origin(2), (&[3, 0][..], &[1.0, 4.0][..]));
        assert_eq!(od.total(), 10.0);
    }

    #[test]
    fn rejects_bad_demand() {
        assert_eq!(
            demand(&[(0, 9, 1.0)]).unwrap_err(),
            AssignmentError::NodeOutOfRange {
                node: 9,
                num_nodes: 4
            }
        );
        assert_eq!(
            demand(&[(0, 3, -1.0)]).unwrap_err(),
            AssignmentError::InvalidFlow {
                origin: 0,
                destination: 3
            }
        );
        assert_eq!(
            OdDemand::from_triples(4, &[0], &[1, 2], &[1.0]).unwrap_err(),
            AssignmentError::MismatchedLengths
        );
    }

    #[test]
    fn flows_follow_the_cheapest_legal_path() {
        let net = graph();
        // Ban A->J->K: the 10 trips from A to K must take A->X->J->K.
        let ban = TurnException {
            from_link: 0,
            to_link: 3,
            rule: TurnRule::Ban,
        };
        let turns = table(&net, &[ban]);
        let od = demand(&[(0, 3, 10.0)]).unwrap();
        let flows =
            all_or_nothing(&net, &turns, &COSTS, &vec![0.0; turns.num_turns()], &od).unwrap();
        assert_eq!(flows.link_flow, [0.0, 10.0, 10.0, 10.0]);
        let t12 = turns.turn_between(1, 2).unwrap();
        let t23 = turns.turn_between(2, 3).unwrap();
        assert_eq!(flows.turn_flow[t12], 10.0);
        assert_eq!(flows.turn_flow[t23], 10.0);
        assert_eq!(flows.turn_flow.iter().sum::<f64>(), 20.0);
    }

    #[test]
    fn flows_add_up_over_od_pairs() {
        let net = graph();
        let turns = table(&net, &[]);
        // A->K by the direct route (cost 6), plus A->J (cost 5) on the same first link.
        let od = demand(&[(0, 3, 4.0), (0, 2, 1.0)]).unwrap();
        let flows =
            all_or_nothing(&net, &turns, &COSTS, &vec![0.0; turns.num_turns()], &od).unwrap();
        assert_eq!(flows.link_flow, [5.0, 0.0, 0.0, 4.0]);
    }

    #[test]
    fn intrazonal_and_zero_flow_are_skipped_unreachable_is_an_error() {
        let net = graph();
        let turns = table(&net, &[]);
        let zero = vec![0.0; turns.num_turns()];

        let od = demand(&[(0, 0, 5.0), (0, 3, 0.0)]).unwrap();
        let flows = all_or_nothing(&net, &turns, &COSTS, &zero, &od).unwrap();
        assert_eq!(flows.link_flow, [0.0; 4]);

        // K has no outgoing links.
        let od = demand(&[(3, 0, 1.0)]).unwrap();
        assert_eq!(
            all_or_nothing(&net, &turns, &COSTS, &zero, &od).unwrap_err(),
            AssignmentError::Unreachable {
                origin: 3,
                destination: 0
            }
        );
    }
}
