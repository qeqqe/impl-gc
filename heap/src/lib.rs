pub mod bump;
pub mod freelist;
pub mod region;

#[allow(dead_code, unused_allocation, unused_mut, unused_must_use)]
#[cfg(test)]
mod test {
    use crate::{bump, region};

    #[test]
    fn test1() {
        let neww = region::Region::new(1024).unwrap();
        println!("base: {:?}", neww.base());
        assert_eq!(neww.size(), 1024);
    }

    #[test]
    fn allocate() {
        let mut r = region::Region::new(1024).unwrap();
        let mut bumper = bump::BumpAllocator::new(&mut r);
        bumper.alloc(123, 128);
        assert_eq!(bumper.used(), 123);
        bumper.reset();
        assert_eq!(bumper.used(), 0);
    }
}
