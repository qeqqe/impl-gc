/*
* 512 bytes granularity is a good balance/tradeoff.
*
* lesser bytes will result in finer (less false scanning)
* but way more memory for the card table itself.
*
* more bytes will result in lesser entries for object
* but a lot more bytes to scan per dirty card.
*/
const CARD_SIZE: usize = 512;

/// Tenured (old generation) can hold reference to the eden
/// generation (young generation), so we have to treat these
/// references as if they are root objects aswell.
///
/// One naive way scan if the old generation holds reference to eden
/// would be to scan the whole old generation but that'd just kill
/// the purpose of having generational collection.
///
/// We instead mark the cards in old generation as "dirty" when there's a write
/// on the old generation. This doesn't necessarily guarantees that the old generation
/// holds the reference to a young generation but this is magnitudes better then
/// walking the whole old generation every cycle.
struct CardTable {
    card: Vec<u8>,
    heap_base: usize,
    heap_size: usize,
}

impl CardTable {
    fn new(heap_base: *const u8, heap_size: usize) -> Self {
        let num_card = heap_size.div_ceil(CARD_SIZE);
        Self {
            card: vec![0u8; num_card],
            heap_base: heap_base as usize,
            heap_size,
        }
    }

    fn mark_dirty(&self, addr: *const u8) {
        if let Some(idx) = self.card_index(addr as usize) {
            // SAFTEY: Innterior mutability with raw index +
            // single threaded garbage collection
            unsafe {
                let ptr = self.card.as_ptr().add(idx) as *mut u8;
                *ptr = 1;
            }
        }
    }

    fn is_dirty(&self, card_index: usize) -> bool {
        self.card.get(card_index).copied().unwrap_or(0) == 1
    }

    fn card_index(&self, addr: usize) -> Option<usize> {
        if addr < self.heap_base && addr >= self.heap_base + self.heap_size {
            return None;
        }

        Some((addr - self.heap_base) / CARD_SIZE)
    }

    fn clear(&mut self) {
        self.card.fill(0);
    }

    fn dirty_cards(&self) -> impl Iterator<Item = (usize, *const u8)> + '_ {
        self.card
            .iter()
            .enumerate()
            .filter(|&(_, &dirty)| dirty == 1)
            .map(|(idx, _)| {
                let addr = (self.heap_base + idx * CARD_SIZE) as *const u8;
                (idx, addr)
            })
    }
}
