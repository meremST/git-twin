//! Commit graph lane layout.
//!
//! The builder is fed commits in display order (children before parents)
//! and keeps its lane state between batches, so progressive loading never
//! renumbers lanes. Each lane cell stores connectivity bits that the UI maps
//! to box-drawing (or ASCII) glyphs.

use super::oid::Oid;

pub const UP: u8 = 1;
pub const DOWN: u8 = 2;
pub const LEFT: u8 = 4;
pub const RIGHT: u8 = 8;

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

#[derive(Default, Clone)]
pub struct GraphBuilder {
    lanes: Vec<Option<(Oid, u8)>>,
    next_color: u8,
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
    pub fn new() -> GraphBuilder {
        GraphBuilder::default()
    }

    fn new_color(&mut self) -> u8 {
        let c = self.next_color;
        self.next_color = self.next_color.wrapping_add(1);
        c
    }

    pub fn lane_count(&self) -> usize {
        self.lanes.len()
    }

    pub fn push(&mut self, oid: Oid, parents: &[Oid]) -> GraphRow {
        let incoming: Vec<usize> = self
            .lanes
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l, Some((o, _)) if *o == oid))
            .map(|(i, _)| i)
            .collect();
        let col = match incoming.first() {
            Some(c) => *c,
            None => match self.lanes.iter().position(|l| l.is_none()) {
                Some(free) => free,
                None => {
                    self.lanes.push(None);
                    self.lanes.len() - 1
                }
            },
        };
        let node_color = match incoming.first() {
            Some(c) => self.lanes[*c].map(|(_, col)| col).unwrap_or(0),
            None => self.new_color(),
        };

        // Width large enough for any lane allocated in this row.
        let mut cells = vec![LaneCell::default(); self.lanes.len() + parents.len()];
        for (i, l) in self.lanes.iter().enumerate() {
            if let Some((o, c)) = l
                && *o != oid
            {
                cells[i].bits |= UP | DOWN;
                cells[i].color = *c;
            }
        }
        for &j in &incoming {
            let c = self.lanes[j].map(|(_, c)| c).unwrap_or(0);
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
        if let Some(p0) = parents.first() {
            self.lanes[col] = Some((*p0, node_color));
            cells[col].bits |= DOWN;
        }
        for p in parents.iter().skip(1) {
            let existing = self
                .lanes
                .iter()
                .position(|l| matches!(l, Some((o, _)) if o == p))
                .filter(|k| *k != col);
            match existing {
                Some(k) => {
                    let c = self.lanes[k].map(|(_, c)| c).unwrap_or(0);
                    connect(&mut cells, col, k, c);
                }
                None => {
                    let k = match self
                        .lanes
                        .iter()
                        .enumerate()
                        .position(|(i, l)| l.is_none() && i != col && !freed.contains(&i))
                    {
                        Some(k) => k,
                        None => {
                            self.lanes.push(None);
                            self.lanes.len() - 1
                        }
                    };
                    let c = self.new_color();
                    self.lanes[k] = Some((*p, c));
                    if cells.len() <= k {
                        cells.resize(k + 1, LaneCell::default());
                    }
                    cells[k].bits |= DOWN;
                    cells[k].color = c;
                    connect(&mut cells, col, k, c);
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
}
