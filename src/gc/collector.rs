use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use crate::{
    gc::{
        card_table::CardTable, marker::Marker, promoter::Promoter, root::RootRegistry,
        safepoint::SafepointCoordinator, sweeper::Sweeper,
    },
    heap::{bump::BumpAllocator, freelist::FreeListAllocator, region::Region},
};

pub struct Collector {
    // `Box<T>` for stable heap addresses
    young_region: Box<Region>,
    old_region: Box<Region>,

    // allocator and GC state with interior mutability so mutators can
    // hold shared references to regions while collector mutates these.
    pub bump: Rc<RefCell<BumpAllocator>>,
    pub freelist: RefCell<FreeListAllocator>,

    // GC components
    card_table: CardTable,
    marker: RefCell<Marker>,
    promoter: RefCell<Promoter>,

    // shared across all mutator threads
    pub safepoint: Arc<SafepointCoordinator>,
    minor_collections: AtomicUsize,
    major_collections: AtomicUsize,
}

pub trait GcTrigger {
    fn collect_minor(&self, roots: &RootRegistry);
    fn collect_major(&self, roots: &RootRegistry);
}

impl Collector {
    pub fn new(young_size: usize, old_size: usize) -> Self {
        let young_region = Region::new(young_size).expect("couldn't allocate young gen");
        let old_region = Region::new(old_size).expect("couldn't allocate old gen");

        let bump = BumpAllocator::from_region(&young_region);
        let mut freelist = FreeListAllocator::from_region(&old_region);
        freelist.free(old_region.base(), old_region.size());
        let card_table = CardTable::new(old_region.base(), old_size);

        Self {
            young_region: Box::new(young_region),
            old_region: Box::new(old_region),
            bump: Rc::new(RefCell::new(bump)),
            freelist: RefCell::new(freelist),
            card_table,
            marker: RefCell::new(Marker::default()),
            // For the current bump-only young space, we promote all survivors.
            promoter: RefCell::new(Promoter::new(0)),
            safepoint: Arc::new(SafepointCoordinator::new()),
            minor_collections: AtomicUsize::new(0),
            major_collections: AtomicUsize::new(0),
        }
    }

    /// roots: the mutator thread's `RootRegistry` (shadow stack + globals)
    pub fn collect_minor(&self, roots: &RootRegistry) {
        let young_before = self.bump.borrow().used();
        let old_before = self.old_gen_used();

        let do_stw = self.safepoint.thread_count() > 1;
        if do_stw {
            self.safepoint.request_safepoint();
            self.safepoint.wait_for_all_threads();
        }

        // MARK eden
        self.marker.borrow_mut().mark_minor(
            roots,
            &self.card_table,
            self.old_region.as_ref(),
            self.young_region.as_ref(),
        );

        let stats = {
            let mut bump = self.bump.borrow_mut();
            let mut promoter = self.promoter.borrow_mut();
            let mut freelist = self.freelist.borrow_mut();
            Sweeper::sweep_young(
                &mut bump,
                &mut promoter,
                &mut freelist,
                roots,
                &self.card_table,
                self.old_region.as_ref(),
            )
        };

        self.minor_collections.fetch_add(1, Ordering::Relaxed);

        println!(
            "[gc] minor: roots={} young_before={}B old_before={}B promoted={} live_objects={} freed={}B live_bytes={}B",
            roots.root_count(),
            young_before,
            old_before,
            stats.promoted_objects,
            stats.live_objects,
            stats.bytes_freed,
            stats.bytes_live,
        );

        if do_stw {
            self.safepoint.release_threads();
        }

        // if old gen filling up (>80% old gen used)
        if self.old_gen_used() > (self.old_region.size() * 8 / 10) {
            self.collect_major(roots);
        }
    }

    pub fn collect_major(&self, roots: &RootRegistry) {
        let old_before = self.old_gen_used();

        let do_stw = self.safepoint.thread_count() > 1;
        if do_stw {
            self.safepoint.request_safepoint();
            self.safepoint.wait_for_all_threads();
        }

        // MARK tenured
        self.marker
            .borrow_mut()
            .mark_major(roots, self.old_region.as_ref());

        let stats = {
            let mut freelist = self.freelist.borrow_mut();
            Sweeper::sweep_old(&mut freelist, self.old_region.as_ref())
        };

        // RESET worklist
        self.marker.borrow_mut().reset();
        self.major_collections.fetch_add(1, Ordering::Relaxed);

        println!(
            "[gc] major: roots={} old_before={}B live_objects={} freed={}B live_bytes={}B",
            roots.root_count(),
            old_before,
            stats.live_objects,
            stats.bytes_freed,
            stats.bytes_live,
        );

        if do_stw {
            self.safepoint.release_threads();
        }
    }

    // helper

    pub fn young_region(&self) -> &Region {
        &self.young_region
    }

    pub fn old_region(&self) -> &Region {
        &self.old_region
    }

    pub fn card_table(&self) -> &CardTable {
        &self.card_table
    }

    pub fn old_gen_used(&self) -> usize {
        self.old_region.size() - self.freelist.borrow().free_bytes()
    }

    pub fn young_gen_used(&self) -> usize {
        self.bump.borrow().used()
    }

    pub fn tlab(&self) -> Rc<RefCell<BumpAllocator>> {
        self.bump.clone()
    }

    pub fn minor_collections(&self) -> usize {
        self.minor_collections.load(Ordering::Relaxed)
    }

    pub fn major_collections(&self) -> usize {
        self.major_collections.load(Ordering::Relaxed)
    }
}

impl GcTrigger for Collector {
    fn collect_minor(&self, roots: &RootRegistry) {
        Collector::collect_minor(self, roots);
    }

    fn collect_major(&self, roots: &RootRegistry) {
        Collector::collect_major(self, roots);
    }
}
