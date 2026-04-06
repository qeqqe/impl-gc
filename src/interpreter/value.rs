use crate::object::header::GcHeader;

/// typed value that lives on the operand stack or in a local variable slot.
/// java type system boils down to these at runtime
#[derive(Debug, Clone, Copy)]
pub enum Value {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    /// null reference or pointer to a GC object's GcHeader
    Reference(*mut GcHeader),
    /// uninitialized local
    Void,
}

impl Value {
    pub fn as_int(self) -> i32 {
        match self {
            Value::Int(v) => v,
            _ => panic!("type error: expected int"),
        }
    }

    pub fn as_long(self) -> i64 {
        match self {
            Value::Long(v) => v,
            _ => panic!("type error: expected long"),
        }
    }

    pub fn as_reference(self) -> *mut GcHeader {
        match self {
            Value::Reference(p) => p,
            _ => panic!("type error: expected ref"),
        }
    }

    pub fn is_reference(&self) -> bool {
        matches!(self, Value::Reference(_))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Reference(p) if p.is_null())
    }

    pub const NULL: Value = Value::Reference(std::ptr::null_mut());
}

