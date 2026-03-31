use crate::object::header::GcHeader;

pub struct TypeDescriptor {
    pub name: &'static str, // for debugging
    pub instance_size: usize,
    pub pointer_offsets: &'static [usize], // offset of pointer field
}

impl TypeDescriptor {
    /// walk every pointer field of `obj`, call `visit` with the GcHeader
    /// of each child object (for marking GRAY and eventually BLACK),
    /// Used exclusively by the mark phase
    ///
    /// example:
    /// ```rust
    /// // user defined data types.
    /// struct Node {
    ///     value: u32, // <- skip, no pointer (pointer offset 8),
    ///     left: *mut Node, // <- trace, Gc managed reference (pointer offset 16),
    ///     right: *mut Node, // <- trace, Gc managed reference (pointer offset 24),
    /// }
    /// ```
    pub fn trace<F: FnMut(*mut GcHeader)>(&self, obj: *mut u8, mut visit: F) {
        for &offset in self.pointer_offsets {
            unsafe {
                // address of the pointer field inside this object
                let field_addr = obj.add(offset) as *mut *mut u8;

                // read the pointer VALUE stored at that field
                let child_user_ptr = *field_addr;

                // null = unset field, skip
                if child_user_ptr.is_null() {
                    continue;
                }

                //  step back past GcHeader to get the child's header
                let child_header = GcHeader::from_object_ptr(child_user_ptr);

                visit(child_header);
            }
        }
    }
}
