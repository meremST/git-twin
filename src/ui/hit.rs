//! Hit zones recorded while rendering. Mouse handling only uses these, so
//! clickable areas always match what was drawn.

use ratatui::layout::Rect;

use crate::app::action::{Action, FileList};
use crate::app::diffview::Show;
use crate::config::ViewKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaneId {
    HList,
    HDiff,
    HFiles,
    WUnstaged,
    WStaged,
    WDiff,
    WMessage,
    Dialog,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitId {
    HTop,
    HBottom,
    HAuthor,
    HDate,
    WLeft,
    WFiles,
    WDiff,
}

/// Geometry shared by drawing and mouse handling; frozen during a drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryScrollbar {
    pub track: Rect,
    pub thumb_height: u16,
    pub max_top: usize,
}

impl HistoryScrollbar {
    pub fn new(track: Rect, total: usize, visible: usize) -> Option<Self> {
        if track.width == 0 || track.height < 2 || visible == 0 || total <= visible {
            return None;
        }
        let thumb_height = (u128::from(track.height) * visible as u128 / total as u128)
            .clamp(1, u128::from(track.height - 1)) as u16;
        Some(Self {
            track,
            thumb_height,
            max_top: total - visible,
        })
    }

    pub fn thumb_top(self, top: usize) -> u16 {
        self.track.y
            + (top.min(self.max_top) as u128 * u128::from(self.travel()) / self.max_top as u128)
                as u16
    }

    pub fn travel(self) -> u16 {
        self.track.height - self.thumb_height
    }

    pub fn jump(self, y: u16) -> usize {
        let pos = y
            .saturating_sub(self.track.y)
            .saturating_sub(self.thumb_height / 2)
            .min(self.travel());
        (u128::from(pos) * self.max_top as u128 / u128::from(self.travel())) as usize
    }

    pub fn drag(self, start_top: usize, start_y: u16, y: u16) -> usize {
        let delta = (i128::from(y) - i128::from(start_y)) * self.max_top as i128
            / i128::from(self.travel());
        (start_top as i128 + delta).clamp(0, self.max_top as i128) as usize
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    MenuTitle(usize),
    MenuItem {
        depth: usize,
        idx: usize,
    },
    MenuBackdrop,
    ViewTab(ViewKind),
    Split(SplitId, Rect),
    Pane(PaneId),
    PaneTab(PaneId),
    HistoryScrollbar(HistoryScrollbar),
    CommitRow(usize),
    RefLabel(usize, Vec<u8>),
    /// Compact ref marker (or its collapse arrow) of the commit at this row.
    RefToggle(usize),
    /// Down arrow ending a cut graph line, at (row, lane).
    CutArrow(usize, u16),
    /// Up arrow drawn on `row`, resuming a cut line into commit `node`.
    ResumeArrow {
        row: usize,
        node: usize,
    },
    ShaEntry,
    FindEntry,
    SearchEntry,
    Button(Action),
    ShowBtn(Show),
    HFileRow(usize),
    DiffLine(PaneId, usize),
    WtFile(FileList, usize, bool),
    WtFilter(FileList),
    Message,
    /// Message text in the status bar (opens the message history).
    StatusMsg,
    DlgField(usize),
    DlgArea(usize, usize),
    DlgChoice(usize, usize),
    DlgListRow(usize, usize),
    DlgButton(usize),
    DlgBackdrop,
}

impl Hit {
    pub fn is_dialog(&self) -> bool {
        matches!(
            self,
            Hit::Pane(PaneId::Dialog)
                | Hit::DlgField(_)
                | Hit::DlgArea(..)
                | Hit::DlgChoice(..)
                | Hit::DlgListRow(..)
                | Hit::DlgButton(_)
                | Hit::DlgBackdrop
        )
    }

