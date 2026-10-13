//! Path storage for path-based assignment.
//!
//! One block per origin, because the solver works origin by origin. Inside a
//! block, all link ids of all paths sit back to back in one arena, and the turn
//! ids of all paths in a second one; a path is a range into each plus its flow.
//! A path of `len` links has `len - 1` turns.

#[derive(Debug, Clone, Copy)]
pub(crate) struct PathRef {
    pub(crate) start: u32,
    pub(crate) len: u32,
    pub(crate) turn_start: u32,
    pub(crate) flow: f64,
}

/// All paths of one OD pair.
#[derive(Debug)]
pub(crate) struct OdPaths {
    pub(crate) dest: u32,
    pub(crate) demand: f64,
    pub(crate) paths: Vec<PathRef>,
}

#[derive(Debug, Default)]
pub(crate) struct OriginPaths {
    pub(crate) links: Vec<u32>,
    pub(crate) turns: Vec<u32>,
    pub(crate) ods: Vec<OdPaths>,
    /// Link arena entries no longer used by any path.
    holes: usize,
}

impl OriginPaths {
    pub(crate) fn path_links(&self, path: &PathRef) -> &[u32] {
        &self.links[path.start as usize..(path.start + path.len) as usize]
    }

    pub(crate) fn path_turns(&self, path: &PathRef) -> &[u32] {
        let end = path.turn_start + path.len.saturating_sub(1);
        &self.turns[path.turn_start as usize..end as usize]
    }

    /// Adds an OD pair with no paths yet; returns its index.
    pub(crate) fn add_od(&mut self, dest: u32, demand: f64) -> usize {
        self.ods.push(OdPaths {
            dest,
            demand,
            paths: Vec::new(),
        });
        self.ods.len() - 1
    }

    /// Index of `links` among the paths of OD `od`, if it is already there.
    /// An OD has few paths, so a scan comparing lengths and then contents is enough.
    pub(crate) fn find_path(&self, od: usize, links: &[u32]) -> Option<usize> {
        self.ods[od]
            .paths
            .iter()
            .position(|p| p.len as usize == links.len() && self.path_links(p) == links)
    }

    /// Adds a path with zero flow and returns its index. `turns` must be the turn ids
    /// between consecutive `links` (one fewer than the links).
    pub(crate) fn add_path(&mut self, od: usize, links: &[u32], turns: &[u32]) -> usize {
        debug_assert_eq!(turns.len(), links.len().saturating_sub(1));
        let start = u32::try_from(self.links.len()).expect("more than u32::MAX arena entries");
        let turn_start = u32::try_from(self.turns.len()).expect("more than u32::MAX arena entries");
        self.links.extend_from_slice(links);
        self.turns.extend_from_slice(turns);
        let paths = &mut self.ods[od].paths;
        paths.push(PathRef {
            start,
            len: links.len() as u32,
            turn_start,
            flow: 0.0,
        });
        paths.len() - 1
    }

    /// Drops the paths of OD `od` that carry no flow.
    pub(crate) fn remove_empty_paths(&mut self, od: usize) {
        let paths = &mut self.ods[od].paths;
        let mut freed = 0;
        paths.retain(|p| {
            let keep = p.flow > 0.0;
            if !keep {
                freed += p.len as usize;
            }
            keep
        });
        self.holes += freed;
    }

    /// Rebuilds the arena without holes once more than half of it is dead.
    pub(crate) fn compact_if_needed(&mut self) {
        if self.holes * 2 <= self.links.len() {
            return;
        }
        let mut packed = Vec::with_capacity(self.links.len() - self.holes);
        let mut packed_turns = Vec::new();
        for od in &mut self.ods {
            for path in &mut od.paths {
                let range = path.start as usize..(path.start + path.len) as usize;
                let turn_range = path.turn_start as usize
                    ..(path.turn_start + path.len.saturating_sub(1)) as usize;
                path.start = packed.len() as u32;
                path.turn_start = packed_turns.len() as u32;
                packed.extend_from_slice(&self.links[range]);
                packed_turns.extend_from_slice(&self.turns[turn_range]);
            }
        }
        self.links = packed;
        self.turns = packed_turns;
        self.holes = 0;
    }
}

/// Paths of every origin, indexed by origin node.
#[derive(Debug)]
pub struct PathStore {
    pub(crate) origins: Vec<OriginPaths>,
}

impl PathStore {
    pub(crate) fn new(num_nodes: usize) -> PathStore {
        PathStore {
            origins: (0..num_nodes).map(|_| OriginPaths::default()).collect(),
        }
    }

    pub fn num_paths(&self) -> usize {
        self.origins
            .iter()
            .flat_map(|o| &o.ods)
            .map(|od| od.paths.len())
            .sum()
    }

