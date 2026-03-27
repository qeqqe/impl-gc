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
        let mut curr: Option<NonNull<FreeBlock>> = self.freeList;

        while let Some(mut block_nn) = curr {
            let block = unsafe { block_nn.as_mut() };

            let block_addr = block_nn.as_ptr() as usize;

            /*
             * align with 2^align
             * Example: (block_addr = 11, align = 4)
             * (1011 + 0100) & !(0011)
             *  1110 & 1100
             *  = 1100
             */
            let aligned_addr = (block_addr + align - 1) & !(align - 1);
            let alignment_wasted = aligned_addr - block_addr;
            let total_needed = alignment_wasted + size;

            if block.size >= total_needed {
                let next = block.next;

                match prev {
                    // before: prev(FreeBlock) -> curr (FreeBlock) (sufficient for `size`) -> next (FreeBlock)
                    // after: prev (FreeBlock) -> next (FreeBlock)
                    Some(mut p) => unsafe { p.as_mut().next = next },

                    // before: curr (FreeBlock) (sufficient for `size`) -> next (FreeBlock)
                    // after: next (curr) (FreeBlock)
                    None => self.freeList = next,
                }

                let min_block_size = std::mem::size_of::<FreeBlock>();
                let leftover = total_needed - block.size;

                if leftover <= min_block_size {
                    let new_block_addr = (aligned_addr + size) as *mut FreeBlock;
                    unsafe {
                        // before: new_block (FreeBlock) -> curr (FreeBlock)
                        new_block_addr.write(FreeBlock {
                            base: new_block_addr as usize,
                            size: leftover,
                            next: self.freeList,
                        });
                        // after: cur (new_block) (FreeBlock) -> next (cur) (FreeBlock)
                        self.freeList = NonNull::new(new_block_addr);
                    }
                }
                return Some(aligned_addr as *mut u8);
            }

            prev = curr;
            curr = block.next;
        }

        None
    }
    pub fn free(&mut self, ptr: *mut u8, size: usize) {
        /*
         * intuition for now:
         * just write at the pointer with a `FreeBlock` and put it in the self.freeList
         * new_freeblock.next => self.freeList
         * self.freeList.next => new_freeblock
         */

        let new_free_block = ptr as *mut FreeBlock;

        unsafe {
            new_free_block.write(FreeBlock {
                base: new_free_block as usize,
                size,
                next: self.freeList,
            });
        }
        self.freeList = NonNull::new(new_free_block);
    }

    fn coalesec(&mut self) {
        todo!()
    }
}
