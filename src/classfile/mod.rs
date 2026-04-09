use crate::interpreter::class::{Class, CpEntry, ExceptionHandler, FieldInfo, Method};
use crate::object::descriptor::TypeDescriptor;
use cafebabe::attributes::AttributeData;
use cafebabe::{FieldAccessFlags, MethodAccessFlags};
use std::collections::HashMap;

#[derive(Debug)]
pub enum ClassLoadError {
    ParseError(String),
    MissingCodeAttribute(String),
    UnsupportedVersion(u16),
    ConstantPoolError(String),
}

impl std::fmt::Display for ClassLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::ParseError(e) => write!(f, "parse error: {}", e),
            Self::MissingCodeAttribute(m) => write!(f, "no Code attribute on method: {}", m),
            Self::UnsupportedVersion(v) => write!(f, "unsupported class version: {}", v),
            Self::ConstantPoolError(e) => write!(f, "constant pool error: {}", e),
        }
    }
}

#[derive(Default)]
pub struct ClassLoader {
    classes: HashMap<String, Class>,
}

impl ClassLoader {
    pub fn load(&mut self, bytes: &[u8]) -> Result<String, ClassLoadError> {
        let constant_pool = parse_constant_pool(bytes)?;
        let class_file =
            cafebabe::parse_class(bytes).map_err(|e| ClassLoadError::ParseError(e.to_string()))?;

        if class_file.major_version > 65 {
            return Err(ClassLoadError::UnsupportedVersion(class_file.major_version));
        }

        let class_name = class_file.this_class.to_string();
        let super_name = class_file.super_class.map(|s| s.to_string());

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
                    offset: 0,
                    is_static: true,
                    is_reference: is_ref,
                });
            }
        }

        let instance_size = offset;

        let offsets_box: Box<[usize]> = pointer_offsets.into_boxed_slice();
        let offsets_static: &'static [usize] = Box::leak(offsets_box);
        let name_static: &'static str = Box::leak(class_name.clone().into_boxed_str());

        let type_desc: &'static TypeDescriptor = Box::leak(Box::new(TypeDescriptor {
            name: name_static,
            instance_size,
            pointer_offsets: offsets_static,
        }));

        let mut methods: Vec<Method> = Vec::new();

        for method in &class_file.methods {
            let mname = method.name.to_string();
            let descriptor = method.descriptor.to_string();
            let is_static = method.access_flags.contains(MethodAccessFlags::STATIC);
            let is_native = method.access_flags.contains(MethodAccessFlags::NATIVE);
            let is_abstract = method.access_flags.contains(MethodAccessFlags::ABSTRACT);

            if is_native || is_abstract {
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
                bytecode: code.code.to_vec(),
                max_locals: code.max_locals as usize,
                max_stack: code.max_stack as usize,
                is_static,
                is_native: false,
                exception_table,
            });
        }

        let class = Class {
            name: class_name.clone(),
            super_name,
            constant_pool,
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

    pub fn class_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.classes.keys().cloned().collect();
        names.sort();
        names
    }
}

#[derive(Debug, Clone)]
enum RawCpEntry {
    Empty,
    Unused,
    Utf8(String),
    Integer(i32),
    Float(f32),
    Long(i64),
    Double(f64),
    Class {
        name_index: u16,
    },
    String {
        string_index: u16,
    },
    FieldRef {
        class_index: u16,
        name_and_type_index: u16,
    },
    MethodRef {
        class_index: u16,
        name_and_type_index: u16,
    },
    InterfaceMethodRef {
        class_index: u16,
        name_and_type_index: u16,
    },
    NameAndType {
        name_index: u16,
        descriptor_index: u16,
    },
    MethodHandle {
        reference_kind: u8,
        reference_index: u16,
    },
    MethodType {
        descriptor_index: u16,
    },
    Dynamic {
        bootstrap_method_attr_index: u16,
        name_and_type_index: u16,
    },
    InvokeDynamic {
        bootstrap_method_attr_index: u16,
        name_and_type_index: u16,
    },
    Module {
        name_index: u16,
    },
    Package {
        name_index: u16,
    },
}

