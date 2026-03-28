use crate::heap::region::{AllocError, Region};
use std::ptr::NonNull;

pub struct BumpAllocator<'a> {
    region: &'a mut Region,
    cursor: usize,
}

impl<'a> BumpAllocator<'a> {
    pub fn new(region: &'a mut Region) -> Self {
        let cursor = region.base() as usize;
        Self { region, cursor }
    }

    pub fn alloc(&mut self, size: usize, align: usize) -> Option<NonNull<u8>> {
        debug_assert!(align.is_power_of_two(), "align must be a power of 2");

        let aligned = (self.cursor + align - 1) & !(align - 1);
        let new_cursor = aligned + size;
        let end = self.region.base() as usize + self.region.size();

        if new_cursor > end {
            return None;
        }

        self.cursor = new_cursor;
        NonNull::new(aligned as *mut u8)
    }

    pub fn reset(&mut self) -> Result<(), AllocError> {
        self.region.reset()?;
        self.cursor = self.region.base() as usize;
        Ok(())
    }

    pub fn used(&self) -> usize {
        self.cursor - self.region.base() as usize
    }

    pub fn remaining(&self) -> usize {
        (self.region.base() as usize + self.region.size()) - self.cursor
    }
}
