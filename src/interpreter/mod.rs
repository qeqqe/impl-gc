use std::collections::HashMap;

use crate::{
    classfile::ClassLoader,
    gc::root::StackFrame,
    mutator::{AllocResult, Mutator},
    object::header::GcHeader,
};
use opcode::*;

use self::{class::CpEntry, frame::Frame, value::Value};

pub mod class;
pub mod frame;
pub mod opcode;
pub mod value;

pub enum ExecResult {
    ReturnVoid,
    ReturnValue(Value),
    Exception(String),
    OutOfMemory,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StaticFieldKey {
    class: String,
    name: String,
    descriptor: String,
}

#[derive(Debug, Clone)]
struct ResolvedField {
    owner_class: String,
    descriptor: String,
    offset: usize,
    is_reference: bool,
    is_static: bool,
}

#[derive(Debug, Clone)]
struct ResolvedMethod {
    owner_class: String,
    method_name: &'static str,
    bytecode: Vec<u8>,
    max_locals: usize,
    max_stack: usize,
    is_native: bool,
}

pub struct Interpreter<'gc> {
    pub mutator: Mutator<'gc>,
    call_stack: Vec<Frame>,
    pub loader: ClassLoader,
    static_fields: HashMap<StaticFieldKey, Value>,
    trace: bool,
}

impl<'gc> Interpreter<'gc> {
    pub fn new(mutator: Mutator<'gc>, loader: ClassLoader) -> Self {
        Self {
            mutator,
            call_stack: Vec::with_capacity(256),
            loader,
            static_fields: HashMap::new(),
            trace: false,
        }
    }

    pub fn set_trace(&mut self, trace: bool) {
        self.trace = trace;
    }

    pub fn execute(
        &mut self,
        class_name: String,
        bytecode: Vec<u8>,
        max_locals: usize,
        max_stack: usize,
        args: Vec<Value>,
        method_name: &'static str,
    ) -> ExecResult {
        if args.len() > max_locals {
            return self.exception(format!(
                "too many args for {}.{}: {} > {}",
                class_name,
                method_name,
                args.len(),
                max_locals
            ));
        }

        if self.trace {
            eprintln!(
                "[call] {}.{} locals={} stack={} args={}",
                class_name,
                method_name,
                max_locals,
                max_stack,
                args.len()
            );
        }

        let mut frame = Frame::new(bytecode, max_locals, max_stack, method_name, class_name);
        for (index, arg) in args.into_iter().enumerate() {
            frame.store_local(index, arg);
        }

        let root_slots = frame.reference_slots();
        self.mutator.push_frame(StackFrame { slots: root_slots });
        self.call_stack.push(frame);

        let result = self.run();

        if self.trace {
            let frame = self.call_stack.last().unwrap();
            match &result {
                ExecResult::ReturnVoid => {
                    eprintln!("[ret] {}.{} => void", frame.class_name, frame.method_name);
                }
                ExecResult::ReturnValue(value) => {
                    eprintln!(
                        "[ret] {}.{} => {}",
                        frame.class_name,
                        frame.method_name,
                        self.value_string(*value)
                    );
                }
                ExecResult::Exception(error) => {
                    eprintln!(
                        "[ret] {}.{} => exception: {}",
                        frame.class_name, frame.method_name, error
                    );
                }
                ExecResult::OutOfMemory => {
                    eprintln!(
                        "[ret] {}.{} => out-of-memory",
                        frame.class_name, frame.method_name
                    );
                }
            }
        }

        self.mutator.pop_frame();
        self.call_stack.pop();
        result
    }

    pub fn invoke_static(
        &mut self,
        class_name: &str,
        method_name: &str,
        descriptor: &str,
        args: Vec<Value>,
    ) -> ExecResult {
        self.invoke_resolved(class_name, method_name, descriptor, args)
    }