fn parse_constant_pool(bytes: &[u8]) -> Result<Vec<CpEntry>, ClassLoadError> {
    let mut cursor = 0usize;

    let magic = read_u4(bytes, &mut cursor)?;
    if magic != 0xCAFE_BABE {
        return Err(ClassLoadError::ConstantPoolError(
            "invalid class magic header".into(),
        ));
    }

    let _minor = read_u2(bytes, &mut cursor)?;
    let _major = read_u2(bytes, &mut cursor)?;
    let cp_count = read_u2(bytes, &mut cursor)? as usize;

    if cp_count == 0 {
        return Err(ClassLoadError::ConstantPoolError(
            "invalid constant pool count".into(),
        ));
    }

    let mut raw = vec![RawCpEntry::Empty; cp_count];
    let mut index = 1usize;

    while index < cp_count {
        let tag = read_u1(bytes, &mut cursor)?;
        raw[index] = match tag {
            1 => {
                let len = read_u2(bytes, &mut cursor)? as usize;
                let raw_bytes = read_bytes(bytes, &mut cursor, len)?;
                let text = String::from_utf8_lossy(raw_bytes).into_owned();
                RawCpEntry::Utf8(text)
            }
            3 => RawCpEntry::Integer(read_u4(bytes, &mut cursor)? as i32),
            4 => RawCpEntry::Float(f32::from_bits(read_u4(bytes, &mut cursor)?)),
            5 => {
                let value = read_u8(bytes, &mut cursor)? as i64;
                if index + 1 < cp_count {
                    raw[index + 1] = RawCpEntry::Unused;
                }
                index += 1;
                RawCpEntry::Long(value)
            }
            6 => {
                let value = f64::from_bits(read_u8(bytes, &mut cursor)?);
                if index + 1 < cp_count {
                    raw[index + 1] = RawCpEntry::Unused;
                }
                index += 1;
                RawCpEntry::Double(value)
            }
            7 => RawCpEntry::Class {
                name_index: read_u2(bytes, &mut cursor)?,
            },
            8 => RawCpEntry::String {
                string_index: read_u2(bytes, &mut cursor)?,
            },
            9 => RawCpEntry::FieldRef {
                class_index: read_u2(bytes, &mut cursor)?,
                name_and_type_index: read_u2(bytes, &mut cursor)?,
            },
            10 => RawCpEntry::MethodRef {
                class_index: read_u2(bytes, &mut cursor)?,
                name_and_type_index: read_u2(bytes, &mut cursor)?,
            },
            11 => RawCpEntry::InterfaceMethodRef {
                class_index: read_u2(bytes, &mut cursor)?,
                name_and_type_index: read_u2(bytes, &mut cursor)?,
            },
            12 => RawCpEntry::NameAndType {
                name_index: read_u2(bytes, &mut cursor)?,
                descriptor_index: read_u2(bytes, &mut cursor)?,
            },
            15 => RawCpEntry::MethodHandle {
                reference_kind: read_u1(bytes, &mut cursor)?,
                reference_index: read_u2(bytes, &mut cursor)?,
            },
            16 => RawCpEntry::MethodType {
                descriptor_index: read_u2(bytes, &mut cursor)?,
            },
            17 => RawCpEntry::Dynamic {
                bootstrap_method_attr_index: read_u2(bytes, &mut cursor)?,
                name_and_type_index: read_u2(bytes, &mut cursor)?,
            },
            18 => RawCpEntry::InvokeDynamic {
                bootstrap_method_attr_index: read_u2(bytes, &mut cursor)?,
                name_and_type_index: read_u2(bytes, &mut cursor)?,
            },
            19 => RawCpEntry::Module {
                name_index: read_u2(bytes, &mut cursor)?,
            },
            20 => RawCpEntry::Package {
                name_index: read_u2(bytes, &mut cursor)?,
            },
            other => {
                return Err(ClassLoadError::ConstantPoolError(format!(
                    "unsupported constant pool tag {} at index {}",
                    other, index
                )));
            }
        };

        index += 1;
    }

    let mut resolved = vec![CpEntry::Empty; cp_count];
    for i in 1..cp_count {
        resolved[i] = resolve_cp_entry(&raw, i);
    }

    Ok(resolved)
}

fn resolve_cp_entry(raw: &[RawCpEntry], index: usize) -> CpEntry {
    match raw.get(index) {
        Some(RawCpEntry::Empty) | Some(RawCpEntry::Unused) | None => CpEntry::Empty,
        Some(RawCpEntry::Utf8(value)) => CpEntry::Utf8(value.clone()),
        Some(RawCpEntry::Integer(value)) => CpEntry::Int(*value),
        Some(RawCpEntry::Float(value)) => CpEntry::Float(*value),
        Some(RawCpEntry::Long(value)) => CpEntry::Long(*value),
        Some(RawCpEntry::Double(value)) => CpEntry::Double(*value),
        Some(RawCpEntry::Class { name_index }) => match utf8_at(raw, *name_index) {
            Some(name) => CpEntry::ClassRef(name),
            None => CpEntry::Unsupported,
        },
        Some(RawCpEntry::String { string_index }) => match utf8_at(raw, *string_index) {
            Some(value) => CpEntry::StringLiteral(value),
            None => CpEntry::Unsupported,
        },
        Some(RawCpEntry::FieldRef {
            class_index,
            name_and_type_index,
        }) => match (
            class_name_at(raw, *class_index),
            name_and_type_at(raw, *name_and_type_index),
        ) {
            (Some(class), Some((name, descriptor))) => CpEntry::FieldRef {
                class,
                name,
                descriptor,
            },
            _ => CpEntry::Unsupported,
        },
        Some(RawCpEntry::MethodRef {
            class_index,
            name_and_type_index,
        }) => match (
            class_name_at(raw, *class_index),
            name_and_type_at(raw, *name_and_type_index),
        ) {
            (Some(class), Some((name, descriptor))) => CpEntry::MethodRef {
                class,
                name,
                descriptor,
            },
            _ => CpEntry::Unsupported,
        },
        Some(RawCpEntry::InterfaceMethodRef {
            class_index,
            name_and_type_index,
        }) => match (
            class_name_at(raw, *class_index),
            name_and_type_at(raw, *name_and_type_index),
        ) {
            (Some(class), Some((name, descriptor))) => CpEntry::InterfaceMethodRef {
                class,
                name,
                descriptor,
            },
            _ => CpEntry::Unsupported,
        },
        Some(RawCpEntry::NameAndType {
            name_index,
            descriptor_index,
        }) => match (utf8_at(raw, *name_index), utf8_at(raw, *descriptor_index)) {
            (Some(name), Some(descriptor)) => CpEntry::NameAndType { name, descriptor },
            _ => CpEntry::Unsupported,
        },
        Some(RawCpEntry::MethodHandle { .. })
        | Some(RawCpEntry::MethodType { .. })
        | Some(RawCpEntry::Dynamic { .. })
        | Some(RawCpEntry::InvokeDynamic { .. })
        | Some(RawCpEntry::Module { .. })
        | Some(RawCpEntry::Package { .. }) => CpEntry::Unsupported,
    }
}

