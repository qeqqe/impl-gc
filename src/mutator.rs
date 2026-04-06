use std::sync::atomic::AtomicU8;

use crate::{
    gc::{
        card_table::CardTable,
        root::{RootRegistry, StackFrame},
        safepoint::SafepointCoordinator,
    },
    heap::{bump::BumpAllocator, region::Region},
    object::{
        descriptor::TypeDescriptor,
        header::{GcHeader, MarkColor},
        pointer::GcPtr,
    },
};

/// Returned by `collector.alloc()`
/// prevent's circular dependencies between `Mutator` and `Collector`.
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
pub struct Mutator<'gc> {
    /// Owned by the mutator threads,
    pub tlab: BumpAllocator,

    roots: RootRegistry,

    pub card_table: &'gc CardTable,

    pub young_gen: &'gc Region,
    pub old_gen: &'gc Region,

    pub safepoint: &'gc SafepointCoordinator,
}

impl<'gc> Mutator<'gc> {
    pub fn new(
        tlab: BumpAllocator,
        card_table: &'gc CardTable,
        young_gen: &'gc Region,
        old_gen: &'gc Region,
        safepoint: &'gc SafepointCoordinator,
    ) -> Self {
        Self {
            tlab,
            roots: RootRegistry::new(),
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

    /// Must be called by the interpreter on EVERY pointer write into a GC object.
    ///
    /// Java bytecodes that needs this:
    ///   PUTFIELD  (write instance field)
    ///   PUTSTATIC (write static field, treat static area as old-gen)
    ///   AASTORE   (write into reference array)
    ///
    /// Only mark card dirty if:
    ///   - holder lives in old gen (old-gen writes are what we track)
    ///   - new_value lives in young gen (cross-gen pointer = potential missed root)
    ///
    /// Writing null or a non-heap value: pass null for `new_value`, barrier no-ops
    #[inline]
    pub fn write_barrier(
        &self,
        holder: *mut GcHeader,    // obj being written INTO
        field_offset: usize,      // byte offset of the field within `object_start()`
        new_value: *mut GcHeader, // value being stored
    ) {
        // skip if new_value is null or not a young-gen (fast path)
        if new_value.is_null() {
            return;
        }

        unsafe {
            let holder_obj = (*holder).object_start() as *const u8;
            let new_value_obj = (*new_value).object_start() as *const u8;

            if self.old_gen.contains(holder_obj) && self.young_gen.contains(new_value_obj) {
                // mark card covering `holder` as dirty
                self.card_table.mark_dirty(holder_obj);
            }

            // Perform the actual write, update the field
            // The field stores a user data pointer (*mut u8), not a GcHeader pointer
            let field_slot = holder_obj.add(field_offset) as *mut *mut GcHeader;
            *field_slot = new_value;
        }
    }

    /// Called on:
    ///     - loop back-edge (GOTO, IF_* branching backward. if we never
    ///        poll the safepoint here, a thread in a tight loop runs forever
    ///        without the GC ever getting a chance to stop it)
    ///
    ///     - method invocation boundry (INVOKE*)
    ///
    ///     - allocation site (before alloc)
    /// Hot path: single atomic load when no GC in progress (zero cost).
    #[inline]
    pub fn safepoint(&self) {
        self.safepoint.poll_and_park();
    }

    /// Push a new interpreter frame's GC roots onto the shadow stack.
    /// Call this immediately after entering a bytecode method.
    ///
    /// `frame.slots` must contain *mut *mut GcHeader for every local variable
    /// slot and operand stack slot in this frame that can hold an object ref.
    pub fn push_frame(&mut self, frame: StackFrame) {
        self.roots.push_frame(frame);
    }

    /// Pop the current frame's roots when returning from a method.
    /// Must be called in all exit paths — normal return AND exception unwind.
    pub fn pop_frame(&mut self) {
        self.roots.pop_frame();
    }

    /// Register a global/static root (class static fields, interned strings, etc.)
    pub fn register_global(&mut self, slot: *mut *mut GcHeader) {
        self.roots.register_global(slot);
    }

    pub fn unregister_global(&mut self, slot: *mut *mut GcHeader) {
        self.roots.unregister_global(slot);
    }
}

impl<'gc> Drop for Mutator<'gc> {
    fn drop(&mut self) {
        // Unregister from safepoint coordinator when thread exits.
        // Without this, `wait_for_all_threads()` would block forever
        // waiting for a thread that no longer exists.
        self.safepoint.unregister_thread();
    }
}
