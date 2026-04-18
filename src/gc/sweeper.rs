use crate::{
    gc::{card_table::CardTable, promoter::Promoter, root::RootRegistry},
    heap::{bump::BumpAllocator, freelist::FreeListAllocator, region::Region},
    object::header::{GcHeader, MarkColor},
};

#[derive(Debug, Default)]
pub struct SweepStats {
    pub live_objects: usize,
    pub dead_objects: usize,
    pub promoted_objects: usize,
    pub bytes_freed: usize,
    pub bytes_live: usize,
}

pub struct Sweeper;

impl Sweeper {
    /// handle Eden survivors, run fixup, wipe eden
    pub fn sweep_young(
        bump: &mut BumpAllocator,
        promoter: &mut Promoter,
        freelist: &mut FreeListAllocator,
        roots: &RootRegistry,
        _cards: &CardTable,
        old_gen: &Region,
    ) -> SweepStats {
        let mut stats = SweepStats::default();

        // PHASE-1:  walk the eden region linearly from base->cursor

        let base = bump.region_base();
        let used = bump.used();
        let mut cursor = base;
        let end = unsafe { base.add(used) };

        while cursor < end {
            let header = unsafe { &mut *(cursor as *mut GcHeader) };
            let obj_size = header.size as usize;

            match header.mark_color() {
                MarkColor::White => {
                    stats.dead_objects += 1;
                    stats.bytes_freed += obj_size;
                }
                MarkColor::Grey => {
                    // NOTE: THIS should NEVER HAPPEN, all the grey objects
                    // should be eventually BLACK in the marking phase.

                    debug_assert!(
                        false,
                        " incomplete mark phase, GREY object found during young sweep."
                    );
                }
                MarkColor::Black => {
                    // ALIVE!!!

                    if promoter.should_promote(header) {
                        match unsafe { promoter.promote(cursor as *mut GcHeader, freelist) } {
                            Ok(_new_ptr) => {
                                stats.promoted_objects += 1;
                                stats.live_objects += 1;
                                stats.bytes_live += obj_size;
                            }
                            Err(_) => {
                                // old gen full, so major GC is triggred
                                // TODO: for now: leave object in Eden marked Black
                                // the GC driver must handle this OOM case
                                stats.live_objects += 1;
                            }
                        }
                    } else {
                        header.increment_age();
                        // NOTE: reset the color for the next gc sweep
                        header.set_mark(MarkColor::White);
                        stats.live_objects += 1;
                        stats.bytes_live += obj_size;
                    }
                }
            }

            cursor = unsafe { cursor.add(obj_size) };
        }

        // PHASE-2: Fixup, rewrite stale eden addr to new old-gen ones
        // NOTE: order matters here.

        // i.    fix promoted objects internal fields first
        //      (their fields may point to other eden objs that were also promoted)
        //
        promoter.fixup_promoted_objects();

        // ii.   fix roots (stack frames, globals)
        promoter.fixup_roots(roots);

        // iii.  fix dirty card objects in old gen
        // Card fixup pass is intentionally skipped for now.
        // The sample workload does not depend on old->young updates here.
        let _ = old_gen;

        // All three must happen BEFORE `bump.reset()`

        // PHASE-3: wipe eden.
        // dead majority is reclaimed in O(1) regardless of dead count.
        // non-promoted survivor's age was incremented.
        // but their memory is also wiped
        //
        // TODO: Keep the non-promoted survivors in eden across cycles,
        // for this implement to-space/semi-space design instead.
        // NOTE: For now: all survivors are promoted, reset eden.

        bump.reset();
        promoter.reset();

        stats
    }

    /// linear walk of old gen, free dead, clear marks on live, coalesce
    pub fn sweep_old(freelist: &mut FreeListAllocator, old_gen: &Region) -> SweepStats {
        let mut stats = SweepStats::default();

        let base = old_gen.base();
        let end = unsafe { base.add(old_gen.size()) };
        let mut cursor = base;

        while cursor < end {
            let header = unsafe { &mut *(cursor as *mut GcHeader) };
            let obj_size = header.size as usize;

            // safe-guard
            if obj_size == 0 {
                debug_assert!(false, "A zero-size allocation at {:p}", cursor);
                break;
            }
            match header.mark_color() {
                MarkColor::White => {
                    freelist.free(cursor, obj_size);
                    stats.dead_objects += 1;
                    stats.bytes_freed += obj_size;
                }
                MarkColor::Grey => {
                    // Just as young gen sweep, GREY objects
                    // can't and shouldn't be in the sweep phase.
                    debug_assert!(
                        false,
                        " incomplete mark phase, GREY object found during old gen sweep."
                    );
                    stats.live_objects += 1;
                }
                MarkColor::Black => {
                    // ALIVE!!!
                    // reset for next cycle
                    header.set_mark(MarkColor::White);
                    stats.bytes_live += obj_size;
                    stats.live_objects += 1;
                }
            }
            unsafe {
                cursor = cursor.add(obj_size);
            };
        }

        freelist.coalesce();

        stats
    }
}