    /// Every path as `(origin, destination, link ids, flow)`.
    pub fn iter(&self) -> impl Iterator<Item = (u32, u32, &[u32], f64)> {
        self.iter_with_turns()
            .map(|(origin, dest, links, _, flow)| (origin, dest, links, flow))
    }

    /// Every path as `(origin, destination, link ids, turn ids, flow)`.
    pub fn iter_with_turns(&self) -> impl Iterator<Item = (u32, u32, &[u32], &[u32], f64)> {
        self.origins.iter().enumerate().flat_map(|(origin, block)| {
            block.ods.iter().flat_map(move |od| {
                od.paths.iter().map(move |p| {
                    (
                        origin as u32,
                        od.dest,
                        block.path_links(p),
                        block.path_turns(p),
                        p.flow,
                    )
                })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Adds `links` (with made-up turn ids) unless already present; returns its index.
    fn find_or_add(block: &mut OriginPaths, od: usize, links: &[u32]) -> usize {
        if let Some(index) = block.find_path(od, links) {
            return index;
        }
        let turns: Vec<u32> = links.windows(2).map(|w| 100 * w[0] + w[1]).collect();
        block.add_path(od, links, &turns)
    }

    #[test]
    fn dedups_paths_within_an_od() {
        let mut block = OriginPaths::default();
        let od = block.add_od(5, 10.0);
        let a = find_or_add(&mut block, od, &[1, 2, 3]);
        let b = find_or_add(&mut block, od, &[1, 4, 3]);
        let again = find_or_add(&mut block, od, &[1, 2, 3]);
        assert_eq!((a, b, again), (0, 1, 0));
        assert_eq!(block.links.len(), 6);
        assert_eq!(block.turns.len(), 4);
        assert_eq!(block.path_links(&block.ods[od].paths[1]), &[1, 4, 3]);
        assert_eq!(block.path_turns(&block.ods[od].paths[1]), &[104, 403]);
    }

    #[test]
    fn same_links_in_another_od_are_a_separate_path() {
        let mut block = OriginPaths::default();
        let od1 = block.add_od(5, 1.0);
        let od2 = block.add_od(6, 1.0);
        find_or_add(&mut block, od1, &[1, 2]);
        assert_eq!(find_or_add(&mut block, od2, &[1, 2]), 0);
        assert_eq!(block.ods[od2].paths.len(), 1);
    }

    #[test]
    fn single_link_paths_have_no_turns() {
        let mut block = OriginPaths::default();
        let od = block.add_od(5, 1.0);
        let p = find_or_add(&mut block, od, &[9]);
        assert_eq!(block.path_turns(&block.ods[od].paths[p]), &[] as &[u32]);
    }

    #[test]
    fn removes_empty_paths_and_compacts_both_arenas() {
        let mut block = OriginPaths::default();
        let od = block.add_od(5, 10.0);
        for (i, links) in [[1u32, 2, 3], [4, 5, 6], [7, 8, 9]].iter().enumerate() {
            let p = find_or_add(&mut block, od, links);
            block.ods[od].paths[p].flow = if i == 1 { 0.0 } else { 5.0 };
        }
        block.remove_empty_paths(od);
        assert_eq!(block.ods[od].paths.len(), 2);
        assert_eq!(block.holes, 3);

        block.compact_if_needed(); // 3 of 9 dead: not enough
        assert_eq!(block.links.len(), 9);

        // Kill another path: 6 of 9 dead, so compaction runs.
        block.ods[od].paths[0].flow = 0.0;
        block.remove_empty_paths(od);
        block.compact_if_needed();
        assert_eq!(block.links, [7, 8, 9]);
        assert_eq!(block.turns, [708, 809]);
        let path = block.ods[od].paths[0];
        assert_eq!(block.path_links(&path), &[7, 8, 9]);
        assert_eq!(block.path_turns(&path), &[708, 809]);
        assert_eq!(block.holes, 0);
    }

    #[test]
    fn store_iterates_all_paths() {
        let mut store = PathStore::new(3);
        let od = store.origins[1].add_od(2, 4.0);
        let p = find_or_add(&mut store.origins[1], od, &[0, 7]);
        store.origins[1].ods[od].paths[p].flow = 4.0;
        assert_eq!(store.num_paths(), 1);
        let all: Vec<_> = store.iter().collect();
        assert_eq!(all, vec![(1, 2, &[0u32, 7][..], 4.0)]);
        let with_turns: Vec<_> = store.iter_with_turns().collect();
        assert_eq!(with_turns, vec![(1, 2, &[0u32, 7][..], &[7u32][..], 4.0)]);
    }
}
