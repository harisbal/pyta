use std::ops::Range;

/// Immutable directed graph in CSR (compressed sparse row) form.
///
/// Links are stored sorted by from-node. The outgoing links of node `n` occupy
/// positions `offsets[n]..offsets[n + 1]` in `targets` and `link_ids`.
/// Costs and flows are NOT stored here: they live in separate arrays indexed by
/// link id, so the topology never changes during assignment. For that to work,
/// link ids are dense: a permutation of `0..num_links`.
#[derive(Debug)]
pub struct Network {
    offsets: Vec<u32>,
    targets: Vec<u32>,
    link_ids: Vec<u32>,
    // Indexed by link id, not by CSR position.
    from_nodes: Vec<u32>,
    to_nodes: Vec<u32>,
}

/// A directed link as given by the caller. `link_id` is explicit and need not
/// match the position in the input slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub link_id: u32,
    pub from: u32,
    pub to: u32,
}

#[derive(Debug, PartialEq)]
pub enum NetworkError {
    NodeOutOfRange {
        link_id: u32,
        node: u32,
        num_nodes: usize,
    },
    LinkIdOutOfRange {
        link_id: u32,
        num_links: usize,
    },
    DuplicateLinkId {
        link_id: u32,
    },
}

impl Network {
    /// Builds the CSR arrays from `edges`. Link ids must be unique and form
    /// exactly the range `0..edges.len()`, in any order.
    pub fn from_edges(num_nodes: usize, edges: &[Edge]) -> Result<Network, NetworkError> {
        // Pass 1: validate and count the out-degree of every node.
        // offsets[n + 1] holds the degree of n for now (shifted by one on purpose).
        // `seen` catches duplicate ids; with ids < num_links and no duplicates,
        // the ids are exactly 0..num_links.
        let num_links = edges.len();
        let mut seen = vec![false; num_links];
        let mut offsets = vec![0u32; num_nodes + 1];
        for edge in edges {
            let link_id = edge.link_id;
            if link_id as usize >= num_links {
                return Err(NetworkError::LinkIdOutOfRange { link_id, num_links });
            }
            if seen[link_id as usize] {
                return Err(NetworkError::DuplicateLinkId { link_id });
            }
            seen[link_id as usize] = true;
            for node in [edge.from, edge.to] {
                if node as usize >= num_nodes {
                    return Err(NetworkError::NodeOutOfRange {
                        link_id,
                        node,
                        num_nodes,
                    });
                }
            }
            offsets[edge.from as usize + 1] += 1;
        }

        // Pass 2: prefix sum turns degrees into start positions.
        for n in 0..num_nodes {
            offsets[n + 1] += offsets[n];
        }

        // Pass 3: place each link at the next free slot of its from-node.
        let mut next = offsets.clone();
        let mut targets = vec![0u32; edges.len()];
        let mut link_ids = vec![0u32; edges.len()];
        let mut from_nodes = vec![0u32; edges.len()];
        let mut to_nodes = vec![0u32; edges.len()];
        for edge in edges {
            from_nodes[edge.link_id as usize] = edge.from;
            to_nodes[edge.link_id as usize] = edge.to;
            let pos = next[edge.from as usize] as usize;
            targets[pos] = edge.to;
            link_ids[pos] = edge.link_id;
            next[edge.from as usize] += 1;
        }

        Ok(Network {
            offsets,
            targets,
            link_ids,
            from_nodes,
            to_nodes,
        })
    }

    pub fn num_nodes(&self) -> usize {
        self.offsets.len() - 1
    }

    pub fn num_links(&self) -> usize {
        self.targets.len()
    }

    /// CSR positions of the outgoing links of `node`.
    pub fn out_links(&self, node: u32) -> Range<usize> {
        self.offsets[node as usize] as usize..self.offsets[node as usize + 1] as usize
    }

    /// To-node of the link at CSR position `pos`.
    pub fn target(&self, pos: usize) -> u32 {
        self.targets[pos]
    }

    /// From-node of the link with this id.
    pub fn from_node(&self, link_id: u32) -> u32 {
        self.from_nodes[link_id as usize]
    }

    /// To-node of the link with this id.
    pub fn to_node(&self, link_id: u32) -> u32 {
        self.to_nodes[link_id as usize]
    }

    /// Link id of CSR position `pos`: the index into per-link cost/flow arrays.
    pub fn link_id(&self, pos: usize) -> u32 {
        self.link_ids[pos]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(link_id: u32, from: u32, to: u32) -> Edge {
        Edge { link_id, from, to }
    }

    // A=0 B=1 C=2 D=3; links 0:A->B 1:A->C 2:B->C 3:B->D 4:C->D
    fn example() -> Network {
        let edges = [
            edge(0, 0, 1),
            edge(1, 0, 2),
            edge(2, 1, 2),
            edge(3, 1, 3),
            edge(4, 2, 3),
        ];
        Network::from_edges(4, &edges).unwrap()
    }

    #[test]
    fn offsets_and_targets() {
        let net = example();
        assert_eq!(net.offsets, [0, 2, 4, 5, 5]);
        assert_eq!(net.targets, [1, 2, 2, 3, 3]);
        assert_eq!(net.link_ids, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn nodes_of_a_link_by_id() {
        let edges = [edge(1, 0, 2), edge(0, 1, 3)];
        let net = Network::from_edges(4, &edges).unwrap();
        assert_eq!((net.from_node(0), net.to_node(0)), (1, 3));
        assert_eq!((net.from_node(1), net.to_node(1)), (0, 2));
    }

    #[test]
    fn out_links_of_b() {
        let net = example();
        assert_eq!(net.out_links(1), 2..4);
        assert_eq!(net.out_links(3), 5..5); // D has no outgoing links
    }

    #[test]
    fn explicit_ids_in_shuffled_input() {
        // Same graph; both the list order and the ids are shuffled.
        let edges = [
            edge(3, 2, 3),
            edge(1, 0, 2),
            edge(4, 1, 3),
            edge(0, 0, 1),
            edge(2, 1, 2),
        ];
        let net = Network::from_edges(4, &edges).unwrap();
        assert_eq!(net.offsets, [0, 2, 4, 5, 5]);
        assert_eq!(net.targets, [2, 1, 3, 2, 3]);
        assert_eq!(net.link_ids, [1, 0, 4, 2, 3]);
    }

    #[test]
    fn rejects_bad_node() {
        let err = Network::from_edges(2, &[edge(0, 0, 1), edge(1, 1, 5)]).unwrap_err();
        assert_eq!(
            err,
            NetworkError::NodeOutOfRange {
                link_id: 1,
                node: 5,
                num_nodes: 2
            }
        );
    }

    #[test]
    fn rejects_duplicate_link_id() {
        let err = Network::from_edges(2, &[edge(0, 0, 1), edge(0, 1, 0)]).unwrap_err();
        assert_eq!(err, NetworkError::DuplicateLinkId { link_id: 0 });
    }

    #[test]
    fn rejects_link_id_out_of_range() {
        let err = Network::from_edges(2, &[edge(0, 0, 1), edge(7, 1, 0)]).unwrap_err();
        assert_eq!(
            err,
            NetworkError::LinkIdOutOfRange {
                link_id: 7,
                num_links: 2
            }
        );
    }
}
