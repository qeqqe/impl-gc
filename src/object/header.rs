use std::sync::atomic::{AtomicU8, Ordering};

use crate::object::descriptor::TypeDescriptor;

#[derive(Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum MarkColor {
    White = 0, // not yet reached -> dead after mark phase
    Grey = 1,  // discovered, children not yet scanned
    Black = 2, // fully scanned
}

#[repr(C)]
pub struct GcHeader {
    pub mark: AtomicU8,
    pub age: u8,
    pub flag: u8,
    pub _pad: u8,
    pub type_desc: *const TypeDescriptor,
    pub size: u32,
}

impl GcHeader {
    pub fn object_start(&self) -> *mut u8 {
        unsafe { (self as *const GcHeader).add(1) as *mut u8 }
    }

    pub fn from_object_ptr(obj: *mut u8) -> *mut GcHeader {
        unsafe { (obj as *mut GcHeader).sub(1) }
    }

    pub fn is_marked(&self) -> bool {
        self.mark.load(Ordering::Relaxed) != MarkColor::White as u8
    }

    /// get mark
    pub fn mark_color(&self) -> MarkColor {
        match self.mark.load(Ordering::Relaxed) {
            1 => MarkColor::Grey,
            2 => MarkColor::Black,
            _ => MarkColor::White,
        }
    }

    pub fn set_mark(&self, color: MarkColor) {
        self.mark.store(color as u8, Ordering::Relaxed);
    }

    pub fn increment_age(&mut self) {
        self.age = self.age.saturating_add(1);
    }
}
