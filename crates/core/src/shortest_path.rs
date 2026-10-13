use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::{Network, TurnTable};

/// Marks "no predecessor" (the origin itself, or an unreachable node).
pub const NO_PRED: u32 = u32::MAX;

/// Shortest-path tree from one origin. All vectors are indexed by node.
#[derive(Debug)]
pub struct ShortestPathTree {
    /// Cost from the origin; `f64::INFINITY` if unreachable.
    pub dist: Vec<f64>,
    /// Node before this one on the shortest path.
    pub pred_node: Vec<u32>,
    /// Link used to arrive at this node.
    pub pred_link: Vec<u32>,
}

impl ShortestPathTree {
    /// Link ids of the shortest path from the origin to `dest`, in travel order.
    /// `None` if `dest` is unreachable. The path to the origin itself is empty.
    pub fn path_to(&self, dest: u32) -> Option<Vec<u32>> {
        if self.dist[dest as usize].is_infinite() {
            return None;
        }
        let mut links = Vec::new();
        let mut node = dest;
        while self.pred_link[node as usize] != NO_PRED {
            links.push(self.pred_link[node as usize]);
            node = self.pred_node[node as usize];
        }
        links.reverse();
        Some(links)
    }
}

/// Heap entry: a node id (node-based search) or a link id (link-labelled search).
/// `BinaryHeap` is a max-heap and `f64` is not `Ord` (NaN), so we wrap them and
/// define the order ourselves: reversed, so the smallest cost pops first.
#[derive(Debug, PartialEq)]
struct HeapEntry {
    cost: f64,
    id: u32,
}

impl Eq for HeapEntry {}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.total_cmp(&self.cost)
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Dijkstra from `origin`. `costs` is indexed by link id and must be non-negative.
///
/// `is_endpoint` is indexed by node. A path may start or end at an endpoint node
/// but never passes through one, so we do not relax out of it unless it is the
/// origin. Pass all `false` when every node may be passed through.
pub fn dijkstra(
    network: &Network,
    costs: &[f64],
    origin: u32,
    is_endpoint: &[bool],
) -> ShortestPathTree {
    let n = network.num_nodes();
    debug_assert_eq!(is_endpoint.len(), n);
    let mut tree = ShortestPathTree {
        dist: vec![f64::INFINITY; n],
        pred_node: vec![NO_PRED; n],
        pred_link: vec![NO_PRED; n],
    };

    let mut heap = BinaryHeap::new();
    tree.dist[origin as usize] = 0.0;
    heap.push(HeapEntry {
        cost: 0.0,
        id: origin,
    });

    while let Some(HeapEntry { cost, id: node }) = heap.pop() {
        // A node can be pushed several times; only the cheapest entry is current.
        if cost > tree.dist[node as usize] {
            continue;
        }
        if is_endpoint[node as usize] && node != origin {
            continue;
        }
        for pos in network.out_links(node) {
            let link = network.link_id(pos);
            let next = network.target(pos);
            let new_cost = cost + costs[link as usize];
            if new_cost < tree.dist[next as usize] {
                tree.dist[next as usize] = new_cost;
                tree.pred_node[next as usize] = node;
                tree.pred_link[next as usize] = link;
                heap.push(HeapEntry {
                    cost: new_cost,
                    id: next,
                });
            }
        }
    }
    tree
}

/// Turn-aware shortest-path search with reusable buffers.
///
/// Labels sit on links: `label[e]` is the cheapest cost to reach the END of link `e`,
/// including the cost of `e` itself and of every turn on the way. Create one with
/// [`TurnSearch::new`], call [`TurnSearch::run`] once per origin (the buffers are reused,
/// so there is no allocation after the first call), and read the answers from it.
/// [`dijkstra_turns`] is the one-shot version.
#[derive(Debug)]
pub struct TurnSearch {
    origin: u32,
    /// Indexed by link id; `f64::INFINITY` if the link cannot be reached.
    label: Vec<f64>,
    /// Link before this one on the best path; `NO_PRED` for the origin's first links.
    pred_link: Vec<u32>,
    /// Per node: cheapest arrival cost and the link that achieves it.
    node_dist: Vec<f64>,
    arrival_link: Vec<u32>,
    heap: BinaryHeap<HeapEntry>,
}

/// Result of [`dijkstra_turns`]: a finished [`TurnSearch`].
pub type LinkTree = TurnSearch;

impl TurnSearch {
    /// A search sized for `network`.
    pub fn new(network: &Network) -> TurnSearch {
        TurnSearch {
            origin: 0,
            label: vec![f64::INFINITY; network.num_links()],
            pred_link: vec![NO_PRED; network.num_links()],
            node_dist: vec![f64::INFINITY; network.num_nodes()],
            arrival_link: vec![NO_PRED; network.num_nodes()],
            heap: BinaryHeap::new(),
        }
    }

