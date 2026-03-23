#[allow(dead_code, unused_allocation, unused_mut)]
pub mod region;

#[cfg(test)]
mod test {
    use crate::region;

    #[test]
    fn test1() {
        let neww = region::Region::new(1024).unwrap();
        println!("base: {:?}", neww.base());
        assert_eq!(neww.size(), 1024);
    }
}
