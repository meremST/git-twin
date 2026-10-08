//! Background workers: a small pool for reads, one serialized writer, and
//! dedicated threads for long streams (history, find). Results carry the
//! request id and the repository generation so that stale answers can be
//! dropped by the UI.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};

use crate::domain::commit::{Commit, CommitDetails};
use crate::domain::diff::Diff;
use crate::domain::graph::GraphRow;
use crate::domain::oid::Oid;
use crate::domain::refs::RefSet;
use crate::domain::status::RepoStatus;
use crate::git::GitError;
use crate::git::read::{Blob, Related};
use crate::git::repo::RepoState;

/// Snapshot of the working tree for the git gui view.
#[derive(Clone, Debug, Default)]
pub struct WtSnapshot {
    pub status: RepoStatus,
    pub state: RepoState,
    /// Amend mode: staged entries against HEAD^ (status, path).
    pub amend_staged: Option<Vec<(u8, Vec<u8>)>>,
    /// HEAD^ in amend mode (None = root commit, diff against the empty tree).
    pub amend_base: Option<Oid>,
    pub head: Option<Oid>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum After {
    Nothing,
    /// Re-read everything (refs, history, status).
    Refresh,
    /// Only the working tree status changed.
    Rescan,
    /// Re-read only refs (HEAD and branch labels); history rows are kept.
    RefsOnly,
    /// Clear the commit message (successful commit).
    Committed,
    /// Select a commit after the refresh.
    Select(Oid),
    /// Branch deletion failed because the branch is not merged: offer -D.
    OfferForceDelete(Vec<u8>),
    /// Record undo information for git gui "Undo Last Revert".
    Reverted(Vec<u8>),
}

pub enum Payload {
    LogBatch {
        commits: Vec<Commit>,
        graph: Vec<GraphRow>,
    },
    LogDone(Result<usize, GitError>),
    Refs(Result<RefSet, GitError>),
    Details {
        oid: Oid,
        details: Result<CommitDetails, GitError>,
        related: Related,
    },
    CommitDiff(Result<Diff, GitError>),
    Tree(Result<Vec<Vec<u8>>, GitError>),
    Blob {
        path: Vec<u8>,
        blob: Result<Blob, GitError>,
    },
    Find(Result<HashSet<Oid>, GitError>),
    Highlight(Result<HashSet<Oid>, GitError>),
    Status(Result<WtSnapshot, GitError>),
    WtDiff(Result<Diff, GitError>),
    Op {
        label: String,
        result: Result<String, GitError>,
        after: After,
    },
    Text {
        title: String,
        result: Result<String, GitError>,
    },
    Goto(Result<Oid, GitError>),
    Origin(Result<(Oid, u32), GitError>),
    Amend(Result<String, GitError>),
    Remotes(Vec<String>),
    TagInfo {
        full: Vec<u8>,
        result: Result<Option<crate::git::read::TagDetails>, GitError>,
    },
}

pub struct Msg {
    pub id: u64,
    pub generation: u64,
    pub payload: Payload,
}

type Job = Box<dyn FnOnce() -> Payload + Send>;

struct Envelope {
    id: u64,
    generation: u64,
    job: Job,
}

/// Streaming sink handed to long-running jobs.
#[derive(Clone)]
pub struct Sink {
    id: u64,
    generation: u64,
    tx: SyncSender<Msg>,
}

impl Sink {
    pub fn send(&self, payload: Payload) -> bool {
        self.tx
            .send(Msg {
                id: self.id,
                generation: self.generation,
                payload,
            })
            .is_ok()
    }
}

pub struct Workers {
    read_tx: SyncSender<Envelope>,
    write_tx: SyncSender<Envelope>,
    tx: SyncSender<Msg>,
    pub rx: Receiver<Msg>,
    next_id: u64,
    pub writes: Arc<AtomicUsize>,
    pub reads: Arc<AtomicUsize>,
    /// Description of the running mutation (for the quit dialog).
    pub write_label: Arc<Mutex<String>>,
}

fn worker_loop(rx: Arc<Mutex<Receiver<Envelope>>>, tx: SyncSender<Msg>, counter: Arc<AtomicUsize>) {
    loop {
        let env = {
            let Ok(guard) = rx.lock() else { return };
            match guard.recv() {
                Ok(e) => e,
                Err(_) => return,
            }
        };
        let payload = (env.job)();
        counter.fetch_sub(1, Ordering::SeqCst);
        if tx
            .send(Msg {
                id: env.id,
                generation: env.generation,
                payload,
            })
            .is_err()
        {
            return;
        }
    }
}

impl Workers {
    pub fn new(readers: usize) -> Workers {
        let (tx, rx) = sync_channel::<Msg>(1024);
        let (read_tx, read_rx) = sync_channel::<Envelope>(256);
        let (write_tx, write_rx) = sync_channel::<Envelope>(64);
        let reads = Arc::new(AtomicUsize::new(0));
        let writes = Arc::new(AtomicUsize::new(0));
        let read_rx = Arc::new(Mutex::new(read_rx));
        for i in 0..readers.max(1) {
            let (r, t, c) = (read_rx.clone(), tx.clone(), reads.clone());
            let _ = std::thread::Builder::new()
                .name(format!("git-read-{i}"))
                .spawn(move || worker_loop(r, t, c));
        }
        let write_rx = Arc::new(Mutex::new(write_rx));
        {
            let (t, c) = (tx.clone(), writes.clone());
            let _ = std::thread::Builder::new()
                .name("git-write".into())
                .spawn(move || worker_loop(write_rx, t, c));
        }
        Workers {
            read_tx,
            write_tx,
            tx,
            rx,
            next_id: 1,
            writes,
            reads,
            write_label: Arc::new(Mutex::new(String::new())),
        }
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Queues a read. Returns the request id (0 if the queue is full).
    pub fn read(&mut self, generation: u64, job: impl FnOnce() -> Payload + Send + 'static) -> u64 {
        let id = self.id();
        self.reads.fetch_add(1, Ordering::SeqCst);
        match self.read_tx.try_send(Envelope {
            id,
            generation,
            job: Box::new(job),
        }) {
            Ok(()) => id,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.reads.fetch_sub(1, Ordering::SeqCst);
                0
            }
        }
    }

