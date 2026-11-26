//! A work-stealing pool of threads: what builds parts side by side.
//!
//! Every worker has a queue of its own. A task spawned from a worker goes
//! onto that worker's queue and is taken from there newest first (what it
//! needs is still warm); a worker with nothing to do takes the oldest
//! task of another's queue, or of the queue tasks from outside land on.
//! A thread that waits for a task's result does not sleep: it
//! [helps](Pool::help_until), running tasks itself until what it waits
//! for is there — so a part that waits for the parts it places never
//! holds a thread idle, and a pool of one thread cannot lock itself up.
//!
//! **Its threads are the host's.** On the desktop they are the standard
//! library's. In a browser the standard library cannot start threads:
//! there the host [says how](set_spawner) — a Web Worker that runs this
//! program on the page's memory — and the pool is the same. A browser's
//! page thread may never wait (it would stop the page): there, what it
//! [locks](lock) it spins for, and it helps or spins rather than
//! sleeps.

use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError, TryLockError};
use std::thread::JoinHandle;

/// What starts a thread that runs the work given to it, where the
/// standard library cannot (see [`set_spawner`]).
pub type Spawner = fn(Box<dyn FnOnce() + Send>) -> Result<(), String>;

static SPAWNER: OnceLock<Spawner> = OnceLock::new();

/// Has pools start their threads with `spawner` from now on: what a host
/// whose standard library has no threads (a browser) calls once, before
/// it makes a threaded [`Engine`](crate::Engine).
pub fn set_spawner(spawner: Spawner) {
    let _ = SPAWNER.set(spawner);
}

type Task = Box<dyn FnOnce() + Send>;

struct Shared {
    /// The queue of each worker, and last the one for tasks from
    /// outside the pool.
    queues: Vec<Mutex<VecDeque<Task>>>,
    /// How many tasks are queued: what idle workers sleep on.
    queued: Mutex<usize>,
    wake: Condvar,
    stop: AtomicBool,
}

thread_local! {
    /// Which worker of which pool this thread is.
    static WORKER: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

impl Shared {
    fn id(&self) -> usize {
        std::ptr::from_ref(self) as usize
    }

    /// The queue of the worker this thread is, or the outside queue.
    fn own(&self) -> usize {
        WORKER
            .get()
            .filter(|w| w.0 == self.id())
            .map_or(self.queues.len() - 1, |w| w.1)
    }

    fn push(&self, task: Task) {
        // Counted before it is queued: whoever takes it counts it off.
        *lock(&self.queued) += 1;
        let own = self.own();
        lock(&self.queues[own]).push_back(task);
        let _queued = lock(&self.queued);
        self.wake.notify_one();
    }

    /// A task to run: the newest of this thread's own, else the oldest
    /// of another queue.
    fn take(&self) -> Option<Task> {
        let own = self.own();
        // One queue locked at a time: two workers taking from each
        // other must not wait on each other.
        let mine = lock(&self.queues[own]).pop_back();
        let found = mine.or_else(|| {
            let n = self.queues.len();
            (1..n).find_map(|k| lock(&self.queues[(own + k) % n]).pop_front())
        });
        if found.is_some() {
            *lock(&self.queued) -= 1;
        }
        found
    }
}

/// Locks `mutex`, whatever panicked while it was held. In a browser it
/// spins until it has it: the page's thread may not wait, and what the
/// pool's threads hold they hold for a moment.
pub fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    if cfg!(target_arch = "wasm32") {
        loop {
            match mutex.try_lock() {
                Ok(guard) => return guard,
                Err(TryLockError::Poisoned(e)) => return e.into_inner(),
                Err(TryLockError::WouldBlock) => std::hint::spin_loop(),
            }
        }
    }
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The pool: its threads end when it is dropped.
pub struct Pool {
    shared: Arc<Shared>,
    /// The standard library's threads, to be joined; a host's own
    /// [spawner](set_spawner) ends its threads by itself.
    workers: Vec<JoinHandle<()>>,
    threads: usize,
}

impl std::fmt::Debug for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pool({} threads)", self.threads)
    }
}

