use crate::interpreter::class::{Class, ExceptionHandler, FieldInfo, Method};
use crate::object::descriptor::TypeDescriptor;
use cafebabe::attributes::AttributeData;
use cafebabe::{FieldAccessFlags, MethodAccessFlags};
use std::collections::HashMap;

#[derive(Debug)]
pub enum ClassLoadError {
    ParseError(String),
    MissingCodeAttribute(String),
    UnsupportedVersion(u16),
}

impl std::fmt::Display for ClassLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::ParseError(e) => write!(f, "parse error: {}", e),
            Self::MissingCodeAttribute(m) => write!(f, "no Code attribute on method: {}", m),
            Self::UnsupportedVersion(v) => write!(f, "unsupported class version: {}", v),
        }
    }
}
#[derive(Default)]
pub struct ClassLoader {
    classes: HashMap<String, Class>,
}

impl ClassLoader {
    /// load raw .class bytes
    /// `bytes` must live as long as the returned Class, cafebabe is zero-copy
    /// for strings. We call .to_string() to own the data so we can drop bytes.
    pub fn load(&mut self, bytes: &[u8]) -> Result<String, ClassLoadError> {
        let class_file =
            cafebabe::parse_class(bytes).map_err(|e| ClassLoadError::ParseError(e.to_string()))?;

        // java 21
        if class_file.major_version > 65 {
            return Err(ClassLoadError::UnsupportedVersion(class_file.major_version));
        }

        let class_name = class_file.this_class.to_string();
        let super_name = class_file.super_class.map(|s| s.to_string());

        // fields: compute byte offsets + find pointer fields for GC
        let mut fields: Vec<FieldInfo> = Vec::new();
        let mut offset = 0usize;
        let mut pointer_offsets: Vec<usize> = Vec::new();

        for field in &class_file.fields {
            let descriptor = field.descriptor.to_string();
            let is_static = field.access_flags.contains(FieldAccessFlags::STATIC);
            let is_ref = is_reference_type(&descriptor);
            let size = jvm_field_size(&descriptor);

            let align = size.next_power_of_two().min(8);
            offset = (offset + align - 1) & !(align - 1);

            //  skip offset tracking, static fields don't live in the instance
            if !is_static {
                if is_ref {
                    pointer_offsets.push(offset);
                }
                fields.push(FieldInfo {
                    name: field.name.to_string(),
                    descriptor: descriptor.clone(),
                    offset,
                    is_static: false,
                    is_reference: is_ref,
                });
                offset += size;
            } else {
                fields.push(FieldInfo {
                    name: field.name.to_string(),
                    descriptor: descriptor.clone(),
                    offset: 0, // offset into statics table, set separately
                    is_static: true,
                    is_reference: is_ref,
                });
            }
        }

        let instance_size = offset;

        // `TypeDescriptor` leaked to 'static for GC
        // GcHeader.type_desc is *const TypeDescriptor, must be 'static
        // we use Box::leak here; real JVM would use a 'static arena allocator
        let offsets_box: Box<[usize]> = pointer_offsets.into_boxed_slice();
        let offsets_static: &'static [usize] = Box::leak(offsets_box);

        let name_static: &'static str = Box::leak(class_name.clone().into_boxed_str());

        let type_desc: &'static TypeDescriptor = Box::leak(Box::new(TypeDescriptor {
            name: name_static,
            instance_size,
            pointer_offsets: offsets_static,
        }));

        // methods
        let mut methods: Vec<Method> = Vec::new();

        for method in &class_file.methods {
            let mname = method.name.to_string();
            let descriptor = method.descriptor.to_string();
            let is_static = method.access_flags.contains(MethodAccessFlags::STATIC);
            let is_native = method.access_flags.contains(MethodAccessFlags::NATIVE);
            let is_abstract = method.access_flags.contains(MethodAccessFlags::ABSTRACT);

            if is_native || is_abstract {
                // NO bytecode, push a stub so method resolution doesn't panic
                methods.push(Method {
                    name: Box::leak(mname.into_boxed_str()),
                    descriptor: Box::leak(descriptor.into_boxed_str()),
                    bytecode: Vec::new(),
                    max_locals: 0,
                    max_stack: 0,
                    is_static,
                    is_native,
                    exception_table: Vec::new(),
                });
                continue;
            }

            // find the Code attribute, every concrete method must have exactly one
            let code = method
                .attributes
                .iter()
                .find_map(|attr| {
                    if let AttributeData::Code(c) = &attr.data {
                        Some(c)
                    } else {
                        None
                    }
                })
                .ok_or_else(|| ClassLoadError::MissingCodeAttribute(mname.clone()))?;

            let exception_table: Vec<ExceptionHandler> = code
                .exception_table
                .iter()
                .map(|e| ExceptionHandler {
                    start_pc: e.start_pc,
                    end_pc: e.end_pc,
                    handler_pc: e.handler_pc,
                    catch_type: e.catch_type.as_ref().map(|name| name.to_string()),
                })
                .collect();

            methods.push(Method {
                name: Box::leak(mname.into_boxed_str()),
                descriptor: Box::leak(descriptor.into_boxed_str()),
                // code.code is the raw bytecode bytes, exactly what our interpreter consumes
                bytecode: code.code.to_vec(),
                max_locals: code.max_locals as usize,
                max_stack: code.max_stack as usize,
                is_static,
                is_native: false,
                exception_table,
            });
        }

        // store and return
        let class = Class {
            name: class_name.clone(),
            super_name,
            methods,
            fields,
            type_desc,
            instance_size,
        };

        self.classes.insert(class_name.clone(), class);
        Ok(class_name)
    }

    pub fn get(&self, name: &str) -> Option<&Class> {
        self.classes.get(name)
    }

    pub fn get_type_desc(&self, name: &str) -> Option<&'static TypeDescriptor> {
        self.classes.get(name).map(|c| c.type_desc)
    }
}

/// returns true if this JVM field descriptor is a heap reference
/// references: class types (Lsome/Class;) and array types ([...)
fn is_reference_type(descriptor: &str) -> bool {
    matches!(descriptor.chars().next(), Some('L') | Some('['))
}

/// Size in bytes of a JVM field type
fn jvm_field_size(descriptor: &str) -> usize {
    match descriptor.chars().next().unwrap_or('V') {
        'B' | 'Z' => 1, // byte, boolean
        'C' | 'S' => 2, // char, short
        'I' | 'F' => 4, // int, float
        'J' | 'D' => 8, // long, double
        'L' | '[' => 8, // reference (64-bit pointer)
        _ => 4,         // default
    }
}
