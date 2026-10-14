//! Path storage for path-based assignment.
//!
//! One block per origin, because the solver works origin by origin. Inside a
//! block, paths are stored as a tree of *steps*: a step is "arrive on link `L`,
//! coming from step `P`". The root stands for the origin itself. A path is the
//! step where it ends plus its flow, so paths that start the same way share the
//! steps of that start. Paths from one origin mostly leave it along the same
//! roads, which makes the tree several times smaller than storing every path in full.

const NONE: u32 = u32::MAX;

/// One step of the tree. Steps are only ever added after their parent, so a step's id is
/// always larger than its parent's.
#[derive(Debug, Clone, Copy)]
struct Step {
    parent: u32,
    /// Link this step arrives on.
    link: u32,
    /// Turn from the parent's link onto this link; `NONE` for the first link of a path.
    turn: u32,
    /// Number of links from the origin up to and including this step; the root has 0.
    depth: u32,
    /// Children are a linked list: the first one, then each child's next sibling.
    first_child: u32,
    next_sibling: u32,
}

const ROOT: Step = Step {
    parent: NONE,
    link: NONE,
    turn: NONE,
    depth: 0,
    first_child: NONE,
    next_sibling: NONE,
};

/// The links and turns of the part of a path that is not shared with another path, in no
/// particular order (the tree is walked from the end of the path towards its start).
#[derive(Debug, Default)]
pub(crate) struct Stretch {
    pub(crate) links: Vec<u32>,
    pub(crate) turns: Vec<u32>,
}

impl Stretch {
    fn clear(&mut self) {
        self.links.clear();
        self.turns.clear();
    }
}

/// The steps of all paths of one origin.
#[derive(Debug)]
pub(crate) struct StepTree {
    /// `steps[0]` is the root.
    steps: Vec<Step>,
    /// Number of steps right after the last compaction.
    compacted_len: usize,
}

impl StepTree {
    fn new() -> StepTree {
        StepTree {
            steps: vec![ROOT],
            compacted_len: 1,
        }
    }

    /// The child of `parent` that arrives on `link`, if there is one.
    fn child(&self, parent: u32, link: u32) -> Option<u32> {
        let mut c = self.steps[parent as usize].first_child;
        while c != NONE {
            let step = &self.steps[c as usize];
            if step.link == link {
                return Some(c);
            }
            c = step.next_sibling;
        }
        None
    }

    fn push_step(&mut self, parent: u32, link: u32, turn: u32) -> u32 {
        let id = u32::try_from(self.steps.len()).expect("more than u32::MAX steps");
        assert!(id != NONE, "more than u32::MAX steps");
        let parent_step = &mut self.steps[parent as usize];
        let step = Step {
            parent,
            link,
            turn,
            depth: parent_step.depth + 1,
            first_child: NONE,
            next_sibling: parent_step.first_child,
        };
        parent_step.first_child = id;
        self.steps.push(step);
        id
    }

    /// The last step of the path with these `links`, if the tree has it.
    fn find(&self, links: &[u32]) -> Option<u32> {
        let mut node = 0;
        for &link in links {
            node = self.child(node, link)?;
        }
        Some(node)
    }

    /// The last step of the path with these `links`, adding the steps it lacks.
    fn find_or_add(&mut self, links: &[u32], turns: &[u32]) -> u32 {
        let mut node = 0;
        for (i, &link) in links.iter().enumerate() {
            node = match self.child(node, link) {
                Some(existing) => {
                    debug_assert!(i == 0 || self.steps[existing as usize].turn == turns[i - 1]);
                    existing
                }
                None => self.push_step(node, link, if i == 0 { NONE } else { turns[i - 1] }),
            };
        }
        node
    }