    pub fn pane(&self) -> Option<PaneId> {
        match self {
            Hit::Pane(p) | Hit::DiffLine(p, _) => Some(*p),
            Hit::HistoryScrollbar(_)
            | Hit::CommitRow(_)
            | Hit::RefLabel(..)
            | Hit::RefToggle(_)
            | Hit::CutArrow(..)
            | Hit::ResumeArrow { .. } => Some(PaneId::HList),
            Hit::HFileRow(_) => Some(PaneId::HFiles),
            Hit::WtFile(FileList::Unstaged, ..) | Hit::WtFilter(FileList::Unstaged) => {
                Some(PaneId::WUnstaged)
            }
            Hit::WtFile(FileList::Staged, ..) | Hit::WtFilter(FileList::Staged) => {
                Some(PaneId::WStaged)
            }
            Hit::Message => Some(PaneId::WMessage),
            Hit::DlgField(_) | Hit::DlgArea(..) | Hit::DlgListRow(..) | Hit::DlgBackdrop => {
                Some(PaneId::Dialog)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct HitMap {
    pub zones: Vec<(Rect, Hit)>,
}

impl HitMap {
    pub fn clear(&mut self) {
        self.zones.clear();
    }

    pub fn push(&mut self, r: Rect, h: Hit) {
        if r.width > 0 && r.height > 0 {
            self.zones.push((r, h));
        }
    }

    /// Topmost zone containing the point.
    pub fn at(&self, x: u16, y: u16) -> Option<(Rect, &Hit)> {
        self.zones
            .iter()
            .rev()
            .find(|(r, _)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
            .map(|(r, h)| (*r, h))
    }

    /// Pane under the point (for wheel scrolling).
    pub fn pane_at(&self, x: u16, y: u16) -> Option<PaneId> {
        self.zones
            .iter()
            .rev()
            .filter(|(r, _)| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
            .find_map(|(_, h)| match h {
                Hit::MenuBackdrop | Hit::DlgBackdrop => Some(None),
                _ => h.pane().map(Some),
            })
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_scrollbar_geometry_handles_overflow_and_small_tracks() {
        let track = Rect::new(10, 5, 1, 10);
        assert!(HistoryScrollbar::new(track, 0, 10).is_none());
        assert!(HistoryScrollbar::new(track, 10, 10).is_none());
        assert!(HistoryScrollbar::new(Rect::new(10, 5, 1, 1), 100, 1).is_none());
        let bar = HistoryScrollbar::new(track, 100, 20).unwrap();
        assert_eq!(bar.thumb_height, 2);
        assert_eq!(bar.thumb_top(0), track.y);
        assert_eq!(bar.thumb_top(80) + bar.thumb_height, track.bottom());
        assert_eq!(bar.jump(0), 0);
        assert_eq!(bar.jump(track.y + 5), 40);
        assert_eq!(bar.jump(u16::MAX), 80);
        assert_eq!(
            bar.drag(37, 10, 10),
            37,
            "grabbing must not quantize the position"
        );
        assert_eq!(bar.drag(37, 10, 11), 47);
        assert_eq!(bar.drag(37, 10, 0), 0);
        assert_eq!(bar.drag(37, 10, u16::MAX), 80);

        for total in [21, 100_000, usize::MAX] {
            let bar = HistoryScrollbar::new(track, total, 20).unwrap();
            assert!(bar.thumb_height >= 1 && bar.thumb_height < track.height);
            assert_eq!(bar.jump(track.bottom() - 1), total - 20);
            assert_eq!(bar.thumb_top(total), track.bottom() - bar.thumb_height);
        }
    }

    #[test]
    fn topmost_wins() {
        let mut m = HitMap::default();
        m.push(Rect::new(0, 0, 10, 10), Hit::Pane(PaneId::HList));
        m.push(Rect::new(0, 2, 10, 1), Hit::CommitRow(5));
        assert_eq!(m.at(3, 2).unwrap().1, &Hit::CommitRow(5));
        assert_eq!(m.at(3, 3).unwrap().1, &Hit::Pane(PaneId::HList));
        assert!(m.at(10, 3).is_none());
        assert_eq!(m.pane_at(3, 2), Some(PaneId::HList));
        m.push(Rect::new(0, 0, 20, 20), Hit::MenuBackdrop);
        assert_eq!(m.pane_at(3, 2), None);
    }
}
