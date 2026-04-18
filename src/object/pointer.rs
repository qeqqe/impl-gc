use std::marker::PhantomData;

use crate::object::header::GcHeader;

/// This is a `GcHeader` wrapper, While carrying &T we have to manage
/// the lifetime, we don't have static lifetime for the GC objects
/// they're only alive till our collector allows it to.
///
/// We will use raw pointers, let alone they don't have any type information
/// due to no rust runtime reflection, `GcPtr<T>` is a `#[repr(transparent)]` wrapper
/// around `GcHeader`, this adds zero overhead and carries the type T through
/// `PhantomData`
///
/// With this we can write/get `&GcHeader` without casting everywhere.
#[repr(transparent)]
pub struct GcPtr<T> {
    ptr: *mut GcHeader,
    _marker: PhantomData<T>,
}
impl<T> Copy for GcPtr<T> {}
impl<T> Clone for GcPtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> GcPtr<T> {
    /// # Safety
    /// `ptr` must be a valid pointer to a GC-managed allocation header.
    pub unsafe fn from_raw(ptr: *mut GcHeader) -> Self {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    pub fn as_ptr(&self) -> *mut GcHeader {
        self.ptr
    }

    pub fn header(&self) -> &GcHeader {
        unsafe { self.ptr.as_ref().unwrap() }
    }

    /// # Safety
    /// The allocation behind this pointer must contain a properly initialized `T`
    /// value in its payload area and remain valid for the returned borrow.
    pub unsafe fn data(&self) -> &T {
        unsafe {
            self.ptr
                .cast::<u8>()
                .add(std::mem::size_of::<GcHeader>())
                .cast::<T>()
                .as_ref()
                .unwrap()
        }
    }

    /// # Safety
    /// The allocation behind this pointer must contain a properly initialized `T`
    /// payload and be uniquely borrowed for the duration of the mutable borrow.
    pub unsafe fn data_mut(&mut self) -> &mut T {
        unsafe {
            self.ptr
                .cast::<u8>()
                .add(std::mem::size_of::<GcHeader>())
                .cast::<T>()
                .as_mut()
                .unwrap()
        }
    }
    /// # Safety
    /// Caller must ensure reinterpreting this payload as `U` is valid.
    pub unsafe fn cast<U>(&self) -> GcPtr<U> {
        GcPtr {
            ptr: self.ptr,
            _marker: PhantomData,
        }
    }

    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }
}
