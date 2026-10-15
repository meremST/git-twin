//! Commit graph lane layout.
//!
//! The builder is fed commits in display order (children before parents)
//! and keeps its lane state between batches, so progressive loading never
//! renumbers lanes. Each lane cell stores connectivity bits that the UI maps
//! to box-drawing (or ASCII) glyphs.
//!
//! Like gitk, lines that stay idle for too long are cut: the line ends with a
//! down arrow ([`CUT`]) and its column is freed. When the awaited commit
//! arrives, one cell of its row carries [`RESUME`]: the UI draws the matching
//! up arrow in the row above, which the builder guarantees to be empty there.
//! That cell is the node itself when possible, otherwise a free column joined
//! to the node like a merge (`─╯`).

use std::collections::HashMap;

use super::oid::Oid;

pub const UP: u8 = 1;
pub const DOWN: u8 = 2;
pub const LEFT: u8 = 4;
pub const RIGHT: u8 = 8;
/// The line coming from above stops here (drawn as a down arrow).
pub const CUT: u8 = 16;
/// A cut line resumes here: draw an up arrow in the row above this cell.
pub const RESUME: u8 = 32;

/// A line cut at `row` / `lane`, heading for commit `target`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cut {
    pub row: usize,
    pub lane: u16,
    pub target: Oid,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LaneCell {
    pub bits: u8,
    pub color: u8,
    /// Colour of the horizontal segment leaving this cell to the right.
    pub hcolor: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphRow {
    pub node: u16,
    pub node_color: u8,
    pub cells: Box<[LaneCell]>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Lane {
    oid: Oid,
    color: u8,
    /// Rows this lane has only passed through.
    idle: usize,
}

#[derive(Default, Clone)]
pub struct GraphBuilder {
    lanes: Vec<Option<Lane>>,
    next_color: u8,
    /// Cut lines idle for more than this many rows (0 = never).
    cut_after: usize,
    /// Commits awaited by cut lines, with the colour of the line.
    parked: HashMap<Oid, u8>,
    /// Columns cut in the previous row: not reused right away, so a new line
    /// never seems to continue a cut one.
    just_cut: Vec<usize>,
    /// Connectivity of the previous row (where resume arrows are drawn).
    prev: Vec<u8>,
    /// Rows pushed so far.
    row: usize,
    /// Cuts made since the last [`GraphBuilder::take_cuts`].
    cuts: Vec<Cut>,
}

fn connect(cells: &mut [LaneCell], a: usize, b: usize, color: u8) {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    for (x, cell) in cells.iter_mut().enumerate().take(hi + 1).skip(lo) {
        if x == lo {
            cell.bits |= RIGHT;
        } else if x == hi {
            cell.bits |= LEFT;
        } else {
            cell.bits |= LEFT | RIGHT;
        }
        if x < hi {
            cell.hcolor = color;
        }
    }
}

impl GraphBuilder {
    /// Whether an unchanged suffix can reuse its layout after a prefix edit.
    /// Absolute row numbers only affect the positions reported by `take_cuts`.
    pub fn same_continuation(&self, other: &Self) -> bool {
        self.lanes == other.lanes
            && self.next_color == other.next_color
            && self.cut_after == other.cut_after
            && self.parked == other.parked
            && self.just_cut == other.just_cut
            && self.prev == other.prev
    }

    pub fn new() -> GraphBuilder {
        GraphBuilder::default()
    }

    /// Builder cutting lines idle for more than `rows` rows (0 = never).
    pub fn with_cut_after(rows: usize) -> GraphBuilder {
        GraphBuilder {
            cut_after: rows,
            ..GraphBuilder::default()
        }
    }

    fn new_color(&mut self) -> u8 {
        let c = self.next_color;
        self.next_color = self.next_color.wrapping_add(1);
        c
    }

    pub fn lane_count(&self) -> usize {
        self.lanes.len()
    }

    /// Lines cut since the previous call.
    pub fn take_cuts(&mut self) -> Vec<Cut> {
        std::mem::take(&mut self.cuts)
    }

    fn clear_above(&self, k: usize) -> bool {
        self.prev.get(k).is_none_or(|b| *b == 0)
    }

    /// A free column for a new line, never one of `avoid` nor one cut in
    /// the previous row; with `clear`, also empty in the previous row.
    fn alloc(&mut self, avoid: &[usize], clear: bool) -> usize {
        let usable = |b: &GraphBuilder, i: usize| {
            !avoid.contains(&i) && !b.just_cut.contains(&i) && (!clear || b.clear_above(i))
        };
        if let Some(k) = (0..self.lanes.len()).find(|&i| self.lanes[i].is_none() && usable(self, i))
        {
            return k;
        }
        loop {
            self.lanes.push(None);
            let k = self.lanes.len() - 1;
            if usable(self, k) {
                return k;
            }
        }
    }

    /// Column for the up arrow of a resumed line joining the node at `col`:
    /// the nearest column empty both in the previous row and in `cells`.
    fn resume_column(&self, cells: &[LaneCell], col: usize) -> usize {
        let free = |k: usize| {
            self.clear_above(k)
                && cells.get(k).is_none_or(|c| c.bits == 0)
                && self.lanes.get(k).is_none_or(|l| l.is_none())
        };
        (1..)
            .flat_map(|d| [Some(col + d), col.checked_sub(d)])
            .flatten()
            .find(|&k| free(k))
            .expect("columns beyond the row are free")
    }

    pub fn push(&mut self, oid: Oid, parents: &[Oid]) -> GraphRow {
        let incoming: Vec<usize> = self
            .lanes
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l, Some(l) if l.oid == oid))
            .map(|(i, _)| i)
            .collect();
        let resumed = if self.parked.is_empty() {
            None
        } else {
            self.parked.remove(&oid)
        };
        let col = match incoming.first() {
            Some(c) => *c,
            // A resumed line can then come straight down onto the node.
            None => self.alloc(&[], resumed.is_some()),
        };
        let node_color = match (incoming.first(), resumed) {
            (Some(c), _) => self.lanes[*c].map(|l| l.color).unwrap_or(0),
            (None, Some(c)) => c,
            (None, None) => self.new_color(),
        };

        // Width large enough for any lane allocated in this row.
        let mut cells = vec![LaneCell::default(); self.lanes.len() + parents.len()];
        for (i, l) in self.lanes.iter().enumerate() {
            if let Some(l) = l
                && l.oid != oid
            {
                cells[i].bits |= UP | DOWN;
                cells[i].color = l.color;
            }
        }
        for &j in &incoming {
            let c = self.lanes[j].map(|l| l.color).unwrap_or(0);
            cells[j].bits |= UP;
            cells[j].color = c;
            if j != col {
                connect(&mut cells, j, col, c);
            }
        }
        cells[col].color = node_color;
        let freed: Vec<usize> = incoming.iter().copied().filter(|j| *j != col).collect();
        for &j in &incoming {
            self.lanes[j] = None;
        }
        let mut avoid = freed;
        avoid.push(col);
        if let Some(c) = resumed {
            let k = if incoming.is_empty() && self.clear_above(col) {
                col
            } else {
                self.resume_column(&cells, col)
            };
            if cells.len() <= k {
                cells.resize(k + 1, LaneCell::default());
            }
            cells[k].bits |= UP | RESUME;
            if k != col {
                cells[k].color = c;
                connect(&mut cells, k, col, c);
                avoid.push(k);
            }
        }
        if let Some(p0) = parents.first() {
            self.lanes[col] = Some(Lane {
                oid: *p0,
                color: node_color,
                idle: 0,
            });
            cells[col].bits |= DOWN;
        }
        for p in parents.iter().skip(1) {
            let existing = self
                .lanes
                .iter()
                .position(|l| matches!(l, Some(l) if l.oid == *p))
                .filter(|k| *k != col);
            match existing {
                Some(k) => {
                    let lane = self.lanes[k].as_mut().expect("found above");
                    lane.idle = 0;
                    let c = lane.color;
                    connect(&mut cells, col, k, c);
                }
                None => {
                    let k = self.alloc(&avoid, false);
                    let c = self.new_color();
                    self.lanes[k] = Some(Lane {
                        oid: *p,
                        color: c,
                        idle: 0,
                    });
                    if cells.len() <= k {
                        cells.resize(k + 1, LaneCell::default());
                    }
                    cells[k].bits |= DOWN;
                    cells[k].color = c;
                    connect(&mut cells, col, k, c);
                }
            }
        }
        self.just_cut.clear();
        if self.cut_after > 0 {
            for (i, slot) in self.lanes.iter_mut().enumerate() {
                let Some(lane) = slot else { continue };
                lane.idle += 1;
                // Only cut a plain vertical segment, never a junction.
                if lane.idle > self.cut_after && cells.get(i).map(|c| c.bits) == Some(UP | DOWN) {
                    cells[i].bits = UP | CUT;
                    self.parked.entry(lane.oid).or_insert(lane.color);
                    self.cuts.push(Cut {
                        row: self.row,
                        lane: i as u16,
                        target: lane.oid,
                    });
                    self.just_cut.push(i);
                    *slot = None;
                }
            }
        }
        while matches!(self.lanes.last(), Some(None)) {
            self.lanes.pop();
        }
        let used = cells
            .iter()
            .rposition(|c| c.bits != 0)
            .map(|p| p + 1)
            .unwrap_or(0)
            .max(col + 1);
        cells.truncate(used);
        self.prev.clear();
        self.prev.extend(cells.iter().map(|c| c.bits));
        // A node without lines (a lone tip/root) still occupies its cell.
        self.prev[col] = u8::MAX;
        self.row += 1;
        GraphRow {
            node: col as u16,
            node_color,
            cells: cells.into_boxed_slice(),
        }
    }
}

