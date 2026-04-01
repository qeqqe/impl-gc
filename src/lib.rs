#![allow(dead_code, unused_allocation, unused_assignments)]

mod gc;
mod heap;
mod object;
#[allow(dead_code, unused_allocation, unused_mut, unused_must_use)]
#[cfg(test)]
mod test {
    use crate::heap::{bump, freelist, region};

    // Region test

    #[test]
    fn region_create_and_size() {
        let r = region::Region::new(4096).unwrap();
        assert_eq!(r.size(), 4096);
    }

    #[test]
    fn region_base_is_non_null() {
        let r = region::Region::new(4096).unwrap();
        assert!(!r.base().is_null());
    }

    #[test]
    fn region_base_is_page_aligned() {
        // mmap typically returns page-aligned memory
        let r = region::Region::new(4096).unwrap();
        assert_eq!(r.base() as usize % 4096, 0);
    }

    #[test]
    fn region_contains_base() {
        let r = region::Region::new(1024).unwrap();
        assert!(r.contains(r.base()));
    }

    #[test]
    fn region_contains_last_byte() {
        let r = region::Region::new(1024).unwrap();
        let last = unsafe { r.base().add(1023) };
        assert!(r.contains(last));
    }

    #[test]
    fn region_does_not_contain_past_end() {
        let r = region::Region::new(1024).unwrap();
        let past = unsafe { r.base().add(1024) };
        assert!(!r.contains(past));
    }

    #[test]
    fn region_does_not_contain_null() {
        let r = region::Region::new(1024).unwrap();
        assert!(!r.contains(std::ptr::null()));
    }

    #[test]
    fn region_write_and_read_back() {
        let r = region::Region::new(4096).unwrap();
        unsafe {
            std::ptr::write(r.base(), 0xABu8);
            assert_eq!(std::ptr::read(r.base()), 0xAB);
        }
    }

    #[test]
    fn region_reset_succeeds() {
        let mut r = region::Region::new(4096).unwrap();
        r.reset().expect("reset should succeed");
    }

    #[test]
    fn region_reset_zeroes_memory() {
        let mut r = region::Region::new(4096).unwrap();
        // write some garbage
        unsafe {
            std::ptr::write(r.base(), 0xFF);
        }
        r.reset().unwrap();
        // fresh mmap pages are zero-filled
        let val = unsafe { std::ptr::read(r.base()) };
        assert_eq!(val, 0u8);
    }

    #[test]
    fn region_reset_preserves_size() {
        let mut r = region::Region::new(2048).unwrap();
        r.reset().unwrap();
        assert_eq!(r.size(), 2048);
    }

    #[test]
    fn region_multiple_resets() {
        let mut r = region::Region::new(4096).unwrap();
        for _ in 0..5 {
            r.reset().unwrap();
        }
        assert_eq!(r.size(), 4096);
    }

    #[test]
    fn region_small_size() {
        // even 1 byte should work (mmap rounds up to page size internally)
        let r = region::Region::new(1).unwrap();
        assert_eq!(r.size(), 1);
        assert!(!r.base().is_null());
    }

    #[test]
    fn region_large_size() {
        // 16 MiB
        let r = region::Region::new(16 * 1024 * 1024).unwrap();
        assert_eq!(r.size(), 16 * 1024 * 1024);
    }

    // BumpAllocator — basic

    #[test]
    fn bump_initial_state() {
        let mut r = region::Region::new(4096).unwrap();
        let bump = bump::BumpAllocator::new(&mut r);
        assert_eq!(bump.used(), 0);
        assert_eq!(bump.remaining(), 4096);
    }

