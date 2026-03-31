use std::marker::PhantomData;

use crate::object::header::GcHeader;

#[repr(transparent)]
pub struct GcPtr<T> {
    ptr: *mut GcHeader,
    _marker: PhantomData<T>,
}

impl<T> GcPtr<T> {
    pub fn from_raw(ptr: *mut GcHeader) -> Self {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    fn as_ptr(&self) -> *mut GcHeader {
        self.ptr
    }

    fn header(&self) -> &GcHeader {
        unsafe { self.ptr.as_ref().unwrap() }
    }

    pub fn data(&self) -> &T {
        unsafe {
            self.ptr
                .cast::<u8>()
                .add(std::mem::size_of::<GcHeader>())
                .cast::<T>()
                .as_ref()
                .unwrap()
        }
    }

    pub fn data_mut(&mut self) -> &mut T {
        unsafe {
            self.ptr
                .cast::<u8>()
                .add(std::mem::size_of::<GcHeader>())
                .cast::<T>()
                .as_mut()
                .unwrap()
        }
    }

    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }
}
