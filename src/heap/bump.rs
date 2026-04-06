use crate::heap::region::Region;
use std::ptr::NonNull;

pub struct BumpAllocator {
    base: usize, // virtual addr of region start
    cursor: usize,
    limit: usize, // base + size
}

impl BumpAllocator {
    pub fn from_region(region: &Region) -> Self {
        let base = region.base() as usize;
        Self {
            base,
            cursor: base,
            limit: base + region.size(),
        }
    }

    pub fn alloc(&mut self, size: usize, align: usize) -> Option<NonNull<u8>> {
        debug_assert!(align.is_power_of_two());
        let aligned = (self.cursor + align - 1) & !(align - 1);
        let new_cursor = aligned + size;
        if new_cursor > self.limit {
            return None;
        }
        self.cursor = new_cursor;
        NonNull::new(aligned as *mut u8)
    }

    // reset without remapping, uses madvise to zero pages in place
    // base pointer stays stable. much cheaper than munmap + remap
    pub fn reset(&mut self) {
        unsafe {
            libc::madvise(
                self.base as *mut libc::c_void,
                self.limit - self.base,
                libc::MADV_DONTNEED,
            );
        }
        self.cursor = self.base;
    }

    pub fn used(&self) -> usize {
        self.cursor - self.base
    }
    pub fn remaining(&self) -> usize {
        self.limit - self.cursor
    }
    pub fn base_ptr(&self) -> *mut u8 {
        self.base as *mut u8
    }
    pub fn region_base(&self) -> *mut u8 {
        self.base as *mut u8
    }
}