    /// Searches from `origin`, replacing the previous result.
    ///
    /// `link_costs` is indexed by link id and `turn_costs` by turn id of `turns`; both must
    /// be non-negative (infinite means "cannot use"). Moving along link `e` then link `f`
    /// costs `turn_costs[turn(e, f)] + link_costs[f]`, and banned turns are simply absent
    /// from `turns`. The origin's own outgoing links start the search with no turn cost,
    /// since nothing was entered before them.
    ///
    /// Endpoint nodes need no flag here: the turn table has no turns after a link that
    /// enters one, so the search stops there by construction.
    pub fn run(
        &mut self,
        network: &Network,
        turns: &TurnTable,
        link_costs: &[f64],
        turn_costs: &[f64],
        origin: u32,
    ) {
        debug_assert_eq!(link_costs.len(), network.num_links());
        debug_assert_eq!(turn_costs.len(), turns.num_turns());
        debug_assert_eq!(self.label.len(), network.num_links());
        self.origin = origin;
        self.label.fill(f64::INFINITY);
        self.pred_link.fill(NO_PRED);
        self.node_dist.fill(f64::INFINITY);
        self.arrival_link.fill(NO_PRED);
        self.heap.clear();

        for pos in network.out_links(origin) {
            let link = network.link_id(pos);
            self.label[link as usize] = link_costs[link as usize];
            self.heap.push(HeapEntry {
                cost: link_costs[link as usize],
                id: link,
            });
        }

        while let Some(HeapEntry { cost, id: e }) = self.heap.pop() {
            if cost > self.label[e as usize] {
                continue; // stale heap entry
            }
            for t in turns.turns_from(e) {
                let f = turns.to_link(t);
                let new_cost = cost + turn_costs[t] + link_costs[f as usize];
                if new_cost < self.label[f as usize] {
                    self.label[f as usize] = new_cost;
                    self.pred_link[f as usize] = e;
                    self.heap.push(HeapEntry {
                        cost: new_cost,
                        id: f,
                    });
                }
            }
        }

        // Fold link labels into per-node arrival information.
        self.node_dist[origin as usize] = 0.0;
        for link in 0..network.num_links() as u32 {
            let node = network.to_node(link) as usize;
            if self.label[link as usize] < self.node_dist[node] {
                self.node_dist[node] = self.label[link as usize];
                self.arrival_link[node] = link;
            }
        }
    }

    pub fn origin(&self) -> u32 {
        self.origin
    }

    /// Cost to reach the end of `link_id` (infinite if unreachable).
    pub fn label(&self, link_id: u32) -> f64 {
        self.label[link_id as usize]
    }

    /// Cheapest cost from the origin to `node` over all ways of arriving there.
    pub fn node_dist(&self, node: u32) -> f64 {
        self.node_dist[node as usize]
    }

    /// The link the best path to `node` arrives on. `None` for the origin itself
    /// and for unreachable nodes.
    pub fn arrival_link(&self, node: u32) -> Option<u32> {
        match self.arrival_link[node as usize] {
            NO_PRED => None,
            link => Some(link),
        }
    }

    /// Writes the link ids of the best path from the origin to `node` into `out`, in
    /// travel order (replacing its contents), and returns `true`. Returns `false` if
    /// `node` is unreachable. The path to the origin itself is empty.
    pub fn path_into(&self, node: u32, out: &mut Vec<u32>) -> bool {
        out.clear();
        if self.node_dist[node as usize].is_infinite() {
            return false;
        }
        let mut link = self.arrival_link[node as usize];
        while link != NO_PRED {
            out.push(link);
            link = self.pred_link[link as usize];
        }
        out.reverse();
        true
    }

    /// Link ids of the best path from the origin to `node`, in travel order.
    /// `None` if `node` is unreachable. The path to the origin itself is empty.
    pub fn path_to(&self, node: u32) -> Option<Vec<u32>> {
        let mut links = Vec::new();
        self.path_into(node, &mut links).then_some(links)
    }
}