    fn run(&mut self) -> ExecResult {
        loop {
            self.mutator.safepoint();

            let (instruction_pc, opcode, class_name, method_name) = {
                let frame = self.call_stack.last_mut().expect("empty call stack");
                if frame.pc >= frame.bytecode.len() {
                    let class_name = frame.class_name.clone();
                    let method_name = frame.method_name;
                    return self.exception(format!(
                        "pc out of bounds in {}.{}",
                        class_name, method_name
                    ));
                }
                let instruction_pc = frame.pc;
                let opcode = frame.read_u8();
                (
                    instruction_pc,
                    opcode,
                    frame.class_name.clone(),
                    frame.method_name,
                )
            };

            if self.trace {
                eprintln!(
                    "[op] {}.{} pc={} opcode=0x{:02x}",
                    class_name, method_name, instruction_pc, opcode
                );
            }

            match opcode {
                NOP => {}

                ACONST_NULL => self.call_stack.last_mut().unwrap().push(Value::NULL),
                ICONST_M1 => self.call_stack.last_mut().unwrap().push(Value::Int(-1)),
                ICONST_0 => self.call_stack.last_mut().unwrap().push(Value::Int(0)),
                ICONST_1 => self.call_stack.last_mut().unwrap().push(Value::Int(1)),
                ICONST_2 => self.call_stack.last_mut().unwrap().push(Value::Int(2)),
                ICONST_3 => self.call_stack.last_mut().unwrap().push(Value::Int(3)),
                ICONST_4 => self.call_stack.last_mut().unwrap().push(Value::Int(4)),
                ICONST_5 => self.call_stack.last_mut().unwrap().push(Value::Int(5)),
                LCONST_0 => self.call_stack.last_mut().unwrap().push(Value::Long(0)),

                BIPUSH => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let value = frame.read_u8() as i8 as i32;
                    frame.push(Value::Int(value));
                }
                SIPUSH => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let value = frame.read_i16() as i32;
                    frame.push(Value::Int(value));
                }
                LDC => {
                    let cp_index = self.call_stack.last_mut().unwrap().read_u8() as usize;
                    let value = match self.resolve_ldc(&class_name, cp_index) {
                        Ok(value) => value,
                        Err(error) => return self.exception(error),
                    };
                    self.call_stack.last_mut().unwrap().push(value);
                }

                ILOAD_0 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.push(frame.load_local(0));
                }
                ILOAD_1 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.push(frame.load_local(1));
                }
                ILOAD_2 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.push(frame.load_local(2));
                }
                ILOAD_3 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.push(frame.load_local(3));
                }
                ALOAD_0 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.push(frame.load_local(0));
                }
                ALOAD_1 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.push(frame.load_local(1));
                }
                ALOAD_2 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.push(frame.load_local(2));
                }
                ALOAD_3 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.push(frame.load_local(3));
                }
                ILOAD | ALOAD => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let index = frame.read_u8() as usize;
                    frame.push(frame.load_local(index));
                }

                ISTORE_0 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let value = frame.pop();
                    frame.store_local(0, value);
                }
                ISTORE_1 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let value = frame.pop();
                    frame.store_local(1, value);
                }
                ISTORE_2 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let value = frame.pop();
                    frame.store_local(2, value);
                }
                ISTORE_3 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let value = frame.pop();
                    frame.store_local(3, value);
                }
                ASTORE_0 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let value = frame.pop();
                    frame.store_local(0, value);
                }
                ASTORE_1 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let value = frame.pop();
                    frame.store_local(1, value);
                }
                ASTORE_2 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let value = frame.pop();
                    frame.store_local(2, value);
                }
                ASTORE_3 => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let value = frame.pop();
                    frame.store_local(3, value);
                }
                ISTORE | ASTORE => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let index = frame.read_u8() as usize;
                    let value = frame.pop();
                    frame.store_local(index, value);
                }

                IADD => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let b = frame.pop().as_int();
                    let a = frame.pop().as_int();
                    frame.push(Value::Int(a.wrapping_add(b)));
                }
                ISUB => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let b = frame.pop().as_int();
                    let a = frame.pop().as_int();
                    frame.push(Value::Int(a.wrapping_sub(b)));
                }
                IMUL => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let b = frame.pop().as_int();
                    let a = frame.pop().as_int();
                    frame.push(Value::Int(a.wrapping_mul(b)));
                }
                IDIV => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let b = frame.pop().as_int();
                    let a = frame.pop().as_int();
                    if b == 0 {
                        return self.exception("ArithmeticException: / by zero");
                    }
                    frame.push(Value::Int(a.wrapping_div(b)));
                }
                IREM => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let b = frame.pop().as_int();
                    let a = frame.pop().as_int();
                    if b == 0 {
                        return self.exception("ArithmeticException: / by zero");
                    }
                    frame.push(Value::Int(a.wrapping_rem(b)));
                }
                INEG => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let value = frame.pop().as_int();
                    frame.push(Value::Int(value.wrapping_neg()));
                }
                IINC => {
                    let frame = self.call_stack.last_mut().unwrap();
                    let index = frame.read_u8() as usize;
                    let delta = frame.read_u8() as i8 as i32;
                    let current = frame.load_local(index).as_int();
                    frame.store_local(index, Value::Int(current.wrapping_add(delta)));
                }

                POP => {
                    self.call_stack.last_mut().unwrap().pop();
                }
                DUP => {
                    self.call_stack.last_mut().unwrap().dup();
                }

                GOTO => {
                    let offset = self.call_stack.last_mut().unwrap().read_i16();
                    self.branch(instruction_pc, offset);
                }
                IFEQ => {
                    let (offset, value) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        (frame.read_i16(), frame.pop().as_int())
                    };
                    if value == 0 {
                        self.branch(instruction_pc, offset);
                    }
                }
                IFNE => {
                    let (offset, value) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        (frame.read_i16(), frame.pop().as_int())
                    };
                    if value != 0 {
                        self.branch(instruction_pc, offset);
                    }
                }
                IFLT => {
                    let (offset, value) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        (frame.read_i16(), frame.pop().as_int())
                    };
                    if value < 0 {
                        self.branch(instruction_pc, offset);
                    }
                }
                IFGE => {
                    let (offset, value) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        (frame.read_i16(), frame.pop().as_int())
                    };
                    if value >= 0 {
                        self.branch(instruction_pc, offset);
                    }
                }
                IFGT => {
                    let (offset, value) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        (frame.read_i16(), frame.pop().as_int())
                    };
                    if value > 0 {
                        self.branch(instruction_pc, offset);
                    }
                }
                IFLE => {
                    let (offset, value) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        (frame.read_i16(), frame.pop().as_int())
                    };
                    if value <= 0 {
                        self.branch(instruction_pc, offset);
                    }
                }
                IF_ICMPEQ => {
                    let (offset, b, a) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        let offset = frame.read_i16();
                        let b = frame.pop().as_int();
                        let a = frame.pop().as_int();
                        (offset, b, a)
                    };
                    if a == b {
                        self.branch(instruction_pc, offset);
                    }
                }
                IF_ICMPNE => {
                    let (offset, b, a) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        let offset = frame.read_i16();
                        let b = frame.pop().as_int();
                        let a = frame.pop().as_int();
                        (offset, b, a)
                    };
                    if a != b {
                        self.branch(instruction_pc, offset);
                    }
                }
                IF_ICMPLT => {
                    let (offset, b, a) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        let offset = frame.read_i16();
                        let b = frame.pop().as_int();
                        let a = frame.pop().as_int();
                        (offset, b, a)
                    };
                    if a < b {
                        self.branch(instruction_pc, offset);
                    }
                }
                IF_ICMPGE => {
                    let (offset, b, a) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        let offset = frame.read_i16();
                        let b = frame.pop().as_int();
                        let a = frame.pop().as_int();
                        (offset, b, a)
                    };
                    if a >= b {
                        self.branch(instruction_pc, offset);
                    }
                }
                IF_ICMPGT => {
                    let (offset, b, a) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        let offset = frame.read_i16();
                        let b = frame.pop().as_int();
                        let a = frame.pop().as_int();
                        (offset, b, a)
                    };
                    if a > b {
                        self.branch(instruction_pc, offset);
                    }
                }
                IF_ICMPLE => {
                    let (offset, b, a) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        let offset = frame.read_i16();
                        let b = frame.pop().as_int();
                        let a = frame.pop().as_int();
                        (offset, b, a)
                    };
                    if a <= b {
                        self.branch(instruction_pc, offset);
                    }
                }
                IFNULL => {
                    let (offset, value) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        (frame.read_i16(), frame.pop())
                    };
                    if value.is_null() {
                        self.branch(instruction_pc, offset);
                    }
                }
                IFNONNULL => {
                    let (offset, value) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        (frame.read_i16(), frame.pop())
                    };
                    if !value.is_null() {
                        self.branch(instruction_pc, offset);
                    }
                }

                NEW => {
                    let cp_index = self.call_stack.last_mut().unwrap().read_u16() as usize;
                    let target_class = match self.cp_entry(&class_name, cp_index) {
                        Some(CpEntry::ClassRef(name)) => name.clone(),
                        Some(other) => {
                            return self.exception(format!(
                                "NEW expected ClassRef at cp[{}], got {:?}",
                                cp_index, other
                            ));
                        }
                        None => {
                            return self.exception(format!(
                                "NEW invalid constant pool index {} in {}",
                                cp_index, class_name
                            ));
                        }
                    };

                    let type_desc = match self.loader.get_type_desc(&target_class) {
                        Some(desc) => desc,
                        None => {
                            return self
                                .exception(format!("NEW class not loaded: {}", target_class));
                        }
                    };

                    self.mutator.safepoint();
                    let object_ptr = match self.mutator.alloc(type_desc) {
                        AllocResult::Ok(ptr) => ptr.as_ptr(),
                        AllocResult::NeedMinorGC | AllocResult::NeedMajorGC => {
                            return ExecResult::OutOfMemory;
                        }
                        AllocResult::OutOfMemory => return ExecResult::OutOfMemory,
                    };

                    self.call_stack
                        .last_mut()
                        .unwrap()
                        .push(Value::Reference(object_ptr));
                }

                GETFIELD => {
                    let (cp_index, obj_ref) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        (frame.read_u16() as usize, frame.pop().as_reference())
                    };

                    if obj_ref.is_null() {
                        return self.exception("NullPointerException");
                    }

                    let (field_class, field_name, field_desc) =
                        match self.cp_entry(&class_name, cp_index) {
                            Some(CpEntry::FieldRef {
                                class,
                                name,
                                descriptor,
                            }) => (class.clone(), name.clone(), descriptor.clone()),
                            Some(other) => {
                                return self.exception(format!(
                                    "GETFIELD expected FieldRef at cp[{}], got {:?}",
                                    cp_index, other
                                ));
                            }
                            None => {
                                return self.exception(format!(
                                    "GETFIELD invalid constant pool index {} in {}",
                                    cp_index, class_name
                                ));
                            }
                        };

                    let field = match self.resolve_field(&field_class, &field_name, &field_desc) {
                        Some(field) => field,
                        None => {
                            return self.exception(format!(
                                "NoSuchFieldError: {}.{}:{}",
                                field_class, field_name, field_desc
                            ));
                        }
                    };

                    let value = unsafe {
                        let field_ptr = (*obj_ref).object_start().add(field.offset);
                        if field.is_reference {
                            Value::Reference((field_ptr as *const *mut GcHeader).read())
                        } else {
                            match field.descriptor.chars().next().unwrap_or('I') {
                                'B' => Value::Int((field_ptr as *const i8).read() as i32),
                                'Z' => Value::Int((field_ptr as *const u8).read() as i32),
                                'C' => Value::Int((field_ptr as *const u16).read() as i32),
                                'S' => Value::Int((field_ptr as *const i16).read() as i32),
                                'I' => Value::Int((field_ptr as *const i32).read()),
                                'J' => Value::Long((field_ptr as *const i64).read()),
                                'F' => Value::Float((field_ptr as *const f32).read()),
                                'D' => Value::Double((field_ptr as *const f64).read()),
                                _ => Value::Int((field_ptr as *const i32).read()),
                            }
                        }
                    };

                    self.call_stack.last_mut().unwrap().push(value);
                }

                PUTFIELD => {
                    let (cp_index, new_value, obj_ref) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        let cp_index = frame.read_u16() as usize;
                        let new_value = frame.pop();
                        let obj_ref = frame.pop().as_reference();
                        (cp_index, new_value, obj_ref)
                    };

                    if obj_ref.is_null() {
                        return self.exception("NullPointerException");
                    }

                    let (field_class, field_name, field_desc) =
                        match self.cp_entry(&class_name, cp_index) {
                            Some(CpEntry::FieldRef {
                                class,
                                name,
                                descriptor,
                            }) => (class.clone(), name.clone(), descriptor.clone()),
                            Some(other) => {
                                return self.exception(format!(
                                    "PUTFIELD expected FieldRef at cp[{}], got {:?}",
                                    cp_index, other
                                ));
                            }
                            None => {
                                return self.exception(format!(
                                    "PUTFIELD invalid constant pool index {} in {}",
                                    cp_index, class_name
                                ));
                            }
                        };

                    let field = match self.resolve_field(&field_class, &field_name, &field_desc) {
                        Some(field) => field,
                        None => {
                            return self.exception(format!(
                                "NoSuchFieldError: {}.{}:{}",
                                field_class, field_name, field_desc
                            ));
                        }
                    };

                    if field.is_static {
                        return self.exception(format!(
                            "IncompatibleClassChangeError: {}.{} is static",
                            field_class, field_name
                        ));
                    }

                    if field.is_reference {
                        match new_value {
                            Value::Reference(new_ref) => unsafe {
                                self.mutator.write_barrier(obj_ref, field.offset, new_ref);
                            },
                            other => {
                                return self.exception(format!(
                                    "type mismatch on reference field {}.{} with value {}",
                                    field_class,
                                    field_name,
                                    self.value_string(other)
                                ));
                            }
                        }
                    } else {
                        unsafe {
                            let field_ptr = (*obj_ref).object_start().add(field.offset);
                            match (field.descriptor.chars().next().unwrap_or('I'), new_value) {
                                ('B', Value::Int(v)) => (field_ptr as *mut i8).write(v as i8),
                                ('Z', Value::Int(v)) => {
                                    (field_ptr as *mut u8).write((v != 0) as u8)
                                }
                                ('C', Value::Int(v)) => (field_ptr as *mut u16).write(v as u16),
                                ('S', Value::Int(v)) => (field_ptr as *mut i16).write(v as i16),
                                ('I', Value::Int(v)) => (field_ptr as *mut i32).write(v),
                                ('J', Value::Long(v)) => (field_ptr as *mut i64).write(v),
                                ('F', Value::Float(v)) => (field_ptr as *mut f32).write(v),
                                ('D', Value::Double(v)) => (field_ptr as *mut f64).write(v),
                                (_, value) => {
                                    return self.exception(format!(
                                        "type mismatch on primitive field {}.{}:{} with value {}",
                                        field_class,
                                        field_name,
                                        field.descriptor,
                                        self.value_string(value)
                                    ));
                                }
                            }
                        }
                    }
                }

                GETSTATIC => {
                    let cp_index = self.call_stack.last_mut().unwrap().read_u16() as usize;
                    let (field_class, field_name, field_desc) =
                        match self.cp_entry(&class_name, cp_index) {
                            Some(CpEntry::FieldRef {
                                class,
                                name,
                                descriptor,
                            }) => (class.clone(), name.clone(), descriptor.clone()),
                            Some(other) => {
                                return self.exception(format!(
                                    "GETSTATIC expected FieldRef at cp[{}], got {:?}",
                                    cp_index, other
                                ));
                            }
                            None => {
                                return self.exception(format!(
                                    "GETSTATIC invalid constant pool index {} in {}",
                                    cp_index, class_name
                                ));
                            }
                        };

                    let field = match self.resolve_field(&field_class, &field_name, &field_desc) {
                        Some(field) => field,
                        None => {
                            return self.exception(format!(
                                "NoSuchFieldError: {}.{}:{}",
                                field_class, field_name, field_desc
                            ));
                        }
                    };

                    if !field.is_static {
                        return self.exception(format!(
                            "IncompatibleClassChangeError: {}.{} is not static",
                            field_class, field_name
                        ));
                    }

                    let key = StaticFieldKey {
                        class: field.owner_class,
                        name: field_name,
                        descriptor: field_desc.clone(),
                    };

                    let value = *self
                        .static_fields
                        .entry(key)
                        .or_insert_with(|| default_value_for_descriptor(&field_desc));
                    self.call_stack.last_mut().unwrap().push(value);
                }

                PUTSTATIC => {
                    let (cp_index, value) = {
                        let frame = self.call_stack.last_mut().unwrap();
                        (frame.read_u16() as usize, frame.pop())
                    };

                    let (field_class, field_name, field_desc) =
                        match self.cp_entry(&class_name, cp_index) {
                            Some(CpEntry::FieldRef {
                                class,
                                name,
                                descriptor,
                            }) => (class.clone(), name.clone(), descriptor.clone()),
                            Some(other) => {
                                return self.exception(format!(
                                    "PUTSTATIC expected FieldRef at cp[{}], got {:?}",
                                    cp_index, other
                                ));
                            }
                            None => {
                                return self.exception(format!(
                                    "PUTSTATIC invalid constant pool index {} in {}",
                                    cp_index, class_name
                                ));
                            }
                        };

                    let field = match self.resolve_field(&field_class, &field_name, &field_desc) {
                        Some(field) => field,
                        None => {
                            return self.exception(format!(
                                "NoSuchFieldError: {}.{}:{}",
                                field_class, field_name, field_desc
                            ));
                        }
                    };

                    if !field.is_static {
                        return self.exception(format!(
                            "IncompatibleClassChangeError: {}.{} is not static",
                            field_class, field_name
                        ));
                    }

                    if !value_matches_descriptor(value, &field_desc) {
                        return self.exception(format!(
                            "type mismatch on static field {}.{}:{} with value {}",
                            field_class,
                            field_name,
                            field_desc,
                            self.value_string(value)
                        ));
                    }

                    let key = StaticFieldKey {
                        class: field.owner_class,
                        name: field_name,
                        descriptor: field_desc,
                    };
                    self.static_fields.insert(key, value);
                }

                INVOKEVIRTUAL | INVOKESPECIAL | INVOKESTATIC => {
                    let cp_index = self.call_stack.last_mut().unwrap().read_u16() as usize;

                    let (target_class, target_name, target_desc) =
                        match self.cp_entry(&class_name, cp_index) {
                            Some(CpEntry::MethodRef {
                                class,
                                name,
                                descriptor,
                            })
                            | Some(CpEntry::InterfaceMethodRef {
                                class,
                                name,
                                descriptor,
                            }) => (class.clone(), name.clone(), descriptor.clone()),
                            Some(other) => {
                                return self.exception(format!(
                                    "INVOKE expected MethodRef at cp[{}], got {:?}",
                                    cp_index, other
                                ));
                            }
                            None => {
                                return self.exception(format!(
                                    "INVOKE invalid constant pool index {} in {}",
                                    cp_index, class_name
                                ));
                            }
                        };

                    let arg_count = count_args(&target_desc);
                    let is_static = opcode == INVOKESTATIC;
                    let total_pop = if is_static { arg_count } else { arg_count + 1 };

                    let mut args = Vec::with_capacity(total_pop);
                    {
                        let frame = self.call_stack.last_mut().unwrap();
                        for _ in 0..total_pop {
                            args.push(frame.pop());
                        }
                    }
                    args.reverse();

                    if !is_static {
                        match args.first().copied() {
                            Some(Value::Reference(ptr)) if ptr.is_null() => {
                                return self.exception("NullPointerException");
                            }
                            Some(Value::Reference(_)) => {}
                            Some(other) => {
                                return self.exception(format!(
                                    "INVOKE expected receiver reference, got {}",
                                    self.value_string(other)
                                ));
                            }
                            None => {
                                return self.exception("INVOKE missing receiver argument");
                            }
                        }
                    }

                    self.mutator.safepoint();

                    let result = if opcode == INVOKEVIRTUAL {
                        let receiver = match args.first().copied() {
                            Some(Value::Reference(ptr)) => ptr,
                            _ => unreachable!(),
                        };
                        let runtime_class = unsafe { (&*(*receiver).type_desc).name.to_string() };
                        self.invoke_resolved(&runtime_class, &target_name, &target_desc, args)
                    } else {
                        self.invoke_resolved(&target_class, &target_name, &target_desc, args)
                    };

                    match result {
                        ExecResult::ReturnVoid => {}
                        ExecResult::ReturnValue(value) => {
                            self.call_stack.last_mut().unwrap().push(value);
                        }
                        other => return other,
                    }
                }

                RETURN => return ExecResult::ReturnVoid,
                IRETURN | ARETURN => {
                    let value = self.call_stack.last_mut().unwrap().pop();
                    return ExecResult::ReturnValue(value);
                }

                ATHROW => {
                    let thrown = self.call_stack.last_mut().unwrap().pop();
                    return self.exception(format!("throw {}", self.value_string(thrown)));
                }

                _ => {
                    return self.exception(format!(
                        "unsupported opcode: 0x{:02x} at {}.{} pc={}",
                        opcode, class_name, method_name, instruction_pc
                    ));
                }
            }
        }
    }

    fn branch(&mut self, instruction_pc: usize, offset: i16) {
        if offset < 0 {
            self.mutator.safepoint();
        }
        self.call_stack
            .last_mut()
            .unwrap()
            .jump(instruction_pc, offset);
    }

    fn cp_entry(&self, class_name: &str, index: usize) -> Option<&CpEntry> {
        self.loader.get(class_name)?.constant_pool.get(index)
    }

    fn resolve_ldc(&self, class_name: &str, index: usize) -> Result<Value, String> {
        match self.cp_entry(class_name, index) {
            Some(CpEntry::Int(value)) => Ok(Value::Int(*value)),
            Some(CpEntry::Long(value)) => Ok(Value::Long(*value)),
            Some(CpEntry::Float(value)) => Ok(Value::Float(*value)),
            Some(CpEntry::Double(value)) => Ok(Value::Double(*value)),
            Some(CpEntry::StringLiteral(_)) => Ok(Value::NULL),
            Some(other) => Err(format!(
                "LDC unsupported constant pool type at {}[{}]: {:?}",
                class_name, index, other
            )),
            None => Err(format!(
                "LDC invalid constant pool index {} for class {}",
                index, class_name
            )),
        }
    }

    fn resolve_field(
        &self,
        class_name: &str,
        field_name: &str,
        descriptor: &str,
    ) -> Option<ResolvedField> {
        let mut current = Some(class_name.to_string());

        while let Some(name) = current {
            let class = self.loader.get(&name)?;
            if let Some(field) = class.find_field(field_name, descriptor) {
                return Some(ResolvedField {
                    owner_class: name,
                    descriptor: field.descriptor.clone(),
                    offset: field.offset,
                    is_reference: field.is_reference,
                    is_static: field.is_static,
                });
            }
            current = class.super_name.clone();
        }

        None
    }

    fn resolve_method(
        &self,
        class_name: &str,
        method_name: &str,
        descriptor: &str,
    ) -> Option<ResolvedMethod> {
        let mut current = Some(class_name.to_string());

        while let Some(name) = current {
            let class = self.loader.get(&name)?;
            if let Some(method) = class.find_method(method_name, descriptor) {
                return Some(ResolvedMethod {
                    owner_class: name,
                    method_name: method.name,
                    bytecode: method.bytecode.clone(),
                    max_locals: method.max_locals,
                    max_stack: method.max_stack,
                    is_native: method.is_native,
                });
            }
            current = class.super_name.clone();
        }

        None
    }

    fn invoke_resolved(
        &mut self,
        class_name: &str,
        method_name: &str,
        descriptor: &str,
        args: Vec<Value>,
    ) -> ExecResult {
        if class_name == "java/lang/Object" && method_name == "<init>" && descriptor == "()V" {
            return ExecResult::ReturnVoid;
        }

        let resolved = match self.resolve_method(class_name, method_name, descriptor) {
            Some(method) => method,
            None => {
                return self.exception(format!(
                    "NoSuchMethodError: {}.{}{}",
                    class_name, method_name, descriptor
                ));
            }
        };

        if resolved.is_native {
            return self.invoke_native(class_name, method_name, descriptor, args);
        }

        self.execute(
            resolved.owner_class,
            resolved.bytecode,
            resolved.max_locals,
            resolved.max_stack,
            args,
            resolved.method_name,
        )
    }

    fn invoke_native(
        &mut self,
        class_name: &str,
        method_name: &str,
        _descriptor: &str,
        args: Vec<Value>,
    ) -> ExecResult {
        match (class_name, method_name) {
            ("java/lang/System", "exit") => {
                let code = args.first().map(|v| v.as_int()).unwrap_or(0);
                std::process::exit(code);
            }
            ("java/io/PrintStream", "println") => {
                match args.get(1).copied() {
                    Some(Value::Int(v)) => println!("{}", v),
                    Some(Value::Long(v)) => println!("{}", v),
                    Some(Value::Float(v)) => println!("{}", v),
                    Some(Value::Double(v)) => println!("{}", v),
                    Some(Value::Reference(ptr)) if ptr.is_null() => println!("null"),
                    Some(value) => println!("{}", self.value_string(value)),
                    None => println!(),
                }
                ExecResult::ReturnVoid
            }
            _ => self.exception(format!(
                "UnsatisfiedLinkError: native {}.{}",
                class_name, method_name
            )),
        }
    }

    fn exception(&self, message: impl Into<String>) -> ExecResult {
        let mut text = message.into();
        if !self.call_stack.is_empty() {
            text.push_str("\nstack:");
            for frame in self.call_stack.iter().rev() {
                text.push_str(&format!(
                    "\n  at {}.{} (pc={})",
                    frame.class_name, frame.method_name, frame.pc
                ));
            }
        }
        ExecResult::Exception(text)
    }

    fn value_string(&self, value: Value) -> String {
        match value {
            Value::Int(v) => format!("{}", v),
            Value::Long(v) => format!("{}", v),
            Value::Float(v) => format!("{}", v),
            Value::Double(v) => format!("{}", v),
            Value::Reference(ptr) if ptr.is_null() => "null".into(),
            Value::Reference(ptr) => unsafe {
                let header = &*ptr;
                let type_name = if header.type_desc.is_null() {
                    "<?>".to_string()
                } else {
                    (&*header.type_desc).name.to_string()
                };
                format!("<ref {} @{:p}>", type_name, ptr)
            },
            Value::Void => "void".into(),
        }
    }
}

