use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub struct SafepointCoordinator {
    requested: AtomicBool,

    /// no. of mutator threads (registed at thread spawn)
    thread_count: AtomicUsize,

    /// no. of mutators stopped at safepoint and parked
    parked_count: AtomicUsize,

    /// GC waits for all mutators to park
    gc_barrier: (Mutex<bool>, Condvar),

    /// mutator threads wait here until GC releases them
    resume_barrier: (Mutex<bool>, Condvar),
}

impl SafepointCoordinator {
    pub fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            thread_count: AtomicUsize::new(0),
            parked_count: AtomicUsize::new(0),
            gc_barrier: (Mutex::new(false), Condvar::new()),
            resume_barrier: (Mutex::new(false), Condvar::new()),
        }
    }

    // add new mutator thread
    pub fn register_thread(&self) {
        self.thread_count.fetch_add(1, Ordering::Relaxed);
    }

    // remove mutator thread
    pub fn unregister_thread(&self) {
        self.thread_count.fetch_sub(1, Ordering::Relaxed);
    }

    /// sets the flag, mutator threads will see this in the next poll
    pub fn request_safepoint(&self) {
        {
            // close so mutators block when they park
            let mut released = self.resume_barrier.0.lock().unwrap();
            *released = false;
        }

        self.requested.store(true, Ordering::SeqCst);
    }

    /// GC block till all mutator threads call `park()`
    pub fn wait_for_all_threads(&self) {
        let (lock, cvar) = &self.gc_barrier;
        let mut all_parked = lock.lock().unwrap();

        all_parked = cvar
            .wait_while(all_parked, |_| {
                self.parked_count.load(Ordering::SeqCst) < self.thread_count.load(Ordering::SeqCst)
            })
            .unwrap();

        // reset the gc gate for next cycle
        *all_parked = false;
    }

    pub fn release_threads(&self) {
        self.requested.store(false, Ordering::SeqCst);
        {
            let mut released = self.resume_barrier.0.lock().unwrap();
            *released = true;
        }

        self.resume_barrier.1.notify_all();

        // reset for next cycle
        self.parked_count.store(0, Ordering::SeqCst);
    }

    /// Called by mutators
    #[inline]
    pub fn poll(&self) -> bool {
        self.requested.load(Ordering::Relaxed)
    }

    /// Called by mutators when poll() returns `true`
    fn park(&self) {
        let prev = self.parked_count.fetch_add(1, Ordering::Relaxed);
        let thread_count = self.thread_count.load(Ordering::Relaxed);

        // if last thread, run GC
        if prev + 1 == thread_count {
            let (lock, cvar) = &self.gc_barrier;
            let mut all_parked = lock.lock().unwrap();
            *all_parked = true;
            cvar.notify_all();
        }
        // block on the resume barrier until GC is done
        let (lock, cvar) = &self.resume_barrier;
        let released = lock.lock().unwrap();
        let _wait = cvar.wait_while(released, |&mut r| !r).unwrap();
    }

    #[inline]
    pub fn poll_and_park(&self) {
        if self.poll() {
            self.park();
        }
    }
}

