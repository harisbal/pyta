use std::collections::HashMap;
use std::ops::Range;

use crate::Network;

/// What a user exception says about one turn `(from_link, to_link)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TurnRule {
    /// The turn is not allowed.
    Ban,
    /// The turn is allowed with a fixed extra delay (>= 0). Also re-enables a
    /// U-turn that would otherwise be banned.
    Penalty(f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TurnException {
    pub from_link: u32,
    pub to_link: u32,
    pub rule: TurnRule,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TurnOptions {
    /// Allow immediate returns to the node a link came from. Off by default.
    pub allow_u_turns: bool,
}

#[derive(Debug, PartialEq)]
pub enum TurnError {
    LinkIdOutOfRange {
        link_id: u32,
        num_links: usize,
    },
    /// The links do not meet: `to_node(from_link) != from_node(to_link)`.
    NotAdjacent {
        from_link: u32,
        to_link: u32,
    },
    InvalidPenalty {
        from_link: u32,
        to_link: u32,
    },
    DuplicateException {
        from_link: u32,
        to_link: u32,
    },
    /// The turn does not exist in the table, e.g. because it lies at an endpoint node.
    UnusedException {
        from_link: u32,
        to_link: u32,
    },
    TooManyTurns,
}

/// All allowed turns of a network, in CSR form indexed by incoming link id.
///
/// A turn id is a position in these arrays. The turns of link `e` are the ids in
/// `turns_from(e)`. Turn flows and costs are kept elsewhere in arrays indexed by turn id.
#[derive(Debug)]
pub struct TurnTable {
    offsets: Vec<u32>,
    from_links: Vec<u32>,
    to_links: Vec<u32>,
    penalties: Vec<f64>,
}

impl TurnTable {
    /// Generates every turn `(e, f)` where `f` leaves the node `e` enters, except:
    /// - turns at `is_endpoint` nodes (a path entering an endpoint ends there),
    /// - U-turns, unless `options.allow_u_turns`,
    /// - turns banned in `exceptions`.
    ///
    /// Penalty exceptions set the turn's fixed delay. An explicit penalty on a
    /// U-turn allows it even when U-turns are off by default.
    pub fn build(
        network: &Network,
        is_endpoint: &[bool],
        exceptions: &[TurnException],
        options: TurnOptions,
    ) -> Result<TurnTable, TurnError> {
        let num_links = network.num_links();
        assert_eq!(is_endpoint.len(), network.num_nodes());
        let mut rules = collect_rules(network, exceptions)?;

        let mut offsets = Vec::with_capacity(num_links + 1);
        let mut from_links = Vec::new();
        let mut to_links = Vec::new();
        let mut penalties = Vec::new();

        for e in 0..num_links as u32 {
            offsets.push(u32::try_from(to_links.len()).map_err(|_| TurnError::TooManyTurns)?);
            let junction = network.to_node(e);
            if is_endpoint[junction as usize] {
                continue;
            }
            for pos in network.out_links(junction) {
                let f = network.link_id(pos);
                let rule = rules.remove(&(e, f));
                if rule == Some(TurnRule::Ban) {
                    continue;
                }
                let is_u_turn = network.to_node(f) == network.from_node(e);
                if is_u_turn && !options.allow_u_turns && rule.is_none() {
                    continue;
                }
                from_links.push(e);
                to_links.push(f);
                penalties.push(match rule {
                    Some(TurnRule::Penalty(p)) => p,
                    _ => 0.0,
                });
            }
        }
        offsets.push(u32::try_from(to_links.len()).map_err(|_| TurnError::TooManyTurns)?);

        // Anything left over named a turn that was never generated.
        if let Some(&(from_link, to_link)) = rules.keys().min() {
            return Err(TurnError::UnusedException { from_link, to_link });
        }
        Ok(TurnTable {
            offsets,
            from_links,
            to_links,
            penalties,
        })
    }

    pub fn num_turns(&self) -> usize {
        self.to_links.len()
    }

    /// Turn ids that start with incoming link `link_id`.
    pub fn turns_from(&self, link_id: u32) -> Range<usize> {
        self.offsets[link_id as usize] as usize..self.offsets[link_id as usize + 1] as usize
    }

    /// Turn id of moving from `from_link` straight onto `to_link`, if that turn exists.
    /// Scans the few turns of `from_link`, so it is cheap.
    pub fn turn_between(&self, from_link: u32, to_link: u32) -> Option<usize> {
        self.turns_from(from_link)
            .find(|&t| self.to_links[t] == to_link)
    }

    /// Turn ids along a path of link ids: one per consecutive pair, so
    /// `links.len() - 1` of them (none for an empty or single-link path).
    /// `None` if some consecutive pair is not an allowed turn.
    ///
    /// Together with the first link, these ids identify the whole path:
    /// each turn's `to_link` is the next link.
    pub fn path_turns(&self, links: &[u32]) -> Option<Vec<usize>> {
        links
            .windows(2)
            .map(|w| self.turn_between(w[0], w[1]))
            .collect()
    }

    /// Like [`TurnTable::path_turns`], but writes the turn ids as `u32` into `out`
    /// (replacing its contents) and returns `false` if some pair is not an allowed turn.
    pub fn path_turns_into(&self, links: &[u32], out: &mut Vec<u32>) -> bool {
        out.clear();
        for w in links.windows(2) {
            match self.turn_between(w[0], w[1]) {
                Some(t) => out.push(t as u32),
                None => return false,
            }
        }
        true
    }

    pub fn from_link(&self, turn: usize) -> u32 {
        self.from_links[turn]
    }

    pub fn to_link(&self, turn: usize) -> u32 {
        self.to_links[turn]
    }

    /// Fixed delay of the turn (0.0 unless set by an exception).
    pub fn penalty(&self, turn: usize) -> f64 {
        self.penalties[turn]
    }
}

/// Validates the exceptions and indexes them by `(from_link, to_link)`.
fn collect_rules(
    network: &Network,
    exceptions: &[TurnException],
) -> Result<HashMap<(u32, u32), TurnRule>, TurnError> {
    let num_links = network.num_links();
    let mut rules = HashMap::new();
    for ex in exceptions {
        let (from_link, to_link) = (ex.from_link, ex.to_link);
        for link_id in [from_link, to_link] {
            if link_id as usize >= num_links {
                return Err(TurnError::LinkIdOutOfRange { link_id, num_links });
            }
        }
        if network.to_node(from_link) != network.from_node(to_link) {
            return Err(TurnError::NotAdjacent { from_link, to_link });
        }
        if let TurnRule::Penalty(p) = ex.rule
            && (!p.is_finite() || p < 0.0)
        {
            return Err(TurnError::InvalidPenalty { from_link, to_link });
        }
        if rules.insert((from_link, to_link), ex.rule).is_some() {
            return Err(TurnError::DuplicateException { from_link, to_link });
        }
    }
    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Edge;

    // Path graph 0 - 1 - 2 with both directions:
    // link 0: 0->1, link 1: 1->0, link 2: 1->2, link 3: 2->1.
    fn path_graph() -> Network {
        let edges = [(0, 1), (1, 0), (1, 2), (2, 1)]
            .iter()
            .enumerate()
            .map(|(i, &(from, to))| Edge {
                link_id: i as u32,
                from,
                to,
            })
            .collect::<Vec<_>>();
        Network::from_edges(3, &edges).unwrap()
    }

    const NO_ENDPOINTS: [bool; 3] = [false; 3];

    fn turns(table: &TurnTable) -> Vec<(u32, u32)> {
        (0..table.num_turns())
            .map(|t| (table.from_link(t), table.to_link(t)))
            .collect()
    }

    fn build(exceptions: &[TurnException], options: TurnOptions) -> Result<TurnTable, TurnError> {
        TurnTable::build(&path_graph(), &NO_ENDPOINTS, exceptions, options)
    }

    fn ex(from_link: u32, to_link: u32, rule: TurnRule) -> TurnException {
        TurnException {
            from_link,
            to_link,
            rule,
        }
    }

    #[test]
    fn u_turns_are_banned_by_default() {
        // 0->1 may continue to 1->2 (not back to 0); 3 (2->1) may continue to 1->0.
        let table = build(&[], TurnOptions::default()).unwrap();
        assert_eq!(turns(&table), [(0, 2), (3, 1)]);
        assert_eq!(table.turns_from(0), 0..1);
        assert_eq!(table.turns_from(1), 1..1); // 1->0 enters node 0, a dead end
        assert_eq!(table.turns_from(3), 1..2);
    }

    #[test]
    fn u_turns_can_be_allowed() {
        let table = build(
            &[],
            TurnOptions {
                allow_u_turns: true,
            },
        )
        .unwrap();
        assert_eq!(
            turns(&table),
            [(0, 1), (0, 2), (1, 0), (2, 3), (3, 1), (3, 2)]
        );
    }

    #[test]
    fn ban_removes_turn_and_penalty_is_stored() {
        let opts = TurnOptions {
            allow_u_turns: true,
        };
        let table = build(
            &[ex(0, 1, TurnRule::Ban), ex(3, 2, TurnRule::Penalty(7.5))],
            opts,
        )
        .unwrap();
        assert_eq!(turns(&table), [(0, 2), (1, 0), (2, 3), (3, 1), (3, 2)]);
        let t = turns(&table).iter().position(|&p| p == (3, 2)).unwrap();
        assert_eq!(table.penalty(t), 7.5);
        assert_eq!(table.penalty(0), 0.0);
    }

    #[test]
    fn explicit_penalty_enables_a_banned_u_turn() {
        let table = build(&[ex(0, 1, TurnRule::Penalty(30.0))], TurnOptions::default()).unwrap();
        assert_eq!(turns(&table), [(0, 1), (0, 2), (3, 1)]);
        assert_eq!(table.penalty(0), 30.0);
    }

    #[test]
    fn no_turns_at_endpoint_nodes() {
        // Node 1 is endpoint-only: entering it (links 0 and 3) ends the path.
        let table = TurnTable::build(
            &path_graph(),
            &[false, true, false],
            &[],
            TurnOptions {
                allow_u_turns: true,
            },
        )
        .unwrap();
        assert_eq!(turns(&table), [(1, 0), (2, 3)]);
        assert_eq!(table.turns_from(0), 0..0);
        assert_eq!(table.turns_from(3), 2..2);
    }

    #[test]
    fn rejects_bad_exceptions() {
        let o = TurnOptions::default();
        assert_eq!(
            build(&[ex(0, 9, TurnRule::Ban)], o).unwrap_err(),
            TurnError::LinkIdOutOfRange {
                link_id: 9,
                num_links: 4
            }
        );
        // Link 0 ends at node 1 but link 3 starts at node 2.
        assert_eq!(
            build(&[ex(0, 3, TurnRule::Ban)], o).unwrap_err(),
            TurnError::NotAdjacent {
                from_link: 0,
                to_link: 3
            }
        );
        assert_eq!(
            build(&[ex(0, 2, TurnRule::Penalty(-1.0))], o).unwrap_err(),
            TurnError::InvalidPenalty {
                from_link: 0,
                to_link: 2
            }
        );
        assert_eq!(
            build(
                &[ex(0, 2, TurnRule::Ban), ex(0, 2, TurnRule::Penalty(1.0))],
                o
            )
            .unwrap_err(),
            TurnError::DuplicateException {
                from_link: 0,
                to_link: 2
            }
        );
    }

    #[test]
    fn exception_on_a_turn_that_does_not_exist_is_an_error() {
        let table = TurnTable::build(
            &path_graph(),
            &[false, true, false],
            &[ex(0, 2, TurnRule::Penalty(5.0))], // turn at endpoint node 1
            TurnOptions::default(),
        );
        assert_eq!(
            table.unwrap_err(),
            TurnError::UnusedException {
                from_link: 0,
                to_link: 2
            }
        );
    }

    #[test]
    fn turn_ids_along_a_path() {
        let table = build(&[], TurnOptions::default()).unwrap();
        // Turns are (0,2) = id 0 and (3,1) = id 1.
        assert_eq!(table.turn_between(0, 2), Some(0));
        assert_eq!(table.turn_between(3, 1), Some(1));
        assert_eq!(table.turn_between(0, 1), None); // banned U-turn
        assert_eq!(table.turn_between(0, 3), None); // links do not meet

        assert_eq!(table.path_turns(&[0, 2]), Some(vec![0]));
        assert_eq!(table.path_turns(&[3, 1]), Some(vec![1]));
        assert_eq!(table.path_turns(&[0, 1]), None);
        assert_eq!(table.path_turns(&[2]), Some(vec![]));
        assert_eq!(table.path_turns(&[]), Some(vec![]));
    }
}