/// One-shot turn-aware Dijkstra from `origin`; see [`TurnSearch::run`] for the details.
/// Use a [`TurnSearch`] directly when searching from many origins.
pub fn dijkstra_turns(
    network: &Network,
    turns: &TurnTable,
    link_costs: &[f64],
    turn_costs: &[f64],
    origin: u32,
) -> LinkTree {
    let mut search = TurnSearch::new(network);
    search.run(network, turns, link_costs, turn_costs, origin);
    search
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Edge, TurnException, TurnOptions, TurnRule};

    fn network(num_nodes: usize, edges: &[(u32, u32)]) -> Network {
        let edges: Vec<Edge> = edges
            .iter()
            .enumerate()
            .map(|(i, &(from, to))| Edge {
                link_id: i as u32,
                from,
                to,
            })
            .collect();
        Network::from_edges(num_nodes, &edges).unwrap()
    }

    #[test]
    fn picks_cheaper_two_hop_route() {
        // 0->1 (cost 5), 0->2 (cost 1), 2->1 (cost 1): best way to 1 is via 2.
        let net = network(3, &[(0, 1), (0, 2), (2, 1)]);
        let tree = dijkstra(&net, &[5.0, 1.0, 1.0], 0, &[false; 3]);
        assert_eq!(tree.dist, [0.0, 2.0, 1.0]);
        assert_eq!(tree.path_to(1), Some(vec![1, 2]));
        assert_eq!(tree.path_to(0), Some(vec![]));
    }

    #[test]
    fn unreachable_node() {
        let net = network(3, &[(0, 1)]);
        let tree = dijkstra(&net, &[1.0], 0, &[false; 3]);
        assert!(tree.dist[2].is_infinite());
        assert_eq!(tree.path_to(2), None);
    }

    #[test]
    fn paths_do_not_pass_through_endpoints() {
        // Nodes 0 and 1 are endpoint-only nodes, 2 and 3 are ordinary nodes.
        // The shortcut 0->1->3 (cost 2) goes through endpoint 1 and is not allowed;
        // the legal route 0->2->3 costs 10.
        let net = network(4, &[(0, 1), (1, 3), (0, 2), (2, 3)]);
        let costs = [1.0, 1.0, 5.0, 5.0];
        let tree = dijkstra(&net, &costs, 0, &[true, true, false, false]);
        assert_eq!(tree.dist[1], 1.0); // an endpoint can still be a destination
        assert_eq!(tree.dist[3], 10.0);
        assert_eq!(tree.path_to(3), Some(vec![2, 3]));
    }

    // The banned-turn example: A=0 X=1 J=2 K=3.
    // 0: A->J (5), 1: A->X (3), 2: X->J (3), 3: J->K (1).
    fn banned_turn_graph() -> Network {
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

    const BANNED_COSTS: [f64; 4] = [5.0, 3.0, 3.0, 1.0];

    fn turn_table(net: &Network, rule: Option<TurnRule>) -> TurnTable {
        let exceptions: Vec<TurnException> = rule
            .map(|rule| TurnException {
                from_link: 0,
                to_link: 3,
                rule,
            })
            .into_iter()
            .collect();
        TurnTable::build(net, &[false; 4], &exceptions, TurnOptions::default()).unwrap()
    }

    #[test]
    fn banned_turn_forces_the_longer_route() {
        let net = banned_turn_graph();

        // Node-based Dijkstra cannot see the ban and returns the illegal route A-J-K.
        let node_based = dijkstra(&net, &BANNED_COSTS, 0, &[false; 4]);
        assert_eq!(node_based.dist[3], 6.0);
        assert_eq!(node_based.path_to(3), Some(vec![0, 3]));

        // Link-labelled: A->J->K is banned, so the route is A->X->J->K = 3 + 3 + 1.
        let turns = turn_table(&net, Some(TurnRule::Ban));
        let tree = dijkstra_turns(
            &net,
            &turns,
            &BANNED_COSTS,
            &vec![0.0; turns.num_turns()],
            0,
        );
        assert_eq!(tree.node_dist(3), 7.0);
        assert_eq!(tree.path_to(3), Some(vec![1, 2, 3]));
        assert_eq!(tree.arrival_link(3), Some(3));
        // J itself is still reached the direct way: the ban only affects the onward turn.
        assert_eq!(tree.node_dist(2), 5.0);
        assert_eq!(tree.path_to(2), Some(vec![0]));
    }

    #[test]
    fn small_penalty_keeps_the_short_route_big_one_does_not() {
        let net = banned_turn_graph();
        let cost_with = |penalty: f64| {
            let turns = turn_table(&net, Some(TurnRule::Penalty(penalty)));
            let turn_costs: Vec<f64> = (0..turns.num_turns()).map(|t| turns.penalty(t)).collect();
            let tree = dijkstra_turns(&net, &turns, &BANNED_COSTS, &turn_costs, 0);
            (tree.node_dist(3), tree.path_to(3).unwrap())
        };
        assert_eq!(cost_with(0.5), (6.5, vec![0, 3]));
        assert_eq!(cost_with(10.0), (7.0, vec![1, 2, 3]));
    }

    #[test]
    fn origin_and_unreachable_nodes() {
        let net = banned_turn_graph();
        let turns = turn_table(&net, None);
        let tree = dijkstra_turns(
            &net,
            &turns,
            &BANNED_COSTS,
            &vec![0.0; turns.num_turns()],
            3,
        );
        // K has no outgoing links: only the origin itself is reachable.
        assert_eq!(tree.node_dist(3), 0.0);
        assert_eq!(tree.path_to(3), Some(vec![]));
        assert_eq!(tree.arrival_link(3), None);
        assert!(tree.node_dist(0).is_infinite());
        assert_eq!(tree.path_to(0), None);
        assert_eq!(tree.label(0), f64::INFINITY);
    }

    #[test]
    fn paths_stop_at_endpoint_nodes() {
        // Same graph, but J (node 2) is endpoint-only: K can no longer be reached from A.
        let net = banned_turn_graph();
        let turns = TurnTable::build(
            &net,
            &[false, false, true, false],
            &[],
            TurnOptions::default(),
        )
        .unwrap();
        let tree = dijkstra_turns(
            &net,
            &turns,
            &BANNED_COSTS,
            &vec![0.0; turns.num_turns()],
            0,
        );
        assert_eq!(tree.node_dist(2), 5.0); // J can still be a destination
        assert!(tree.node_dist(3).is_infinite());
    }
}
