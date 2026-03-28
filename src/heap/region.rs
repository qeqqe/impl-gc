use core::fmt;
use std::{error::Error, ptr::NonNull};

#[derive(Debug)]
pub struct AllocError {}

impl fmt::Display for AllocError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wasn't able to allocate")
    }
}

impl Error for AllocError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }

    fn cause(&self) -> Option<&dyn Error> {
        self.source()
    }
}

pub struct Region {
    base: NonNull<u8>,
    size: usize,
    commited: usize,
}

impl Drop for Region {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.base.as_ptr() as *mut libc::c_void, self.size);
        }
    }
}

// SAFTEY: Region has it's own exclusive ownership of it's memory range
unsafe impl Send for Region {}
unsafe impl Sync for Region {}

impl Region {
    pub fn new(size: usize) -> Result<Self, AllocError> {
        let size = size as u64;
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size as libc::size_t,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(AllocError {});
        }

        Ok(Self {
            base: NonNull::new(ptr as *mut u8).expect("mmap returned null?"),
            size: size as usize,
            commited: size as usize,
        })
    }

    pub fn base(&self) -> *mut u8 {
        self.base.as_ptr()
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn contains(&self, ptr: *const u8) -> bool {
        let base = self.base.as_ptr() as usize;
        let addr = ptr as usize;

        addr >= base && addr < base + self.size
    }

    pub fn reset(&mut self) -> Result<(), AllocError> {
        unsafe {
            libc::munmap(self.base.as_ptr() as *mut libc::c_void, self.size);
        }
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                self.size as libc::size_t,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            return Err(AllocError {});
        }

        self.base = NonNull::new(ptr as *mut u8).expect("mmap returned null?");

        Ok(())
    }
}
