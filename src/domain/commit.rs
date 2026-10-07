use std::sync::Arc;

use super::oid::Oid;

/// Summary of a commit as shown in the history list.
#[derive(Clone, Debug)]
pub struct Commit {
    pub oid: Oid,
    pub parents: Box<[Oid]>,
    pub author: Arc<str>,
    pub date: Box<str>,
    pub subject: Box<str>,
}

/// Full commit information for the details pane.
#[derive(Clone, Debug, Default)]
pub struct CommitDetails {
    pub oid: Option<Oid>,
    pub parents: Vec<Oid>,
    pub author: String,
    pub author_date: String,
    pub committer: String,
    pub committer_date: String,
    pub message: String,
}

/// Simple string interner for author names (memory bound on huge histories).
#[derive(Default)]
pub struct Interner {
    map: std::collections::HashMap<Box<str>, Arc<str>>,
}

impl Interner {
    pub fn intern(&mut self, s: &str) -> Arc<str> {
        if let Some(a) = self.map.get(s) {
            return a.clone();
        }
        let a: Arc<str> = Arc::from(s);
        self.map.insert(Box::from(s), a.clone());
        a
    }
}
