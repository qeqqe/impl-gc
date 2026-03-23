use core::fmt;
use std::{error::Error, marker::PhantomPinned};

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
    mem_chunk: Vec<u8>,
    _pin: PhantomPinned,
}

impl Region {
    fn new(size: usize) -> Result<Self, AllocError> {
        Ok(Self {
            mem_chunk: Vec::with_capacity(size),
            _pin: PhantomPinned,
        })
    }
}