fn default_value_for_descriptor(descriptor: &str) -> Value {
    match descriptor.chars().next().unwrap_or('V') {
        'B' | 'C' | 'I' | 'S' | 'Z' => Value::Int(0),
        'J' => Value::Long(0),
        'F' => Value::Float(0.0),
        'D' => Value::Double(0.0),
        'L' | '[' => Value::NULL,
        _ => Value::Void,
    }
}

fn value_matches_descriptor(value: Value, descriptor: &str) -> bool {
    match descriptor.chars().next().unwrap_or('V') {
        'B' | 'C' | 'I' | 'S' | 'Z' => matches!(value, Value::Int(_)),
        'J' => matches!(value, Value::Long(_)),
        'F' => matches!(value, Value::Float(_)),
        'D' => matches!(value, Value::Double(_)),
        'L' | '[' => matches!(value, Value::Reference(_)),
        _ => matches!(value, Value::Void),
    }
}

pub fn count_args(descriptor: &str) -> usize {
    let args = descriptor
        .strip_prefix('(')
        .and_then(|value| value.split(')').next())
        .unwrap_or("");

    let mut count = 0usize;
    let mut chars = args.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            'B' | 'C' | 'D' | 'F' | 'I' | 'J' | 'S' | 'Z' => count += 1,
            'L' => {
                while chars.next() != Some(';') {}
                count += 1;
            }
            '[' => {
                while matches!(chars.peek(), Some('[')) {
                    chars.next();
                }
                match chars.next() {
                    Some('L') => while chars.next() != Some(';') {},
                    Some(_) => {}
                    None => {}
                }
                count += 1;
            }
            _ => {}
        }
    }

    count
}
