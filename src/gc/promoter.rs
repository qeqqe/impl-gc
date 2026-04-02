use std::collections::HashMap;

use crate::{heap::freelist::FreeListAllocator, object::header::GcHeader};

/// Copies surviving young generation to old generation when they have survived enough
struct Promoter<'a> {
    threshold: u8,
    old_gen: &'a mut FreeListAllocator<'a>,
    // maps eden address with new old-gen address
    forawding: HashMap<usize, usize>,
}

impl<'a> Promoter<'a> {
    fn should_promote(&self, header: &GcHeader) -> bool {
        header.age >= self.threshold
    }

    fn promote(&mut self, eden_ptr: *mut GcHeader) -> Result<*mut GcHeader, ()> {
        let total_size = unsafe { (*eden_ptr).size as usize };
        let new_raw = self
            .old_gen
            .alloc(total_size, std::mem::align_of::<GcHeader>())
            .ok_or(())?;

        let new_ptr = new_raw as *mut GcHeader;

        unsafe {
            // copying the whole object as verbatim
            std::ptr::copy_nonoverlapping(eden_ptr as *const u8, new_ptr as *mut u8, total_size);

            // bumping the header's age
            (*new_ptr).age = (*new_ptr).age.saturating_add(1);
        }

        // All the old reference pointing to eden_ptr now should be pointing at new_ptr
        self.forawding.insert(eden_ptr as usize, new_ptr as usize);
        Ok(new_ptr)
    }

    // for checking if the forwarding table has the new address in the freelist
    // then updating it in place.
    fn fixup_ptr(&self, slot: *mut *mut GcHeader) {
        unsafe {
            let current = slot as usize;

            if let Some(&ptr) = self.forawding.get(&current) {
                *slot = ptr as *mut GcHeader;
            }
        }
    }
}