fn utf8_at(raw: &[RawCpEntry], index: u16) -> Option<String> {
    match raw.get(index as usize) {
        Some(RawCpEntry::Utf8(value)) => Some(value.clone()),
        _ => None,
    }
}

fn class_name_at(raw: &[RawCpEntry], index: u16) -> Option<String> {
    match raw.get(index as usize) {
        Some(RawCpEntry::Class { name_index }) => utf8_at(raw, *name_index),
        _ => None,
    }
}

fn name_and_type_at(raw: &[RawCpEntry], index: u16) -> Option<(String, String)> {
    match raw.get(index as usize) {
        Some(RawCpEntry::NameAndType {
            name_index,
            descriptor_index,
        }) => Some((utf8_at(raw, *name_index)?, utf8_at(raw, *descriptor_index)?)),
        _ => None,
    }
}

fn read_u1(bytes: &[u8], cursor: &mut usize) -> Result<u8, ClassLoadError> {
    if bytes.len() < *cursor + 1 {
        return Err(ClassLoadError::ConstantPoolError(
            "unexpected eof reading u1".into(),
        ));
    }
    let value = bytes[*cursor];
    *cursor += 1;
    Ok(value)
}

fn read_u2(bytes: &[u8], cursor: &mut usize) -> Result<u16, ClassLoadError> {
    if bytes.len() < *cursor + 2 {
        return Err(ClassLoadError::ConstantPoolError(
            "unexpected eof reading u2".into(),
        ));
    }
    let value = ((bytes[*cursor] as u16) << 8) | bytes[*cursor + 1] as u16;
    *cursor += 2;
    Ok(value)
}

fn read_u4(bytes: &[u8], cursor: &mut usize) -> Result<u32, ClassLoadError> {
    if bytes.len() < *cursor + 4 {
        return Err(ClassLoadError::ConstantPoolError(
            "unexpected eof reading u4".into(),
        ));
    }
    let value = ((bytes[*cursor] as u32) << 24)
        | ((bytes[*cursor + 1] as u32) << 16)
        | ((bytes[*cursor + 2] as u32) << 8)
        | bytes[*cursor + 3] as u32;
    *cursor += 4;
    Ok(value)
}

fn read_u8(bytes: &[u8], cursor: &mut usize) -> Result<u64, ClassLoadError> {
    if bytes.len() < *cursor + 8 {
        return Err(ClassLoadError::ConstantPoolError(
            "unexpected eof reading u8".into(),
        ));
    }
    let value = ((bytes[*cursor] as u64) << 56)
        | ((bytes[*cursor + 1] as u64) << 48)
        | ((bytes[*cursor + 2] as u64) << 40)
        | ((bytes[*cursor + 3] as u64) << 32)
        | ((bytes[*cursor + 4] as u64) << 24)
        | ((bytes[*cursor + 5] as u64) << 16)
        | ((bytes[*cursor + 6] as u64) << 8)
        | bytes[*cursor + 7] as u64;
    *cursor += 8;
    Ok(value)
}

fn read_bytes<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    count: usize,
) -> Result<&'a [u8], ClassLoadError> {
    if bytes.len() < *cursor + count {
        return Err(ClassLoadError::ConstantPoolError(
            "unexpected eof reading byte range".into(),
        ));
    }
    let out = &bytes[*cursor..*cursor + count];
    *cursor += count;
    Ok(out)
}

fn is_reference_type(descriptor: &str) -> bool {
    matches!(descriptor.chars().next(), Some('L') | Some('['))
}

fn jvm_field_size(descriptor: &str) -> usize {
    match descriptor.chars().next().unwrap_or('V') {
        'B' | 'Z' => 1,
        'C' | 'S' => 2,
        'I' | 'F' => 4,
        'J' | 'D' => 8,
        'L' | '[' => 8,
        _ => 4,
    }
}
