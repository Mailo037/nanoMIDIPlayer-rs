//! Delayed key releases for "Custom Hold Length". One thread with a min-heap replaces
//! the original's one-`threading.Timer`-per-note approach.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};

type Job = Box<dyn FnOnce() + Send>;

enum Msg {
    Schedule { at: Instant, group: u32, job: Job },
    ClearGroup(u32),
}

static TX: OnceLock<Sender<Msg>> = OnceLock::new();

fn sender() -> &'static Sender<Msg> {
    TX.get_or_init(|| {
        let (tx, rx) = unbounded();
        std::thread::Builder::new()
            .name("release-scheduler".into())
            .spawn(move || run(rx))
            .expect("spawn scheduler");
        tx
    })
}

fn run(rx: Receiver<Msg>) {
    let mut heap: BinaryHeap<Reverse<(Instant, u64)>> = BinaryHeap::new();
    let mut jobs: HashMap<u64, (u32, Job)> = HashMap::new();
    let mut seq = 0u64;
    loop {
        let timeout = heap
            .peek()
            .map(|Reverse((at, _))| at.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(3600));
        match rx.recv_timeout(timeout) {
            Ok(Msg::Schedule { at, group, job }) => {
                seq += 1;
                heap.push(Reverse((at, seq)));
                jobs.insert(seq, (group, job));
            }
            Ok(Msg::ClearGroup(group)) => jobs.retain(|_, (g, _)| *g != group),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(_) => return,
        }
        let now = Instant::now();
        while let Some(Reverse((at, id))) = heap.peek().copied() {
            if at > now {
                break;
            }
            heap.pop();
            if let Some((_, job)) = jobs.remove(&id) {
                job();
            }
        }
    }
}

/// Runs `job` after `delay`. Jobs are grouped so a stopping player can cancel its own.
pub fn schedule(group: u32, delay: Duration, job: impl FnOnce() + Send + 'static) {
    let _ = sender().send(Msg::Schedule {
        at: Instant::now() + delay,
        group,
        job: Box::new(job),
    });
}

pub fn clear(group: u32) {
    let _ = sender().send(Msg::ClearGroup(group));
}
