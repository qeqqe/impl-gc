use std::{collections::LinkedList, ptr::NonNull};

use crate::region::Region;

#[derive(Default)]
#[repr(C)]
struct FreeBlock {
    base: usize,
    size: usize,
    next: Option<NonNull<FreeBlock>>,
}

pub struct FreeListAllocator<'a> {
    region: &'a mut Region,
    freeList: Option<NonNull<FreeBlock>>,
}

impl<'a> FreeListAllocator<'a> {
    pub fn new(region: &'a mut Region) -> Self {
        let base = region.base();
        Self {
            region,
            freeList: None,
        }
    }

    pub fn alloc(&mut self, size: usize, align: usize) -> Option<*mut u8> {
        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut curr = self.freeList;
        while let Some(mut block_nn) = curr {
            let block = unsafe { block_nn.as_mut() };

            let block_addr = block_nn.as_ptr() as usize;

            // CHECK: how far do we need to push forward to satisfy alignment
            let aligned_addr = (block_addr + align - 1) & !(align - 1);
            let alignment_waste = aligned_addr - block_addr;
            let total_needed = alignment_waste + size;

            if block.size >= total_needed {
                // ── UNLINK this block from the list ──────────────────────
                let next = block.next;

                match prev {
                    None => self.freeList = next,                     // removing head
                    Some(mut p) => unsafe { p.as_mut().next = next }, // removing mid/tail
                }

                // ── SPLIT if there's enough leftover for a new FreeBlock ──
                let leftover = block.size - total_needed;
                let min_block = std::mem::size_of::<FreeBlock>();

                if leftover >= min_block {
                    // carve a new FreeBlock out of the tail of this block
                    let new_block_addr = (aligned_addr + size) as *mut FreeBlock;
                    unsafe {
                        new_block_addr.write(FreeBlock {
                            base: new_block_addr as usize,
                            size: leftover,
                            next: self.freeList, // prepend to free list
                        });
                        self.freeList = NonNull::new(new_block_addr);
                    }
                }

                return Some(aligned_addr as *mut u8);
            }

            // advance: prev = curr, curr = curr.next
            prev = curr;
            curr = block.next;
        }

        None
    }

    pub fn free(&mut self, ptr: *mut u8, size: usize) {
        todo!()
    }

    fn coalesec(&mut self) {
        todo!()
    }
}