/// Maps connectivity bits to a glyph.
pub fn glyph(bits: u8, ascii: bool) -> char {
    let u = bits & UP != 0;
    let d = bits & DOWN != 0;
    let l = bits & LEFT != 0;
    let r = bits & RIGHT != 0;
    if ascii {
        return match (u || d, l || r) {
            (false, false) => ' ',
            (true, false) => '|',
            (false, true) => '-',
            (true, true) => '+',
        };
    }
    match (u, d, l, r) {
        (false, false, false, false) => ' ',
        (true, true, false, false) | (true, false, false, false) | (false, true, false, false) => {
            '│'
        }
        (false, false, true, true) | (false, false, true, false) | (false, false, false, true) => {
            '─'
        }
        (true, true, true, true) => '┼',
        (true, true, false, true) => '├',
        (true, true, true, false) => '┤',
        (false, true, false, true) => '╭',
        (false, true, true, false) => '╮',
        (true, false, false, true) => '╰',
        (true, false, true, false) => '╯',
        (true, false, true, true) => '┴',
        (false, true, true, true) => '┬',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn o(n: u8) -> Oid {
        Oid::from_hex(&format!("{:02x}", n).repeat(20)).unwrap()
    }

    fn render(rows: &[GraphRow]) -> Vec<String> {
        rows.iter()
            .map(|r| {
                let mut s = String::new();
                for (i, c) in r.cells.iter().enumerate() {
                    if i == r.node as usize {
                        s.push('*');
                    } else {
                        s.push(glyph(c.bits, false));
                    }
                    s.push(if c.bits & RIGHT != 0 { '─' } else { ' ' });
                }
                s.trim_end().to_string()
            })
            .collect()
    }

    #[test]
    fn linear_history() {
        let mut g = GraphBuilder::new();
        let rows = vec![
            g.push(o(3), &[o(2)]),
            g.push(o(2), &[o(1)]),
            g.push(o(1), &[]),
        ];
        assert_eq!(render(&rows), vec!["*", "*", "*"]);
        assert!(rows.iter().all(|r| r.node == 0));
        assert_eq!(rows[0].cells[0].bits, DOWN);
        assert_eq!(rows[1].cells[0].bits, UP | DOWN);
        assert_eq!(rows[2].cells[0].bits, UP);
    }

    #[test]
    fn branch_and_merge() {
        // 5 = merge(4, 3); 4 -> 2; 3 -> 2; 2 -> 1
        let mut g = GraphBuilder::new();
        let rows = vec![
            g.push(o(5), &[o(4), o(3)]),
            g.push(o(4), &[o(2)]),
            g.push(o(3), &[o(2)]),
            g.push(o(2), &[o(1)]),
            g.push(o(1), &[]),
        ];
        assert_eq!(render(&rows), vec!["*─╮", "* │", "│ *", "*─╯", "*"]);
    }

    #[test]
    fn octopus_merge() {
        let mut g = GraphBuilder::new();
        let rows = vec![
            g.push(o(9), &[o(1), o(2), o(3)]),
            g.push(o(3), &[o(1)]),
            g.push(o(2), &[o(1)]),
            g.push(o(1), &[]),
        ];
        let r = render(&rows);
        assert_eq!(r[0], "*─┬─╮");
        assert_eq!(rows[3].node, 0);
        assert_eq!(g.lane_count(), 0);
    }

    #[test]
    fn missing_parent_keeps_lane_and_batches_are_continuous() {
        // Parent 7 is outside the loaded set: its lane continues.
        let commits = [(o(5), vec![o(4), o(7)]), (o(4), vec![o(3)]), (o(3), vec![])];
        let mut one = GraphBuilder::new();
        let all: Vec<GraphRow> = commits.iter().map(|(c, p)| one.push(*c, p)).collect();
        let mut two = GraphBuilder::new();
        let mut split: Vec<GraphRow> = commits[..1].iter().map(|(c, p)| two.push(*c, p)).collect();
        split.extend(commits[1..].iter().map(|(c, p)| two.push(*c, p)));
        assert_eq!(all, split);
        assert_eq!(render(&all)[2], "* │");
        assert_eq!(one.lane_count(), 2);
    }

    #[test]
    fn freed_lane_not_reused_in_same_row() {
        // Two branches converge on 2 which is itself a merge.
        let mut g = GraphBuilder::new();
        let rows = [
            g.push(o(6), &[o(2)]),
            g.push(o(5), &[o(2)]),
            g.push(o(2), &[o(1), o(0x10)]),
        ];
        let last = &rows[2];
        // lane 1 was freed in this row: the new parent goes to lane 2 not 1.
        assert_eq!(last.cells[1].bits & DOWN, 0);
        assert!(last.cells.len() >= 3 && last.cells[2].bits & DOWN != 0);
    }

    #[test]
    fn ascii_glyphs() {
        assert_eq!(glyph(UP | DOWN, true), '|');
        assert_eq!(glyph(LEFT | RIGHT, true), '-');
        assert_eq!(glyph(UP | DOWN | LEFT | RIGHT, true), '+');
        assert_eq!(glyph(0, true), ' ');
    }

    /// Merge 100 of a 6-commit side branch (10..15) into 1: the main line
    /// waits idle while the side branch is listed.
    fn long_side_branch(g: &mut GraphBuilder) -> Vec<GraphRow> {
        let mut rows = vec![g.push(o(100), &[o(1), o(15)])];
        for n in (11..=15).rev() {
            rows.push(g.push(o(n), &[o(n - 1)]));
        }
        rows.push(g.push(o(10), &[o(1)]));
        rows.push(g.push(o(1), &[]));
        rows
    }

    #[test]
    fn idle_lines_are_cut_and_resumed() {
        let mut g = GraphBuilder::with_cut_after(2);
        let rows = long_side_branch(&mut g);
        // Lane 0 (waiting for 1) passes row 1, then is cut in row 2.
        assert_eq!(rows[1].cells[0].bits, UP | DOWN);
        assert_eq!(rows[2].cells[0].bits, UP | CUT);
        assert!(
            rows[3..]
                .iter()
                .all(|r| r.cells.first().map_or(0, |c| c.bits) == 0)
        );
        let last = rows.last().unwrap();
        // Lane 1 still arrives on the node: the resumed line gets its own
        // column, empty in the row above, joined like a merge.
        assert_eq!(last.node, 1);
        assert_eq!(render(&rows)[7], "  *─╯");
        assert_eq!(last.cells[2].bits & RESUME, RESUME);
        assert!(rows[6].cells.get(2).is_none());
        assert_eq!(g.lane_count(), 0);
        assert_eq!(
            g.take_cuts(),
            vec![Cut {
                row: 2,
                lane: 0,
                target: o(1)
            }]
        );
        assert!(g.take_cuts().is_empty());
    }

    /// Every resume arrow lands on a cell left empty in the row above, and
    /// every cut is reported where its arrow is drawn.
    #[test]
    fn resume_arrows_always_have_room() {
        let (n, lag) = (80u16, 17u16);
        let id = |k: u16, side: bool| {
            Oid::from_hex(&format!("{:04x}{:02x}", k, side as u8).repeat(40)[..40]).unwrap()
        };
        let mut g = GraphBuilder::with_cut_after(4);
        let mut rows = Vec::new();
        let mut cuts = Vec::new();
        // Like a busy history: main merges a branch forked `lag` commits
        // earlier, and every branch tip is listed right after its merge.
        for k in (0..n).rev() {
            let mut p = Vec::new();
            if k > 0 {
                p.push(id(k - 1, false));
            }
            if k >= lag {
                p.push(id(k - lag, true));
            }
            rows.push(g.push(id(k, false), &p));
            if k >= lag {
                rows.push(g.push(id(k - lag, true), &[id(k - lag, false)]));
            }
            cuts.extend(g.take_cuts());
        }
        let mut resumes = 0;
        for (r, row) in rows.iter().enumerate() {
            for (k, c) in row.cells.iter().enumerate() {
                if c.bits & RESUME != 0 {
                    resumes += 1;
                    let above = &rows[r - 1];
                    assert_ne!(usize::from(above.node), k, "row {r}");
                    assert_eq!(above.cells.get(k).map_or(0, |c| c.bits), 0, "row {r}");
                }
            }
        }
        assert!(!cuts.is_empty());
        for c in &cuts {
            assert_eq!(rows[c.row].cells[usize::from(c.lane)].bits, UP | CUT);
        }
        let targets: std::collections::HashSet<_> = cuts.iter().map(|c| c.target).collect();
        assert_eq!(resumes, targets.len());
        assert_eq!(g.lane_count(), 0);
    }

    #[test]
    fn resumed_root_keeps_line_colour() {
        let mut g = GraphBuilder::with_cut_after(1);
        let first = g.push(o(9), &[o(1)]);
        g.push(o(8), &[]);
        let r = g.push(o(7), &[]);
        assert_eq!(r.cells[0].bits & CUT, 0);
        // Lane 0 was cut while 8 was listed; 7 avoids the cut column.
        let last = g.push(o(1), &[]);
        assert_ne!(last.cells[last.node as usize].bits & RESUME, 0);
        assert_eq!(last.node_color, first.node_color);
    }

    #[test]
    fn no_cut_by_default() {
        let mut g = GraphBuilder::new();
        let rows = long_side_branch(&mut g);
        assert!(
            rows.iter()
                .flat_map(|r| r.cells.iter())
                .all(|c| c.bits & (CUT | RESUME) == 0)
        );
    }

    #[test]
    fn cut_column_is_not_reused_on_the_next_row() {
        let mut g = GraphBuilder::with_cut_after(1);
        g.push(o(100), &[o(1), o(15)]); // lanes: [1, 15]
        let r1 = g.push(o(15), &[o(14)]); // lane 0 idle for 2 rows -> cut
        assert_eq!(r1.cells[0].bits, UP | CUT);
        // A new unrelated tip right after must not start under the arrow.
        let r2 = g.push(o(50), &[o(49)]);
        assert_ne!(r2.node, 0);
    }
}
