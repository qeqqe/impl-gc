use std::sync::atomic::AtomicU8;

use crate::{
    gc::{card_table::CardTable, root::RootRegistry, safepoint::SafepointCoordinator},
    heap::{bump::BumpAllocator, region::Region},
    object::{
        descriptor::TypeDescriptor,
        header::{GcHeader, MarkColor},
        pointer::GcPtr,
    },
};

pub enum AllocResult {
    /// successfull allocation.
    Ok(GcPtr<()>),

    /// TLAB is full, or ran just before the major gc.
    NeedMinorGC,

    /// MinorGc ran and tried to alloc to major Gc but it's full.
    NeedMajorGC,

    // unsuccessfull allocation even after compaction.
    OutOfMemory,
}

/// Result returned by `alloc()`. Handled by the interpreter loop.
/// prevent's circular dependencies between `Mutator` and `Collector`.
pub struct Mutator<'gc> {
    /// Owned by the mutator threads,
    pub tlab: BumpAllocator<'gc>,

    root: RootRegistry,

    pub card_table: &'gc CardTable,

    pub young_gen: &'gc Region,
    pub old_gen: &'gc Region,

    pub safepoint: &'gc SafepointCoordinator,
}

impl<'gc> Mutator<'gc> {
    pub fn new(
        tlab: BumpAllocator<'gc>,
        card_table: &'gc CardTable,
        young_gen: &'gc Region,
        old_gen: &'gc Region,
        safepoint: &'gc SafepointCoordinator,
    ) -> Self {
        Self {
            tlab,
            root: RootRegistry::new(),
            card_table,
            young_gen,
            old_gen,
            safepoint,
        }
    }

    /// First path allocation.
    /// Interpreter can handle GC triggers without any
    /// circular dependencies.
    ///
    /// example:
    ///
    /// ```rust
    /// loop {
    ///     match mutator.alloc() {
    ///         AllocResult::Ok(gc_ptr) =>  { /* use ptr */ break; },
    ///         AllocResult::NeedMinorGC => { collector.sweep_young() },
    ///         AllocResult::NeedMajorGC => { collector.sweep_old() },
    ///         AllocResult::OutOfMemory => { panic!("java.lang.OutOfMemoryError") },
    ///     }
    /// }
    /// ```
    pub fn alloc(&mut self, type_desc: &'static TypeDescriptor) -> AllocResult {
        let total_size = type_desc.instance_size + std::mem::size_of::<GcHeader>();
        let align = std::mem::align_of::<GcHeader>();

        match self.tlab.alloc(total_size, align) {
            Some(raw) => {
                // returns a pointer to the head of the gcheader
                let header_ptr = raw.as_ptr() as *mut GcHeader;

                unsafe {
                    // write in place
                    header_ptr.write(GcHeader {
                        mark: AtomicU8::new(MarkColor::White as u8),
                        age: 0,
                        flag: 0,
                        _pad: 0,
                        type_desc: type_desc as *const TypeDescriptor,
                        size: total_size as u32,
                    });
                }

                let payload = unsafe { (*header_ptr).object_start() };

                unsafe { std::ptr::write_bytes(payload, 0, type_desc.instance_size) };

                let gc_ptr = unsafe { GcPtr::from_raw(header_ptr) };

                AllocResult::Ok(gc_ptr)
            }
            None => AllocResult::NeedMinorGC,
        }
    }
}
