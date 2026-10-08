//! How far each stage of a frame has got, for the stages behind it to wait
//! on: a tile's parsing by rows, a row's reconstruction and its filtering by
//! 64×64 blocks.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Condvar, Mutex};

const FAILED: u32 = u32::MAX;

/// One counter. The store and the loads are sequentially consistent, which is
/// what makes what was written before an advance visible to whoever waited
/// for it, and what lets an advance skip the notify when no one waits.
pub(crate) struct Progress {
    done: AtomicU32,
    waiters: AtomicU32,
    lock: Mutex<()>,
    cv: Condvar,
}

impl Default for Progress {
    fn default() -> Self {
        Progress { done: AtomicU32::new(0), waiters: AtomicU32::new(0), lock: Mutex::new(()), cv: Condvar::new() }
    }
}

impl Progress {
    pub fn advance(&self, n: usize) {
        self.done.store(n as u32, Ordering::SeqCst);
        if self.waiters.load(Ordering::SeqCst) > 0 {
            let _g = self.lock.lock();
            self.cv.notify_all();
        }
    }

    /// It will not finish: let whoever waits on it give up.
    pub fn fail(&self) {
        self.advance(FAILED as usize);
    }

    /// Waits for the count to reach `n`; false if it failed instead. The
    /// stages run in step, so the count is usually there or about to be: the
    /// wait looks a few hundred times before it sleeps.
    pub fn wait_for(&self, n: usize) -> bool {
        let n = n as u32;
        let mut v = self.done.load(Ordering::SeqCst);
        let mut spins = 0;
        while v < n && spins < 256 {
            std::hint::spin_loop();
            spins += 1;
            v = self.done.load(Ordering::SeqCst);
        }
        if v < n {
            self.waiters.fetch_add(1, Ordering::SeqCst);
            let mut g = self.lock.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                v = self.done.load(Ordering::SeqCst);
                if v >= n {
                    break;
                }
                g = self.cv.wait(g).unwrap_or_else(|e| e.into_inner());
            }
            drop(g);
            self.waiters.fetch_sub(1, Ordering::SeqCst);
        }
        v != FAILED
    }
}