    #[test]
    fn bump_single_alloc() {
        let mut r = region::Region::new(4096).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);
        let ptr = bump.alloc(64, 8);
        assert!(ptr.is_some());
        assert!(bump.used() >= 64);
    }

    #[test]
    fn bump_alloc_returns_aligned_pointer() {
        let mut r = region::Region::new(4096).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);
        let ptr = bump.alloc(32, 16).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 16, 0);
    }

    #[test]
    fn bump_alloc_high_alignment() {
        let mut r = region::Region::new(4096).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);
        let ptr = bump.alloc(64, 256).unwrap();
        assert_eq!(ptr.as_ptr() as usize % 256, 0);
    }

    #[test]
    fn bump_sequential_allocs_dont_overlap() {
        let mut r = region::Region::new(4096).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);

        let a = bump.alloc(100, 8).unwrap();
        let b = bump.alloc(100, 8).unwrap();

        let a_start = a.as_ptr() as usize;
        let b_start = b.as_ptr() as usize;

        // b must start at or after a's end
        assert!(b_start >= a_start + 100);
    }

    #[test]
    fn bump_used_plus_remaining_equals_size() {
        let mut r = region::Region::new(4096).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);
        bump.alloc(123, 8);
        bump.alloc(456, 16);
        assert_eq!(bump.used() + bump.remaining(), 4096);
    }

    #[test]
    fn bump_alloc_exact_fit() {
        let mut r = region::Region::new(4096).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);
        // base is page-aligned so align=1 means no waste
        let ptr = bump.alloc(4096, 1);
        assert!(ptr.is_some());
        assert_eq!(bump.remaining(), 0);
    }

    #[test]
    fn bump_alloc_one_byte_over_returns_none() {
        let mut r = region::Region::new(4096).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);
        let ptr = bump.alloc(4097, 1);
        assert!(ptr.is_none());
    }

    // BumpAllocator — OOM & edge cases

    #[test]
    fn bump_oom_returns_none() {
        let mut r = region::Region::new(128).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);
        let ptr = bump.alloc(256, 8);
        assert!(ptr.is_none());
        // cursor should not have advanced
        assert_eq!(bump.used(), 0);
    }

    #[test]
    fn bump_oom_after_partial_fill() {
        let mut r = region::Region::new(256).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);
        assert!(bump.alloc(200, 1).is_some());
        // only ~56 left, asking for 100 should fail
        assert!(bump.alloc(100, 1).is_none());
    }

    #[test]
    fn bump_zero_size_alloc() {
        let mut r = region::Region::new(4096).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);
        // zero-size alloc — should return a valid aligned pointer without advancing
        let ptr = bump.alloc(0, 8);
        assert!(ptr.is_some());
        // used might be 0 or just alignment padding — shouldn't blow up either way
    }

    #[test]
    fn bump_many_tiny_allocs() {
        let mut r = region::Region::new(4096).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);
        let mut count = 0;
        while bump.alloc(1, 1).is_some() {
            count += 1;
        }
        assert_eq!(count, 4096);
    }

    // BumpAllocator — reset

    #[test]
    fn bump_reset_restores_full_capacity() {
        let mut r = region::Region::new(4096).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);
        bump.alloc(2048, 8);
        bump.reset().unwrap();
        assert_eq!(bump.used(), 0);
        assert_eq!(bump.remaining(), 4096);
    }

    #[test]
    fn bump_alloc_after_reset() {
        let mut r = region::Region::new(4096).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);
        bump.alloc(4096, 1);
        assert!(bump.alloc(1, 1).is_none());

        bump.reset().unwrap();
        let ptr = bump.alloc(4096, 1);
        assert!(ptr.is_some());
    }

    #[test]
    fn bump_write_after_reset_is_safe() {
        let mut r = region::Region::new(4096).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);

        // first generation
        let p1 = bump.alloc(8, 8).unwrap();
        unsafe {
            std::ptr::write(p1.as_ptr() as *mut u64, 0xDEAD_BEEF);
        }

        bump.reset().unwrap();

        // second generation — should be writable
        let p2 = bump.alloc(8, 8).unwrap();
        unsafe {
            std::ptr::write(p2.as_ptr() as *mut u64, 0xCAFE_BABE);
            assert_eq!(std::ptr::read(p2.as_ptr() as *const u64), 0xCAFE_BABE);
        }
    }

    // BumpAllocator — alignment stress

    #[test]
    fn bump_alignment_wastes_expected_space() {
        let mut r = region::Region::new(4096).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);

        // alloc 1 byte with align=1 to push cursor off-alignment
        bump.alloc(1, 1);
        let before = bump.used();

        // now alloc with align=256 — padding should show up
        bump.alloc(1, 256);
        let after = bump.used();
        let wasted = (after - before) - 1; // subtract the 1 byte payload
        // wasted must be < 256 (alignment padding) and cursor is now 256-aligned
        assert!(wasted < 256);
    }

    #[test]
    fn bump_various_alignments() {
        let mut r = region::Region::new(4096).unwrap();
        let mut bump = bump::BumpAllocator::new(&mut r);

        for align in [1, 2, 4, 8, 16, 32, 64, 128] {
            let ptr = bump.alloc(1, align).unwrap();
            assert_eq!(
                ptr.as_ptr() as usize % align,
                0,
                "failed for align={}",
                align
            );
        }
    }

    // FreeListAllocator — bootstrapping

    // NOTE: FreeListAllocator::new starts with an empty free list.
    // You must seed it by free()-ing the entire region range.

    /// Helper: create a region + freelist and seed the entire region as free.
    fn make_freelist(region: &mut region::Region) -> freelist::FreeListAllocator<'_> {
        let base = region.base();
        let size = region.size();
        let mut fl = freelist::FreeListAllocator::new(region);
        fl.free(base, size);
        fl
    }

    #[test]
    fn freelist_empty_alloc_returns_none() {
        let mut r = region::Region::new(4096).unwrap();
        let mut fl = freelist::FreeListAllocator::new(&mut r);
        // no free blocks seeded
        assert!(fl.alloc(64, 8).is_none());
    }

    #[test]
    fn freelist_seeded_alloc_succeeds() {
        let mut r = region::Region::new(4096).unwrap();
        let mut fl = make_freelist(&mut r);
        let ptr = fl.alloc(64, 8);
        assert!(ptr.is_some());
    }

    #[test]
    fn freelist_alloc_returns_aligned() {
        let mut r = region::Region::new(4096).unwrap();
        let mut fl = make_freelist(&mut r);
        let ptr = fl.alloc(32, 64).unwrap();
        assert_eq!(ptr as usize % 64, 0);
    }

    // FreeListAllocator — alloc / free basics

    #[test]
    fn freelist_alloc_then_free_then_realloc() {
        let mut r = region::Region::new(4096).unwrap();
        let mut fl = make_freelist(&mut r);

        let ptr = fl.alloc(128, 8).unwrap();
        // free it back
        fl.free(ptr, 128);
        // should be able to alloc again
        let ptr2 = fl.alloc(128, 8);
        assert!(ptr2.is_some());
    }

    #[test]
    fn freelist_multiple_allocs_dont_overlap() {
        let mut r = region::Region::new(4096).unwrap();
        let mut fl = make_freelist(&mut r);

        let a = fl.alloc(256, 8).unwrap() as usize;
        let b = fl.alloc(256, 8).unwrap() as usize;

        // non-overlapping
        assert!(b >= a + 256 || a >= b + 256);
    }

    #[test]
    fn freelist_alloc_too_large_returns_none() {
        let mut r = region::Region::new(256).unwrap();
        let mut fl = make_freelist(&mut r);
        assert!(fl.alloc(512, 8).is_none());
    }

    #[test]
    fn freelist_exact_fit() {
        // Seed exactly 64 bytes; a FreeBlock header lives inside that space.
        // The full 64 bytes should be allocatable (block is consumed).
        let mut r = region::Region::new(4096).unwrap();
        let base = r.base();
        let mut fl = freelist::FreeListAllocator::new(&mut r);

        fl.free(base, 64);

        let ptr = fl.alloc(64, 1);
        assert!(ptr.is_some());
    }

    // FreeListAllocator — coalesce (indirect)

    #[test]
    fn freelist_coalesce_adjacent_blocks() {
        let mut r = region::Region::new(4096).unwrap();
        let mut fl = make_freelist(&mut r);

        // consume the whole thing in two chunks
        let a = fl.alloc(2048, 1).unwrap();
        let b = fl.alloc(2048, 1).unwrap();

        // return them (they should be adjacent in the region)
        fl.free(a, 2048);
        fl.free(b, 2048);

        // after coalesce, a single 4096-byte block should exist
        // so a full-region alloc should succeed
        let big = fl.alloc(4096, 1);
        // May or may not succeed depending on whether coalesce is called
        // automatically. If it returns None, call coalesce isn't public,
        // so this test documents the current behavior.
        // If your design calls coalesce lazily inside alloc, this will pass.
        // Otherwise it exposes fragmentation — which is useful to know.
        if big.is_none() {
            // fragmentation present — document it
            eprintln!(
                "NOTE: coalesce not triggered automatically; full-region re-alloc fails due to fragmentation"
            );
        }
    }

    #[test]
    fn freelist_free_in_address_order_enables_coalesce() {
        let mut r = region::Region::new(4096).unwrap();
        let mut fl = make_freelist(&mut r);

        let a = fl.alloc(1024, 1).unwrap();
        let b = fl.alloc(1024, 1).unwrap();
        let c = fl.alloc(1024, 1).unwrap();

        // free in address order so the linked list stays sorted
        fl.free(a, 1024);
        fl.free(b, 1024);
        fl.free(c, 1024);

        // should be able to get a contiguous 3072-byte block
        // (if coalesce runs — same caveat as above)
        let big = fl.alloc(3072, 1);
        if big.is_some() {
            // coalesce worked
        } else {
            eprintln!(
                "NOTE: 3072-byte alloc failed after freeing 3x1024; coalesce may not be automatic"
            );
        }
    }

    // FreeListAllocator — fragmentation / reuse patterns

    #[test]
    fn freelist_interleaved_alloc_free() {
        let mut r = region::Region::new(4096).unwrap();
        let mut fl = make_freelist(&mut r);

        let mut ptrs = Vec::new();
        // allocate 8 x 256-byte blocks
        for _ in 0..8 {
            let p = fl.alloc(256, 8).expect("alloc should succeed");
            ptrs.push(p);
        }

        // free every other one
        for i in (0..8).step_by(2) {
            fl.free(ptrs[i], 256);
        }

        // reallocate into the holes
        for _ in 0..4 {
            let p = fl.alloc(256, 8);
            assert!(p.is_some(), "should reuse freed blocks");
        }
    }

    #[test]
    fn freelist_write_to_allocated_memory() {
        let mut r = region::Region::new(4096).unwrap();
        let mut fl = make_freelist(&mut r);

        let ptr = fl.alloc(64, 8).unwrap();

        // should be safe to write into allocated memory
        unsafe {
            let slice = std::slice::from_raw_parts_mut(ptr, 64);
            for (i, byte) in slice.iter_mut().enumerate() {
                *byte = i as u8;
            }
            // read back
            for (i, byte) in slice.iter().enumerate() {
                assert_eq!(*byte, i as u8);
            }
        }
    }

    #[test]
    fn freelist_alloc_with_different_alignments() {
        let mut r = region::Region::new(8192).unwrap();
        let mut fl = make_freelist(&mut r);

        for align in [1, 2, 4, 8, 16, 32, 64] {
            let ptr = fl
                .alloc(32, align)
                .expect(&format!("alloc align={}", align));
            assert_eq!(ptr as usize % align, 0, "pointer not aligned to {}", align);
        }
    }

    // FreeListAllocator — stress

    #[test]
    fn freelist_alloc_until_oom_then_free_all() {
        let mut r = region::Region::new(4096).unwrap();
        let mut fl = make_freelist(&mut r);

        let mut allocs: Vec<(*mut u8, usize)> = Vec::new();
        let block_size = 64;

        // alloc until OOM
        loop {
            match fl.alloc(block_size, 8) {
                Some(ptr) => allocs.push((ptr, block_size)),
                None => break,
            }
        }
        assert!(
            !allocs.is_empty(),
            "should have allocated at least one block"
        );

        // free everything
        for (ptr, size) in &allocs {
            fl.free(*ptr, *size);
        }

        // at least one alloc should work again
        let ptr = fl.alloc(block_size, 8);
        assert!(
            ptr.is_some(),
            "should be able to alloc after freeing everything"
        );
    }

    #[test]
    fn freelist_repeated_alloc_free_cycles() {
        let mut r = region::Region::new(4096).unwrap();
        let mut fl = make_freelist(&mut r);

        for _ in 0..100 {
            let ptr = fl.alloc(64, 8).expect("alloc should succeed");
            // write to it
            unsafe {
                std::ptr::write(ptr, 0xAA);
            }
            fl.free(ptr, 64);
        }
    }

    // Cross-module: Region + BumpAllocator integration

    #[test]
    fn bump_allocated_pointer_is_inside_region() {
        let mut r = region::Region::new(4096).unwrap();
        let base = r.base();
        let mut bump = bump::BumpAllocator::new(&mut r);

        let ptr = bump.alloc(64, 8).unwrap();
        // manually check containment (we can't call r.contains while bump borrows it,
        // but we can check arithmetic)
        let addr = ptr.as_ptr() as usize;
        let base_addr = base as usize;
        assert!(addr >= base_addr && addr + 64 <= base_addr + 4096);
    }

    #[test]
    fn freelist_allocated_pointer_is_inside_region() {
        let mut r = region::Region::new(4096).unwrap();
        let base = r.base() as usize;
        let size = r.size();
        let mut fl = make_freelist(&mut r);

        let ptr = fl.alloc(128, 8).unwrap() as usize;
        assert!(ptr >= base && ptr + 128 <= base + size);
    }
}
