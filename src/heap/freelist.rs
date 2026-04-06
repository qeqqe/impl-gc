use std::ptr::NonNull;

use crate::heap::region::Region;

#[derive(Default)]
#[repr(C)]
struct FreeBlock {
    base: usize,
    size: usize,
    next: Option<NonNull<FreeBlock>>,
}

pub struct FreeListAllocator {
    region_base: usize,
    region_size: usize,
    free_list: Option<NonNull<FreeBlock>>,
}

impl FreeListAllocator {
    pub fn new(region_base: usize, region_size: usize) -> Self {
        Self {
            region_base,
            region_size,
            free_list: None,
        }
    }

    pub fn from_region(region: &Region) -> Self {
        Self {
            region_base: region.base() as usize,
            region_size: region.size(),
            free_list: None,
        }
    }
    pub fn alloc(&mut self, size: usize, align: usize) -> Option<*mut u8> {
        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut curr: Option<NonNull<FreeBlock>> = self.free_list;

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
                    None => self.free_list = next,
                }

                let min_block_size = std::mem::size_of::<FreeBlock>();
                let leftover = block.size - total_needed;

                if leftover >= min_block_size {
                    let new_block_addr = (aligned_addr + size) as *mut FreeBlock;
                    unsafe {
                        // before: new_block (FreeBlock) -> curr (FreeBlock)
                        new_block_addr.write(FreeBlock {
                            base: new_block_addr as usize,
                            size: leftover,
                            next: self.free_list,
                        });

                        // prepending to the start
                        // after: cur (new_block) (FreeBlock) -> next (cur) (FreeBlock)
                        self.free_list = NonNull::new(new_block_addr);
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
         * Traverse the FreeBlock and find the appropriate place for the FreeBlock to be
         * which is important for `coalesce` to work.
         */

        let new_addr = ptr as usize;
        let new_ptr = ptr as *mut FreeBlock;
        unsafe {
            new_ptr.write(FreeBlock {
                base: new_addr,
                size,
                next: None,
            });
        }

        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut curr = self.free_list;

        while let Some(block_nn) = curr {
            if block_nn.as_ptr() as usize > new_addr {
                break;
            }
            prev = curr;
            curr = unsafe { block_nn.as_ref().next };
        }

        unsafe {
            (*new_ptr).next = curr;
            match prev {
                None => self.free_list = NonNull::new(new_ptr),
                Some(mut p) => p.as_mut().next = NonNull::new(new_ptr),
            }
        }
    }

    pub fn coalesce(&mut self) {
        /*
         * 1. Right before the GC marks references in heap
         * it'll looks like this:
         * [ALIVE] -> [DEAD] -> [DEAD] -> [ALIVE] -> [DEAD]
         *
         * 2. After sweep it'll look like this
         * [ALIVE] -> [FREE] -> [FREE] -> [ALIVE] -> [FREE]
         *
         * The two seperate but contiguous [FREE]'s can add
         * complications while we are seeking for satisfying
         * space for allocation.
         *
         * 3. We will `coalesec` the contiguous spcaes and compact
         * them together
         *
         * This will what it'll end up looking like:
         * [ALIVE] -> [FREE x2] -> [ALIVE] -> [FREE]
         *
         */
        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut curr = self.free_list;

        while let Some(block_nn) = curr {
            // read everything up front before any writes
            let block_addr = block_nn.as_ptr() as usize;
            let block_size = unsafe { block_nn.as_ref().size };
            let block_next = unsafe { block_nn.as_ref().next };

            if let Some(prev_nn) = prev {
                // before: [ALIVE] -> [FREE (prev)] -> [FREE (cur)] -> [FREE] -> [ALIVE]
                // after: [ALIVE] -> [FREE 2x (prev)] -> [FREE (cur)] -> [ALIVE]
                // only move the curr (could be more then one FREE, so keep the prev where
                // it is)

                let prev_ptr = prev_nn.as_ptr();
                let prev_size = unsafe { (*prev_ptr).size };

                if prev_nn.as_ptr() as usize + prev_size == block_addr {
                    // merge, write expanded block in-place at prev, skip curr
                    unsafe {
                        prev_ptr.write(FreeBlock {
                            base: prev_ptr as usize,
                            size: prev_size + block_size,
                            next: block_next,
                        });
                    }
                    curr = block_next;
                    continue;
                }
            }

            prev = curr;
            curr = block_next;
        }
    }
}