/// What each thread of the pool does, until the pool is dropped.
fn work(shared: &Shared, k: usize) {
    WORKER.set(Some((shared.id(), k)));
    loop {
        if let Some(task) = shared.take() {
            // A task that panics does not take the worker with it.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
            continue;
        }
        let mut queued = lock(&shared.queued);
        while *queued == 0 && !shared.stop.load(Ordering::Acquire) {
            queued = shared
                .wake
                .wait(queued)
                .unwrap_or_else(PoisonError::into_inner);
        }
        if shared.stop.load(Ordering::Acquire) {
            break;
        }
    }
}

impl Pool {
    /// A pool of `threads` workers (at least one).
    pub fn new(threads: usize) -> Self {
        let threads = threads.max(1);
        let shared = Arc::new(Shared {
            queues: (0..=threads).map(|_| Mutex::new(VecDeque::new())).collect(),
            queued: Mutex::new(0),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
        });
        let mut workers = Vec::new();
        for k in 0..threads {
            let shared = shared.clone();
            match SPAWNER.get() {
                Some(spawn) => {
                    if let Err(e) = spawn(Box::new(move || work(&shared, k))) {
                        panic!("no thread for the pool: {e}");
                    }
                }
                None => workers.push(
                    std::thread::Builder::new()
                        .name(format!("workpiece-build-{k}"))
                        .spawn(move || work(&shared, k))
                        .expect("a thread for the pool"),
                ),
            }
        }
        Self {
            shared,
            workers,
            threads,
        }
    }

    /// Queues `task`.
    pub fn spawn(&self, task: impl FnOnce() + Send + 'static) {
        self.shared.push(Box::new(task));
    }

    /// Runs queued tasks on this thread until `done` says so; sleeps a
    /// moment at a time when there are none (the one waited for is being
    /// run by another thread) — in a browser it spins, as the page's
    /// thread may not sleep.
    pub fn help_until(&self, done: &dyn Fn() -> bool) {
        while !done() {
            // (Who waits is stopped like who works.)
            workpiece_core::cancel::checkpoint();
            match self.shared.take() {
                Some(task) => task(),
                None if cfg!(target_arch = "wasm32") => std::hint::spin_loop(),
                None => std::thread::sleep(std::time::Duration::from_micros(200)),
            }
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        {
            let _queued = lock(&self.shared.queued);
            self.shared.wake.notify_all();
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn tasks_that_wait_for_tasks_finish_on_one_thread() {
        let pool = Arc::new(Pool::new(1));
        let done = Arc::new(AtomicUsize::new(0));
        // A task that spawns ten and waits for them, on a pool of one.
        let (inner, count) = (pool.clone(), done.clone());
        pool.spawn(move || {
            let mine = Arc::new(AtomicUsize::new(0));
            for _ in 0..10 {
                let mine = mine.clone();
                inner.spawn(move || {
                    mine.fetch_add(1, Ordering::SeqCst);
                });
            }
            inner.help_until(&|| mine.load(Ordering::SeqCst) == 10);
            count.store(mine.load(Ordering::SeqCst), Ordering::SeqCst);
        });
        pool.help_until(&|| done.load(Ordering::SeqCst) == 10);
        assert_eq!(done.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn many_threads_share_the_work() {
        let pool = Pool::new(4);
        let sum = Arc::new(AtomicUsize::new(0));
        for k in 0..1000 {
            let sum = sum.clone();
            pool.spawn(move || {
                sum.fetch_add(k, Ordering::SeqCst);
            });
        }
        pool.help_until(&|| sum.load(Ordering::SeqCst) == 499_500);
        assert_eq!(sum.load(Ordering::SeqCst), 499_500);
    }
}