    /// Adds, for every link and turn, the flow of all paths through it. `leaf_flow[s]` is the
    /// flow of the paths that end at step `s` and is used up as scratch space.
    ///
    /// Children come after their parents, so going through the steps backwards pushes each
    /// step's total onto its parent before the parent is looked at. A link or turn then gets
    /// one addition per step instead of one per path that crosses it.
    fn add_flows(&self, leaf_flow: &mut [f64], link_flow: &mut [f64], turn_flow: &mut [f64]) {
        for s in (1..self.steps.len()).rev() {
            let flow = leaf_flow[s];
            if flow == 0.0 {
                continue;
            }
            let step = &self.steps[s];
            link_flow[step.link as usize] += flow;
            if step.turn != NONE {
                turn_flow[step.turn as usize] += flow;
            }
            leaf_flow[step.parent as usize] += flow;
        }
    }

    /// Cost of travelling from the origin to the end of every step, in `out[step]`.
    fn costs_into(&self, link_cost: &[f64], turn_cost: &[f64], out: &mut Vec<f64>) {
        out.clear();
        out.push(0.0);
        for step in &self.steps[1..] {
            let turn = if step.turn == NONE {
                0.0
            } else {
                turn_cost[step.turn as usize]
            };
            let cost = out[step.parent as usize] + link_cost[step.link as usize] + turn;
            out.push(cost);
        }
    }

    /// Links of the path ending at `leaf`, in travel order.
    fn links_into(&self, leaf: u32, out: &mut Vec<u32>) {
        out.clear();
        let mut s = leaf;
        while s != 0 {
            out.push(self.steps[s as usize].link);
            s = self.steps[s as usize].parent;
        }
        out.reverse();
    }

    /// Turn ids of the path ending at `leaf`, in travel order.
    fn turns_into(&self, leaf: u32, out: &mut Vec<u32>) {
        out.clear();
        let mut s = leaf;
        while s != 0 {
            let step = &self.steps[s as usize];
            if step.turn != NONE {
                out.push(step.turn);
            }
            s = step.parent;
        }
        out.reverse();
    }

    /// The parts of the paths ending at `a` and `b` that are not shared: what is left after
    /// dropping the longest common start and then the longest common finish. Links and turns
    /// are trimmed separately in the sense that a turn is shared only when both of its links are.
    ///
    /// The common start is found by walking up from both ends until the walks meet, so only
    /// the part below that meeting point is ever read.
    pub(crate) fn differing(&self, a: u32, b: u32, in_a: &mut Stretch, in_b: &mut Stretch) {
        in_a.clear();
        in_b.clear();
        if a == b {
            return;
        }
        let steps = &self.steps;
        let up = |s: u32| steps[s as usize].parent;
        let depth = |s: u32| steps[s as usize].depth;
        let (da, db) = (depth(a), depth(b));

        // Lowest common ancestor: the end of the common start.
        let (mut p, mut q) = (a, b);
        while depth(p) > depth(q) {
            p = up(p);
        }
        while depth(q) > depth(p) {
            q = up(q);
        }
        while p != q {
            p = up(p);
            q = up(q);
        }
        let common_start = depth(p);

        // Common finish: walk back from both ends while the links agree, but never into the
        // common start.
        let limit = da.min(db) - common_start;
        let (mut x, mut y) = (a, b);
        let (mut last_x, mut last_y) = (NONE, NONE);
        let mut matched = 0;
        while matched < limit && steps[x as usize].link == steps[y as usize].link {
            last_x = x;
            last_y = y;
            x = up(x);
            y = up(y);
            matched += 1;
        }
        // The turn into the common finish is shared too when the links just before it agree.
        // That can only happen when the walk above stopped at its limit, not on a mismatch.
        let turn_differs = steps[x as usize].link != steps[y as usize].link;
        let (after_x, after_y) = if turn_differs {
            (last_x, last_y)
        } else {
            (NONE, NONE)
        };
        self.collect_stretch(x, after_x, common_start, in_a);
        self.collect_stretch(y, after_y, common_start, in_b);
    }

