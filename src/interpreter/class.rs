use crate::object::descriptor::TypeDescriptor;

/// produced by the classfile loader after parsing with cafebabe
pub struct Method {
    pub name: &'static str,
    pub descriptor: &'static str, // JVM type descriptor e.g. "(II)V"
    pub bytecode: Vec<u8>,        // raw Code attribute bytes
    pub max_locals: usize,
    pub max_stack: usize,
    pub is_static: bool,
    pub is_native: bool,

    // Exception table: each entry: (start_pc, end_pc, handler_pc, catch_type_index)
    pub exception_table: Vec<ExceptionHandler>,
}

#[derive(Clone)]
pub struct ExceptionHandler {
    pub start_pc: u16,
    pub end_pc: u16,
    pub handler_pc: u16,
    pub catch_type: Option<String>,
}
/// runtime class representation
/// one exists per loaded java class
pub struct Class {
    pub name: String, // e.g. "java/lang/Object"
    pub super_name: Option<String>,
    pub methods: Vec<Method>,
    pub fields: Vec<FieldInfo>,

    /// the gc's view of this class which field offsets hold references
    /// built at class load time from the field descriptor list
    pub type_desc: &'static TypeDescriptor,

    /// total size of the object's payload (fields only, no header)
    pub instance_size: usize,
}

pub struct FieldInfo {
    pub name: String,
    pub descriptor: String, // "I", "Ljava/lang/String;", "[B" etc.
    pub offset: usize,      // byte offset within `object_start()`
    pub is_static: bool,
    pub is_reference: bool, // true if this field is a gc pointer
}

impl Class {
    /// look up a method by name + descriptor.
    pub fn find_method(&self, name: &str, descriptor: &str) -> Option<&Method> {
        self.methods
            .iter()
            .find(|m| m.name == name && m.descriptor == descriptor)
    }
}
