use std::collections::HashMap;

use super::oid::Oid;
use super::sanitize;

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub enum RefKind {
    LocalBranch,
    RemoteBranch,
    Tag,
    Stash,
    Other,
}

#[derive(Clone, Debug)]
pub struct GitRef {
    /// Full refname bytes, used as identity for commands.
    pub full: Vec<u8>,
    /// Sanitized short name for display.
    pub short: String,
    pub kind: RefKind,
    /// Commit pointed to (tags are peeled).
    pub target: Oid,
}

impl GitRef {
    pub fn from_full(full: &[u8], target: Oid) -> GitRef {
        let (kind, prefix): (RefKind, &[u8]) = if full.starts_with(b"refs/heads/") {
            (RefKind::LocalBranch, b"refs/heads/")
        } else if full.starts_with(b"refs/remotes/") {
            (RefKind::RemoteBranch, b"refs/remotes/")
        } else if full.starts_with(b"refs/tags/") {
            (RefKind::Tag, b"refs/tags/")
        } else if full == b"refs/stash" {
            (RefKind::Stash, b"refs/")
        } else {
            (RefKind::Other, b"refs/")
        };
        let short_bytes = full.strip_prefix(prefix).unwrap_or(full);
        GitRef {
            full: full.to_vec(),
            short: sanitize::display_bytes(short_bytes),
            kind,
            target,
        }
    }

    /// Short name as raw bytes (for commands such as `git branch -m`).
    pub fn short_bytes(&self) -> &[u8] {
        for p in [
            &b"refs/heads/"[..],
            b"refs/remotes/",
            b"refs/tags/",
            b"refs/",
        ] {
            if let Some(s) = self.full.strip_prefix(p) {
                return s;
            }
        }
        &self.full
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    /// HEAD is a symbolic ref; `oid` is None on an unborn branch.
    Branch {
        full: Vec<u8>,
        short: String,
        oid: Option<Oid>,
    },
    Detached(Oid),
}

impl Head {
    pub fn oid(&self) -> Option<Oid> {
        match self {
            Head::Branch { oid, .. } => *oid,
            Head::Detached(o) => Some(*o),
        }
    }

    pub fn branch_full(&self) -> Option<&[u8]> {
        match self {
            Head::Branch { full, .. } => Some(full),
            Head::Detached(_) => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Head::Branch { short, oid: None, .. } => format!("{short} (unborn)"),
            Head::Branch { short, .. } => short.clone(),
            Head::Detached(o) => format!("HEAD detached at {}", o.short(8)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RefSet {
    pub refs: Vec<GitRef>,
    pub head: Head,
    by_commit: HashMap<Oid, Vec<usize>>,
}

impl RefSet {
    pub fn new(mut refs: Vec<GitRef>, head: Head) -> RefSet {
        refs.sort_by(|a, b| (a.kind, &a.short).cmp(&(b.kind, &b.short)));
        let mut by_commit: HashMap<Oid, Vec<usize>> = HashMap::new();
        for (i, r) in refs.iter().enumerate() {
            by_commit.entry(r.target).or_default().push(i);
        }
        RefSet {
            refs,
            head,
            by_commit,
        }
    }

    pub fn empty() -> RefSet {
        RefSet::new(
            Vec::new(),
            Head::Branch {
                full: b"refs/heads/master".to_vec(),
                short: "master".into(),
                oid: None,
            },
        )
    }

    pub fn for_commit(&self, oid: &Oid) -> impl Iterator<Item = &GitRef> {
        self.by_commit
            .get(oid)
            .into_iter()
            .flat_map(move |v| v.iter().map(move |i| &self.refs[*i]))
    }

    pub fn is_head_ref(&self, r: &GitRef) -> bool {
        self.head.branch_full() == Some(r.full.as_slice())
    }

    pub fn local_branches(&self) -> impl Iterator<Item = &GitRef> {
        self.refs.iter().filter(|r| r.kind == RefKind::LocalBranch)
    }

    pub fn find_full(&self, full: &[u8]) -> Option<&GitRef> {
        self.refs.iter().find(|r| r.full == full)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_refs() {
        let o = Oid::from_hex(&"1".repeat(40)).unwrap();
        let r = GitRef::from_full(b"refs/heads/feature/x", o);
        assert_eq!(r.kind, RefKind::LocalBranch);
        assert_eq!(r.short, "feature/x");
        assert_eq!(r.short_bytes(), b"feature/x");
        let r = GitRef::from_full(b"refs/remotes/origin/main", o);
        assert_eq!(r.kind, RefKind::RemoteBranch);
        assert_eq!(r.short, "origin/main");
        let r = GitRef::from_full(b"refs/tags/v1", o);
        assert_eq!(r.kind, RefKind::Tag);
        let set = RefSet::new(
            vec![r],
            Head::Branch {
                full: b"refs/heads/main".to_vec(),
                short: "main".into(),
                oid: Some(o),
            },
        );
        assert_eq!(set.for_commit(&o).count(), 1);
    }
}