    /// Fills `out` with the steps from just below depth `common_start` down to `end`, plus
    /// the turn into `after` (the first step of the common finish), if there is one.
    fn collect_stretch(&self, end: u32, after: u32, common_start: u32, out: &mut Stretch) {
        if after != NONE {
            let turn = self.steps[after as usize].turn;
            if turn != NONE {
                out.turns.push(turn);
            }
        }
        let mut s = end;
        while self.steps[s as usize].depth > common_start {
            let step = &self.steps[s as usize];
            out.links.push(step.link);
            if step.turn != NONE {
                out.turns.push(step.turn);
            }
            s = step.parent;
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PathRef {
    /// Last step of the path in the tree.
    pub(crate) leaf: u32,
    pub(crate) flow: f64,
}

/// All paths of one OD pair.
#[derive(Debug)]
pub(crate) struct OdPaths {
    pub(crate) dest: u32,
    pub(crate) demand: f64,
    pub(crate) paths: Vec<PathRef>,
}

#[derive(Debug)]
pub(crate) struct OriginPaths {
    pub(crate) tree: StepTree,
    pub(crate) ods: Vec<OdPaths>,
}

impl Default for OriginPaths {
    fn default() -> OriginPaths {
        OriginPaths {
            tree: StepTree::new(),
            ods: Vec::new(),
        }
    }
}

impl OriginPaths {
    pub(crate) fn path_links_into(&self, path: &PathRef, out: &mut Vec<u32>) {
        self.tree.links_into(path.leaf, out);
    }

    pub(crate) fn path_turns_into(&self, path: &PathRef, out: &mut Vec<u32>) {
        self.tree.turns_into(path.leaf, out);
    }

    /// Adds the flow of every path of this origin to the link and turn totals. `scratch` is
    /// reusable working space.
    pub(crate) fn add_flows(
        &self,
        scratch: &mut Vec<f64>,
        link_flow: &mut [f64],
        turn_flow: &mut [f64],
    ) {
        scratch.clear();
        scratch.resize(self.tree.steps.len(), 0.0);
        for od in &self.ods {
            for path in &od.paths {
                scratch[path.leaf as usize] += path.flow;
            }
        }
        self.tree.add_flows(scratch, link_flow, turn_flow);
    }

    /// Cost from the origin to the end of every step, indexed by a path's `leaf`.
    pub(crate) fn step_costs_into(&self, link_cost: &[f64], turn_cost: &[f64], out: &mut Vec<f64>) {
        self.tree.costs_into(link_cost, turn_cost, out);
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

    /// Index of `links` among the paths of OD `od`, if it is already there. Equal paths end
    /// at the same step, so this is a walk down the tree and a comparison of step ids.
    pub(crate) fn find_path(&self, od: usize, links: &[u32]) -> Option<usize> {
        let leaf = self.tree.find(links)?;
        self.ods[od].paths.iter().position(|p| p.leaf == leaf)
    }

    /// Adds a path with zero flow and returns its index. `turns` must be the turn ids
    /// between consecutive `links` (one fewer than the links).
    pub(crate) fn add_path(&mut self, od: usize, links: &[u32], turns: &[u32]) -> usize {
        debug_assert_eq!(turns.len(), links.len().saturating_sub(1));
        let leaf = self.tree.find_or_add(links, turns);
        let paths = &mut self.ods[od].paths;
        paths.push(PathRef { leaf, flow: 0.0 });
        paths.len() - 1
    }

    /// Drops the paths of OD `od` that carry no flow. Their steps stay in the tree until the
    /// next compaction, because other paths may share them.
    pub(crate) fn remove_empty_paths(&mut self, od: usize) {
        self.ods[od].paths.retain(|p| p.flow > 0.0);
    }

    /// Rebuilds the tree without unused steps once it has doubled since the last rebuild.
    pub(crate) fn compact_if_needed(&mut self) {
        // The floor keeps tiny blocks from being rebuilt over and over.
        if self.tree.steps.len() <= 2 * self.tree.compacted_len.max(512) {
            return;
        }
        self.compact();
    }

    /// Keeps exactly the steps on the way to some path, renumbering them in their old order,
    /// which keeps every parent before its children.
    fn compact(&mut self) {
        let old = &self.tree.steps;
        let mut keep = vec![false; old.len()];
        keep[0] = true;
        for od in &self.ods {
            for path in &od.paths {
                let mut s = path.leaf;
                while !keep[s as usize] {
                    keep[s as usize] = true;
                    s = old[s as usize].parent;
                }
            }
        }
        let mut new_id = vec![NONE; old.len()];
        let mut kept = 0u32;
        for (i, &k) in keep.iter().enumerate() {
            if k {
                new_id[i] = kept;
                kept += 1;
            }
        }
        let mut steps = Vec::with_capacity(kept as usize);
        for (i, step) in old.iter().enumerate() {
            if keep[i] {
                steps.push(Step {
                    parent: if step.parent == NONE {
                        NONE
                    } else {
                        new_id[step.parent as usize]
                    },
                    first_child: NONE,
                    next_sibling: NONE,
                    ..*step
                });
            }
        }
        // Child lists are rebuilt from the parent links.
        for id in 1..steps.len() {
            let parent = steps[id].parent as usize;
            steps[id].next_sibling = steps[parent].first_child;
            steps[parent].first_child = id as u32;
        }
        for od in &mut self.ods {
            for path in &mut od.paths {
                path.leaf = new_id[path.leaf as usize];
            }
        }
        self.tree.compacted_len = steps.len();
        self.tree.steps = steps;
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

    /// Number of steps in the trees of all origins (the root of each included).
    pub fn num_steps(&self) -> usize {
        self.origins.iter().map(|o| o.tree.steps.len()).sum()
    }

    /// Every path as `(origin, destination, link ids, flow)`.
    pub fn iter(&self) -> impl Iterator<Item = (u32, u32, Vec<u32>, f64)> {
        self.iter_with_turns()
            .map(|(origin, dest, links, _, flow)| (origin, dest, links, flow))
    }

    /// Every path as `(origin, destination, link ids, turn ids, flow)`.
    pub fn iter_with_turns(&self) -> impl Iterator<Item = (u32, u32, Vec<u32>, Vec<u32>, f64)> {
        self.origins.iter().enumerate().flat_map(|(origin, block)| {
            block.ods.iter().flat_map(move |od| {
                od.paths.iter().map(move |p| {
                    let (mut links, mut turns) = (Vec::new(), Vec::new());
                    block.path_links_into(p, &mut links);
                    block.path_turns_into(p, &mut turns);
                    (origin as u32, od.dest, links, turns, p.flow)
                })
            })
        })
    }

    /// Adds the flow of every path to `link_flow` and `turn_flow`.
    pub(crate) fn add_flows(&self, link_flow: &mut [f64], turn_flow: &mut [f64]) {
        let mut scratch = Vec::new();
        for block in &self.origins {
            block.add_flows(&mut scratch, link_flow, turn_flow);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The turn between two links, made up but the same for the same pair.
    fn turn(a: u32, b: u32) -> u32 {
        100 * a + b
    }

    fn turns_of(links: &[u32]) -> Vec<u32> {
        links.windows(2).map(|w| turn(w[0], w[1])).collect()
    }

    /// Adds `links` (with made-up turn ids) unless already present; returns its index.
    fn find_or_add(block: &mut OriginPaths, od: usize, links: &[u32]) -> usize {
        if let Some(index) = block.find_path(od, links) {
            return index;
        }
        block.add_path(od, links, &turns_of(links))
    }

    fn links_of(block: &OriginPaths, od: usize, path: usize) -> Vec<u32> {
        let mut out = Vec::new();
        block.path_links_into(&block.ods[od].paths[path], &mut out);
        out
    }

    fn turns_of_path(block: &OriginPaths, od: usize, path: usize) -> Vec<u32> {
        let mut out = Vec::new();
        block.path_turns_into(&block.ods[od].paths[path], &mut out);
        out
    }

    #[test]
    fn dedups_paths_within_an_od() {
        let mut block = OriginPaths::default();
        let od = block.add_od(5, 10.0);
        let a = find_or_add(&mut block, od, &[1, 2, 3]);
        let b = find_or_add(&mut block, od, &[1, 4, 3]);
        let again = find_or_add(&mut block, od, &[1, 2, 3]);
        assert_eq!((a, b, again), (0, 1, 0));
        assert_eq!(links_of(&block, od, 1), [1, 4, 3]);
        assert_eq!(turns_of_path(&block, od, 1), [104, 403]);
    }

    #[test]
    fn paths_that_start_alike_share_steps() {
        let mut block = OriginPaths::default();
        let od = block.add_od(5, 1.0);
        find_or_add(&mut block, od, &[1, 2, 3]);
        assert_eq!(block.tree.steps.len(), 4); // root + 3
        find_or_add(&mut block, od, &[1, 2, 4]);
        assert_eq!(block.tree.steps.len(), 5); // only the last step is new
        find_or_add(&mut block, od, &[1, 5]);
        assert_eq!(block.tree.steps.len(), 6);
        // A prefix of an existing path is a path of its own, with no new steps.
        let prefix = find_or_add(&mut block, od, &[1, 2]);
        assert_eq!(block.tree.steps.len(), 6);
        assert_eq!(links_of(&block, od, prefix), [1, 2]);
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
        assert_eq!(turns_of_path(&block, od, p), &[] as &[u32]);
        assert_eq!(links_of(&block, od, p), [9]);
    }

    #[test]
    fn removes_empty_paths_and_compaction_keeps_the_rest() {
        let mut block = OriginPaths::default();
        let od = block.add_od(5, 10.0);
        // Enough distinct long paths that the tree outgrows the compaction floor.
        let mut paths: Vec<Vec<u32>> = Vec::new();
        for i in 0..400u32 {
            paths.push(vec![1, 2 + i % 7, 10 + i, 1000 + i, 2000 + i]);
        }
        for (i, links) in paths.iter().enumerate() {
            let p = find_or_add(&mut block, od, links);
            block.ods[od].paths[p].flow = if i % 4 == 0 { 5.0 } else { 0.0 };
        }
        let before = block.tree.steps.len();
        block.remove_empty_paths(od);
        assert_eq!(block.ods[od].paths.len(), 100);
        assert_eq!(
            block.tree.steps.len(),
            before,
            "removal leaves steps in place"
        );

        block.compact_if_needed();
        assert!(block.tree.steps.len() < before, "unused steps were dropped");
        // Every kept path still reads back whole, and new paths can still be added.
        let expected: Vec<&Vec<u32>> = paths.iter().step_by(4).collect();
        for (i, links) in expected.iter().enumerate() {
            assert_eq!(&links_of(&block, od, i), *links);
            assert_eq!(turns_of_path(&block, od, i), turns_of(links));
            assert_eq!(block.find_path(od, links), Some(i));
        }
        let added = find_or_add(&mut block, od, &[1, 2, 77, 78]);
        assert_eq!(links_of(&block, od, added), [1, 2, 77, 78]);
        // Parents still come before children.
        for (id, step) in block.tree.steps.iter().enumerate().skip(1) {
            assert!((step.parent as usize) < id);
        }
    }

    #[test]
    fn store_iterates_all_paths() {
        let mut store = PathStore::new(3);
        let od = store.origins[1].add_od(2, 4.0);
        let p = find_or_add(&mut store.origins[1], od, &[0, 7]);
        store.origins[1].ods[od].paths[p].flow = 4.0;
        assert_eq!(store.num_paths(), 1);
        let all: Vec<_> = store.iter().collect();
        assert_eq!(all, vec![(1, 2, vec![0u32, 7], 4.0)]);
        let with_turns: Vec<_> = store.iter_with_turns().collect();
        assert_eq!(with_turns, vec![(1, 2, vec![0u32, 7], vec![7u32], 4.0)]);
        let (mut link_flow, mut turn_flow) = (vec![0.0; 8], vec![0.0; 8]);
        store.add_flows(&mut link_flow, &mut turn_flow);
        assert_eq!((link_flow[0], link_flow[7], turn_flow[7]), (4.0, 4.0, 4.0));
        assert_eq!(link_flow.iter().sum::<f64>(), 8.0);
    }

    #[test]
    fn flows_and_costs_from_the_tree_match_walking_every_path() {
        let mut block = OriginPaths::default();
        let od = block.add_od(1, 1.0);
        let all: [&[u32]; 5] = [&[1, 2, 3], &[1, 2, 4], &[1, 5], &[6, 2, 3], &[1, 2]];
        for (i, links) in all.iter().enumerate() {
            let p = find_or_add(&mut block, od, links);
            block.ods[od].paths[p].flow = 1.0 + i as f64;
        }
        let (mut link_flow, mut turn_flow) = (vec![0.0; 7], vec![0.0; 700]);
        block.add_flows(&mut Vec::new(), &mut link_flow, &mut turn_flow);
        let (mut want_links, mut want_turns) = (vec![0.0; 7], vec![0.0; 700]);
        for (i, links) in all.iter().enumerate() {
            for &a in *links {
                want_links[a as usize] += 1.0 + i as f64;
            }
            for t in turns_of(links) {
                want_turns[t as usize] += 1.0 + i as f64;
            }
        }
        assert_eq!(link_flow, want_links);
        assert_eq!(turn_flow, want_turns);

        let link_cost = [0.0, 1.0, 2.5, 4.0, 8.0, 16.0, 32.0];
        let turn_cost: Vec<f64> = (0..700).map(|t| t as f64 / 100.0).collect();
        let mut costs = Vec::new();
        block.step_costs_into(&link_cost, &turn_cost, &mut costs);
        for (i, links) in all.iter().enumerate() {
            let want: f64 = links.iter().map(|&a| link_cost[a as usize]).sum::<f64>()
                + turns_of(links)
                    .iter()
                    .map(|&t| turn_cost[t as usize])
                    .sum::<f64>();
            let leaf = block.ods[od].paths[i].leaf;
            assert!((costs[leaf as usize] - want).abs() < 1e-12, "path {i}");
        }
    }

    /// The straightforward way: trim the common start and finish of two full sequences.
    fn trim_common<'a>(a: &'a [u32], b: &'a [u32]) -> (&'a [u32], &'a [u32]) {
        let n = a.len().min(b.len());
        let mut prefix = 0;
        while prefix < n && a[prefix] == b[prefix] {
            prefix += 1;
        }
        let mut suffix = 0;
        while suffix < n - prefix && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix] {
            suffix += 1;
        }
        (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix])
    }

    /// Small deterministic generator, so the test needs no extra crate.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self, below: u32) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) % below as u64) as u32
        }
    }

    /// The tree's differing stretches must equal trimming the two full paths, for every pair
    /// of paths, including pairs where one path is inside the other or they overlap at both ends.
    #[test]
    fn differing_matches_trimming_full_paths() {
        for seed in 0..40 {
            let mut rng = Lcg(seed + 1);
            // A tiny alphabet makes shared starts, shared finishes and repeats likely.
            let alphabet = 2 + rng.next(4);
            let mut block = OriginPaths::default();
            let od = block.add_od(1, 1.0);
            let mut all: Vec<Vec<u32>> = Vec::new();
            for _ in 0..30 {
                let len = 1 + rng.next(8);
                let links: Vec<u32> = (0..len).map(|_| rng.next(alphabet)).collect();
                let p = find_or_add(&mut block, od, &links);
                if p == all.len() {
                    all.push(links);
                }
            }
            let (mut in_a, mut in_b) = (Stretch::default(), Stretch::default());
            for (i, a) in all.iter().enumerate() {
                for (j, b) in all.iter().enumerate() {
                    let leaf = |k: usize| block.ods[od].paths[k].leaf;
                    block.tree.differing(leaf(i), leaf(j), &mut in_a, &mut in_b);
                    if i == j {
                        assert!(in_a.links.is_empty() && in_a.turns.is_empty());
                        continue;
                    }
                    let sorted = |v: &[u32]| {
                        let mut v = v.to_vec();
                        v.sort_unstable();
                        v
                    };
                    let (want_a, want_b) = trim_common(a, b);
                    assert_eq!(
                        (sorted(&in_a.links), sorted(&in_b.links)),
                        (sorted(want_a), sorted(want_b))
                    );
                    let (ta, tb) = (turns_of(a), turns_of(b));
                    let (want_ta, want_tb) = trim_common(&ta, &tb);
                    assert_eq!(
                        (sorted(&in_a.turns), sorted(&in_b.turns)),
                        (sorted(want_ta), sorted(want_tb)),
                        "seed {seed}: {a:?} against {b:?}"
                    );
                }
            }
        }
    }
}
