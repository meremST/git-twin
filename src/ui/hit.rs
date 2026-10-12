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
    CommitRow(usize),
    RefLabel(usize, Vec<u8>),
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
    pub fn pane(&self) -> Option<PaneId> {
        match self {
            Hit::Pane(p) | Hit::DiffLine(p, _) => Some(*p),
            Hit::CommitRow(_) | Hit::RefLabel(..) | Hit::CutArrow(..) | Hit::ResumeArrow { .. } => {
                Some(PaneId::HList)
            }
            Hit::HFileRow(_) => Some(PaneId::HFiles),
            Hit::WtFile(FileList::Unstaged, ..) => Some(PaneId::WUnstaged),
            Hit::WtFile(FileList::Staged, ..) => Some(PaneId::WStaged),
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
