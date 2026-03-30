use std::sync::atomic::{AtomicU8, Ordering};

enum MarkColor {
    White = 0,
    Gray = 1,
    Black = 2,
}

#[repr(C)]
pub struct GcHeader {
    // 0 = white (dead, default),
    // 1 = gray(children not processed),
    // 2 = black (fully processed)
    pub mark: AtomicU8,
    pub age: u8,
    pub flag: u8,
    pub _pad: u8,
    pub type_desc: *const u8, // TODO: make a type descriptor for this
    pub size: u32,
}

impl GcHeader {
    pub fn object_start(&self) -> *mut u8 {
        unsafe { (self as *const Self).add(1) as *mut u8 }
    }

    pub fn is_marked(&self) -> bool {
        let mark = self.mark.load(Ordering::Relaxed);
        1 == mark || 0 == mark
    }

    pub fn mark(&self, color: MarkColor) {
        self.mark.store(color as u8, Ordering::Relaxed);
    }

    pub fn increment_age(&mut self) {
        self.age = self.age.saturating_add(1);
    }
}
