use impl_gc::{
    classfile::ClassLoader, gc::collector::Collector, heap::bump::BumpAllocator, mutator::Mutator,
};
use std::fs;

fn main() {
    let mut collector = Collector::new(4 * 1024 * 1024, 32 * 1024 * 1024);

    let mutator = Mutator::new(
        BumpAllocator::from_region(collector.young_region()),
        collector.card_table(),
        collector.old_region(),
        collector.young_region(),
        &collector.safepoint,
    );

    let mut loader = ClassLoader::default();
    let bytes = fs::read("sample/GcStressTest.class").expect("class file not found");

    let class_name = loader.load(&bytes).expect("class load failed");
    println!("loaded: {}", class_name);

    let class = loader.get(&class_name).unwrap();
    println!("instance_size: {} bytes", class.instance_size);
    println!("methods: {}", class.methods.len());
    for m in &class.methods {
        println!(
            "  {} {} (bytecode: {} bytes)",
            m.name,
            m.descriptor,
            m.bytecode.len()
        );
    }
}
