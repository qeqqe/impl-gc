use std::sync::Arc;

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

    // allocators. initialized from regions, no borrow needed
    pub bump: BumpAllocator,
    pub freelist: FreeListAllocator,

    // GC components
    card_table: CardTable,
    marker: Marker,
    promoter: Promoter,

    // shared across all mutator threads
    pub safepoint: Arc<SafepointCoordinator>,
}

pub trait GcTrigger {
    fn collect_minor(&mut self, roots: &RootRegistry);
    fn collect_major(&mut self, roots: &RootRegistry);
}

impl Collector {
    pub fn new(young_size: usize, old_size: usize) -> Self {
        let young_region = Region::new(young_size).expect("couldn't allocate young gen");
        let old_region = Region::new(old_size).expect("couldn't allocate young gen");

        let bump = BumpAllocator::from_region(&young_region);
        let freelist = FreeListAllocator::from_region(&old_region);
        let card_table = CardTable::new(old_region.base(), old_size);

        Self {
            young_region: Box::new(young_region),
            old_region: Box::new(old_region),
            bump,
            freelist,
            card_table,
            marker: Marker::default(),
            promoter: Promoter::new(2),
            safepoint: Arc::new(SafepointCoordinator::new()),
        }
    }

    /// roots: the mutator thread's `RootRegistry` (shadow stack + globals)
    pub fn collect_minor(&mut self, roots: &RootRegistry) {
        // STW
        self.safepoint.request_safepoint();
        self.safepoint.wait_for_all_threads();

        // MARK eden
        self.marker.mark_minor(
            roots,
            &self.card_table,
            &self.old_region,
            &self.young_region,
        );

        let stats = Sweeper::sweep_young(
            &mut self.bump,
            &mut self.promoter,
            &mut self.freelist,
            roots,
            &self.card_table,
            &self.old_region,
        );

        log::debug!(
            "minor GC: promoted={}, freed={}, live={}",
            stats.promoted_objects,
            stats.bytes_freed,
            stats.bytes_live
        );

        // resume
        self.safepoint.release_threads();

        // if old gen filling up (>80% old gen used)
        if self.old_gen_used() > (self.old_region.size() * 8 / 10) {
            self.collect_major(roots);
        }
    }

    pub fn collect_major(&mut self, roots: &RootRegistry) {
        // STW
        self.safepoint.request_safepoint();
        self.safepoint.wait_for_all_threads();

        // MARK tenured
        self.marker.mark_major(roots, &self.old_region);

        let stats = Sweeper::sweep_old(&mut self.freelist, &self.old_region);

        // RESET worklist
        self.marker.reset();

        log::debug!(
            "major GC: freed={} live={}",
            stats.bytes_freed,
            stats.bytes_live
        );

        // RESUME mutator
        self.safepoint.release_threads();
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
        self.old_region.size() - self.freelist.free_bytes()
    }
}

impl GcTrigger for Collector {
    fn collect_minor(&mut self, roots: &RootRegistry) {
        Collector::collect_minor(self, roots);
    }

    fn collect_major(&mut self, roots: &RootRegistry) {
        Collector::collect_major(self, roots);
    }
}
