use std::sync::atomic::AtomicU8;

#[repr(C)]
pub struct GcHeader {
    // 0 = white (dead, default),
    // 1 = gray(seen, but children not yet processed),
    // 2 = black (fully processed)
    pub mark: AtomicU8,
    pub age: u8,
    pub flag: u8,
    pub _pad: u8,
    pub type_desc: *const u8, // TODO: make a type descriptor for this
    pub size: u32,
}

impl GcHeader {
    pub fn object_start(&self) -> *mut u8 {}
}
