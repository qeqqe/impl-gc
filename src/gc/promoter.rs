use std::{collections::HashMap, ops::Add};

use crate::{
    gc::{card_table::CardTable, root::RootRegistry},
    heap::{freelist::FreeListAllocator, region::Region},
    object::header::GcHeader,
};

/// Copies surviving young generation to old generation when they have survived enough
pub struct Promoter {
    pub threshold: u8,
    forwarding: HashMap<usize, usize>,
}

impl Promoter {
    pub fn new(threshold: u8) -> Self {
        Self {
            threshold,
            forwarding: HashMap::new(),
        }
    }

    pub fn should_promote(&self, header: &GcHeader) -> bool {
        header.age >= self.threshold
    }

    /// for all the BLACK eden objects
    pub fn promote(
        &mut self,
        eden_ptr: *mut GcHeader,
        old_alloc: &mut FreeListAllocator,
    ) -> Result<*mut GcHeader, ()> {
        let total_size = unsafe { (*eden_ptr).size as usize };
        let new_raw = old_alloc
            .alloc(total_size, std::mem::align_of::<GcHeader>())
            .ok_or(())?;
        let new_ptr = new_raw as *mut GcHeader;
        unsafe {
            // copying the whole object as verbatim
            std::ptr::copy_nonoverlapping(eden_ptr as *const u8, new_ptr as *mut u8, total_size);

            // bumping the header's age
            (*new_ptr).age = (*new_ptr).age.saturating_add(1);
        }
        self.forwarding.insert(eden_ptr as usize, new_ptr as usize);
        Ok(new_ptr)
    }

    /// promoted objects were copied verbatim,
    /// so their pointer fields still point to eden
    /// addresses of OTHER objects that may have also been promoted
    pub fn fixup_promoted_objects(&self) {
        for (&_old_addr, &new_addr) in &self.forwarding {
            let new_header = new_addr as *mut GcHeader;
            let type_desc = unsafe { &*(*new_header).type_desc };

            type_desc.trace_slots(unsafe { (*new_header).object_start() }, |slot| {
                self.fixup_ptr(slot);
            });
        }
    }

    /// called after all Eden survivors are promoted
    pub fn fixup_roots(&self, roots: &RootRegistry) {
        for slot in roots.iter_roots() {
            // iter_root() yields *mut *mut GcHeader, the slot addresses
            self.fixup_ptr(slot as *mut *mut GcHeader);
        }
    }

    /// fix dirty cards references, updating eden <- old_gen reference to point to the
    /// promoted eden ptr
    pub fn fixup_dirty_cards(&self, cards: &CardTable, old_gen: &Region) {
        const CARD_SIZE: usize = 512;

        for (_, root) in cards.dirty_cards() {
            let card_base = root as usize;
            let card_end = card_base.add(CARD_SIZE);

            let mut cursor = card_base as *mut u8;

            while cursor < card_end as *mut u8 && old_gen.contains(root) {
                let header = unsafe { &*GcHeader::from_object_ptr(cursor) };
                let type_desc = unsafe { &*header.type_desc };

                type_desc.trace(cursor, |child_header_ptr| {
                    // trace gives us *mut GcHeader, we need the slot address
                    // this requires trace to yield `*mut *mut GcHeader`
                });

                unsafe {
                    cursor.add(header.size as usize);
                }
            }
        }
    }

    /// for checking if the forwarding table has the new address in the freelist
    /// then updating it in place.
    pub fn fixup_ptr(&self, slot: *mut *mut GcHeader) {
        unsafe {
            let current = slot as usize;

            if let Some(&ptr) = self.forwarding.get(&current) {
                *slot = ptr as *mut GcHeader;
            }
        }
    }

    fn forwarded_addr(&self, ptr: *mut GcHeader) -> Option<*mut GcHeader> {
        self.forwarding
            .get(&(ptr as usize))
            .map(|&addr| addr as *mut GcHeader)
    }

    fn was_promoted(&self, addr: *mut GcHeader) -> bool {
        self.forwarding.contains_key(&(addr as usize))
    }

    /// resets eden mapping...
    /// NOTE: run `fixup_promoted_objects` before `bump.reset()`,
    /// because fixup reads the forwarding table which
    /// references old eden addresses, those addresses
    /// are still valid until `bump.reset()` fires...
    pub fn reset(&mut self) {
        self.forwarding.clear();
    }
}