    /// Queues a mutation; mutations run one at a time in order.
    pub fn write(
        &mut self,
        generation: u64,
        label: &str,
        job: impl FnOnce() -> Payload + Send + 'static,
    ) -> u64 {
        let id = self.id();
        self.writes.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut l) = self.write_label.lock() {
            *l = label.to_string();
        }
        match self.write_tx.try_send(Envelope {
            id,
            generation,
            job: Box::new(job),
        }) {
            Ok(()) => id,
            Err(_) => {
                self.writes.fetch_sub(1, Ordering::SeqCst);
                0
            }
        }
    }

    /// Runs a streaming job on its own thread.
    pub fn stream(&mut self, generation: u64, job: impl FnOnce(Sink) + Send + 'static) -> u64 {
        let id = self.id();
        let sink = Sink {
            id,
            generation,
            tx: self.tx.clone(),
        };
        let _ = std::thread::Builder::new()
            .name("git-stream".into())
            .spawn(move || job(sink));
        id
    }

    pub fn writing(&self) -> bool {
        self.writes.load(Ordering::SeqCst) > 0
    }

    pub fn busy(&self) -> bool {
        self.writing() || self.reads.load(Ordering::SeqCst) > 0
    }
}

/// Cancellation flag shared with a job.
#[derive(Clone, Default)]
pub struct Cancel(pub Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn flag(&self) -> &AtomicBool {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_complete_in_order() {
        let mut w = Workers::new(2);
        let a = w.write(1, "a", || Payload::Remotes(vec!["a".into()]));
        let b = w.write(1, "b", || Payload::Remotes(vec!["b".into()]));
        let mut got = Vec::new();
        for _ in 0..2 {
            let m =
                w.rx.recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
            got.push(m.id);
        }
        assert_eq!(got, vec![a, b]);
        assert!(!w.writing());
        let r = w.read(7, || Payload::Remotes(Vec::new()));
        let m =
            w.rx.recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
        assert_eq!((m.id, m.generation), (r, 7));
    }

    #[test]
    fn stream_sends_many() {
        let mut w = Workers::new(1);
        let id = w.stream(3, |s| {
            for _ in 0..3 {
                s.send(Payload::Remotes(Vec::new()));
            }
        });
        for _ in 0..3 {
            let m =
                w.rx.recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
            assert_eq!(m.id, id);
        }
    }
}
