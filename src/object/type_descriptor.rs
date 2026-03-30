use crate::object::header::GcHeader;

pub struct TypeDescriptor {
    pub name: &'static str, // for debugging
    pub instance_size: usize,
    pub pointer_offsets: &'static [usize], // offset of pointer field
}

impl TypeDescriptor {
    fn trace<F: FnMut(*mut GcHeader)>(&self, obj: *mut u8, mut visit: F) {
        for &offset in self.pointer_offsets {
            let field_ptr = unsafe { (obj as *mut u8).add(offset) as *mut GcHeader };
            visit(field_ptr);
        }
    }
}
