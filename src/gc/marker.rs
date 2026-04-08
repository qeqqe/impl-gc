use crate::{
    gc::{card_table::CardTable, root::RootRegistry},
    heap::region::Region,
    object::header::{GcHeader, MarkColor},
};

#[derive(Debug, Default)]
pub struct Marker {
    worklist: Vec<*mut GcHeader>,
}

/// Marker owns the worklist.
impl Marker {
    /// only trace objects reachable from roots that live in young gen
    /// Old -> young cross gen pointers are in dirty cards
    pub fn mark_minor(
        &mut self,
        roots: &RootRegistry,
        cards: &CardTable,
        old_gen: &Region,
        young_gen: &Region,
    ) {
        // roots only those pointing into young gen
        for root in roots.iter_roots() {
            unsafe {
                let header = &*root;
                let header_start = header.object_start();

                if young_gen.contains(header_start) && !header.is_marked() {
                    header.set_mark(MarkColor::Grey);
                    self.worklist.push(root);
                }
            }
        }

        // scan dirty cards for cross gen pointers
        for (_, card_base) in cards.dirty_cards() {
            self.scan_base(card_base, old_gen, young_gen);
        }
        self.drain(|child_obj| young_gen.contains(child_obj));
    }

    pub fn mark_major(&mut self, roots: &RootRegistry, heap: &Region) {
        for root in roots.iter_roots() {
            unsafe {
                let header = &*root;
                if !header.is_marked() {
                    header.set_mark(MarkColor::Grey);
                    self.worklist.push(root);
                }
            }
        }

        self.drain(|child_obj| heap.contains(child_obj));
    }

    /// pops from worklist, traces children, marks black
    fn drain<F>(&mut self, should_follow: F)
    where
        F: Fn(*const u8) -> bool,
    {
        while let Some(ptr) = self.worklist.pop() {
            unsafe {
                let header = &*ptr;

                if header.mark_color() == MarkColor::Black {
                    continue;
                }

                let type_desc = &*header.type_desc;

                type_desc.trace(header.object_start(), |child_header| {
                    let child = &*child_header;
                    let child_obj = child.object_start();

                    if should_follow(child_obj) && !child.is_marked() {
                        child.set_mark(MarkColor::Grey);
                        self.worklist.push(child_header);
                    }
                });

                // all children marked
                header.set_mark(MarkColor::Black);
            }
        }
    }

    /// walk every object in  dirty card and push valid ones in worklist
    fn scan_base(&mut self, card_base: *const u8, old_gen: &Region, young_gen: &Region) {
        const CARD_SIZE: usize = 512;
        let card_end = unsafe { card_base.add(CARD_SIZE) };
        let mut cursor = card_base as *mut u8;

        while cursor < card_end as *mut u8 && old_gen.contains(cursor) {
            unsafe {
                let header = &*GcHeader::from_object_ptr(cursor);
                let type_desc = &*header.type_desc;

                type_desc.trace(header.object_start(), |child_header| {
                    let child = &*child_header;
                    if young_gen.contains(child.object_start()) && !child.is_marked() {
                        child.set_mark(MarkColor::Grey);
                        self.worklist.push(child_header);
                    }
                });
                cursor = cursor.add(header.size as usize);
            }
        }
    }

    pub fn reset(&mut self) {
        self.worklist.clear();
    }
}
