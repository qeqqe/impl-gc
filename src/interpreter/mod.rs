use crate::{
    gc::root::StackFrame,
    interpreter::{frame::Frame, value::Value},
    mutator::{AllocResult, Mutator},
    object::descriptor::TypeDescriptor,
};
use opcode::*;

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

/// owns the call stack and drive bytecode execution
/// one Interpreter exists per mutator thread. holds the java call stack
/// (Vec<Frame>) and references to all GC through the `Mutator`
///
/// this does NOT own the GC collector, the caller (thread runner) owns both
/// `Interpreter` and `Collector`, wiring them together in the retry loop
pub struct Interpreter<'gc> {
    /// per thread gc interface
    pub mutator: Mutator<'gc>,

    /// java call stack
    call_stack: Vec<Frame>,

    /// Class/method resolution table, maps type indices
    /// to TypeDescriptor and method indices to bytecode
    pub type_table: Vec<&'static TypeDescriptor>,

    /// static fie storage
    pub statics: Vec<Value>,
}

impl<'gc> Interpreter<'gc> {
    pub fn new(mutator: Mutator<'gc>) -> Self {
        Self {
            mutator,
            call_stack: Vec::with_capacity(256),
            type_table: Vec::new(),
            statics: Vec::new(),
        }
    }
    /// Top level call, interpreter loop runs here.
    /// exectue bytecode for a method
    /// `bytecode`     : raw bytes of code attribute
    /// `max_locals`   : size of local variable
    /// `max_stack`    : max operan stack depth
    /// `args`         : initial local vars
    pub fn execute(
        &mut self,
        bytecode: Vec<u8>,
        max_locals: usize,
        max_stack: usize,
        args: Vec<Value>,
        method_name: &'static str,
    ) -> ExecResult {
        let mut frame = Frame::new(bytecode, max_locals, max_stack, method_name);

        // copy args to local[0..args.len()]
        for (i, arg) in args.into_iter().enumerate() {
            frame.store_local(i, arg);
        }

        // register this frame ref slot with the RootRegistry
        // so GC can find all live object reference in this frame
        let root_slot = frame.reference_slots();
        self.mutator.push_frame(StackFrame { slots: root_slot });

        self.call_stack.push(frame);

        let result = self.run();

        // pop the fucking root when method exits with eitherr
        // a exception or not
        self.mutator.pop_frame();
        self.call_stack.pop();

        result
    }

    // The loop,
    // Fetches one opcode and dispatchesW
    fn run(&mut self) -> ExecResult {
        loop {
            // every iteration is potential safepoint, also every
            // cheap when no GC (one atomic load)
            self.mutator.safepoint();

            let frame = self.call_stack.last_mut().unwrap();

            // branch instruction need the instruction's
            // start address for the offset calculation
            // so save pc
            let instruction_pc = frame.pc;
            let opcode = frame.read_u8();

            match opcode {
                // CONSTANTS
                // push constant values onto operand stack
                ACONST_NULL => {
                    frame.push(Value::NULL);
                }
                ICONST_M1 => {
                    frame.push(Value::Int(-1));
                }
                ICONST_0 => {
                    frame.push(Value::Int(0));
                }
                ICONST_1 => {
                    frame.push(Value::Int(1));
                }
                ICONST_2 => {
                    frame.push(Value::Int(2));
                }
                ICONST_3 => {
                    frame.push(Value::Int(3));
                }
                ICONST_4 => {
                    frame.push(Value::Int(4));
                }
                ICONST_5 => {
                    frame.push(Value::Int(5));
                }

                BIPUSH => {
                    // byte sized integer literal (sign extended to int)
                    let bytes = frame.read_u8() as i8 as i32;
                    frame.push(Value::Int(bytes));
                }

                SIPUSH => {
                    let short = frame.read_i16() as i32;
                    // short sized int literal (sign extended to int)
                    frame.push(Value::Int(short));
                }

                // LOCAL VARIABLE LOADS

                // ALOAD variants load references, GC should be able to
                // access these through the shadow stack (already pushed in `execute()`)
                ILOAD_0 => {
                    let v = frame.load_local(0);
                    frame.push(v);
                }

                ILOAD_1 => {
                    let v = frame.load_local(1);
                    frame.push(v);
                }

                ILOAD_2 => {
                    let v = frame.load_local(2);
                    frame.push(v);
                }
                ILOAD_3 => {
                    let v = frame.load_local(3);
                    frame.push(v);
                }
                ALOAD_0 => {
                    // load reference from local[0] (typically `this`)
                    let v = frame.load_local(0);
                    frame.push(v);
                }
                ALOAD_1 => {
                    let v = frame.load_local(1);
                    frame.push(v);
                }
                ALOAD_2 => {
                    let v = frame.load_local(2);
                    frame.push(v);
                }
                ALOAD_3 => {
                    let v = frame.load_local(3);
                    frame.push(v);
                }

                ILOAD => {
                    let idx = frame.read_u8() as usize;
                    let v = frame.load_local(idx);
                    frame.push(v);
                }

                ALOAD => {
                    let idx = frame.read_u8() as usize;
                    let v = frame.load_local(idx);
                    frame.push(v);
                }
                // LOCAL VAR STORE

                // pop operand stack top into a local slot
                // ASTORE variants: after storing a reference into a local,
                // the `reference_slots()` registration already covers it because
                // we registered &mut locals[i], writing through the slot
                // automatically updates what the GC sees.
                ISTORE_0 => {
                    let v = frame.pop();
                    frame.store_local(0, v);
                }
                ISTORE_1 => {
                    let v = frame.pop();
                    frame.store_local(1, v);
                }
                ISTORE_2 => {
                    let v = frame.pop();
                    frame.store_local(2, v);
                }
                ISTORE_3 => {
                    let v = frame.pop();
                    frame.store_local(3, v);
                }

                ASTORE_0 => {
                    let v = frame.pop();
                    frame.store_local(0, v);
                }
                ASTORE_1 => {
                    let v = frame.pop();
                    frame.store_local(1, v);
                }
                ASTORE_2 => {
                    let v = frame.pop();
                    frame.store_local(2, v);
                }
                ASTORE_3 => {
                    let v = frame.pop();
                    frame.store_local(3, v);
                }

                ISTORE | ASTORE => {
                    let idx = frame.read_u8() as usize;
                    let v = frame.pop();
                    frame.store_local(idx, v);
                }

                // ARITHMETIC
                IADD => {
                    let b = frame.pop().as_int();
                    let a = frame.pop().as_int();

                    frame.push(Value::Int(a.wrapping_add(b)))
                }

                ISUB => {
                    let b = frame.pop().as_int();
                    let a = frame.pop().as_int();

                    frame.push(Value::Int(a.wrapping_sub(b)));
                }

                IMUL => {
                    let b = frame.pop().as_int();
                    let a = frame.pop().as_int();

                    frame.push(Value::Int(a.wrapping_mul(b)));
                }

                IDIV => {
                    let b = frame.pop().as_int();
                    let a = frame.pop().as_int();

                    if b == 0 {
                        return ExecResult::Exception("Division by zero".into());
                    }

                    frame.push(Value::Int(a.wrapping_div(b)));
                }

                IREM => {
                    let b = frame.pop().as_int();
                    let a = frame.pop().as_int();

                    if b == 0 {
                        return ExecResult::Exception("Modulo by zero".into());
                    }
                    frame.push(Value::Int(a.wrapping_rem(b)));
                }

                INEG => {
                    let v = frame.pop().as_int();
                    frame.push(Value::Int(v.wrapping_neg()));
                }

                IINC => {
                    let idx = frame.read_u8() as usize;
                    let constant = frame.read_u8() as i8 as i32;
                    let current = frame.load_local(idx).as_int();
                    frame.store_local(idx, Value::Int(current.wrapping_add(constant)));
                }

                // STACK
                POP => {
                    frame.pop();
                }

                DUP => {
                    frame.dup();
                }

                // BRANCHES
                // conditional/unconditional jumps
                //
                // SAFEPOINT: poll on EVERY backward jump (offset < 0).
                // forward jump don't create loops so no need for polling.
                //
                // branch offset is relative to the start of the branch instruction
                // not the current pc
                GOTO => {
                    let offset = frame.read_i16();

                    // back edge
                    if offset < 0 {
                        drop(frame);
                        self.mutator.safepoint();
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.jump(instruction_pc, offset);
                    } else {
                        frame.jump(instruction_pc, offset);
                    }
                }

                IFEQ => {
                    // if top == 0, branch
                    let offset = frame.read_i16();
                    let v = frame.pop().as_int();
                    if v == 0 {
                        if offset < 0 {
                            drop(frame);
                            self.mutator.safepoint();
                            self.call_stack
                                .last_mut()
                                .unwrap()
                                .jump(instruction_pc, offset);
                        } else {
                            frame.jump(instruction_pc, offset);
                        }
                    }
                    // no jump, frame.pc already past operand
                }
                IFNE => {
                    let offset = frame.read_i16();
                    let v = frame.pop().as_int();
                    if v != 0 {
                        self.branch(instruction_pc, offset);
                    }
                }
                IFLT => {
                    let offset = frame.read_i16();
                    let v = frame.pop().as_int();
                    if v < 0 {
                        self.branch(instruction_pc, offset);
                    }
                }
                IFGE => {
                    let offset = frame.read_i16();
                    let v = frame.pop().as_int();
                    if v >= 0 {
                        self.branch(instruction_pc, offset);
                    }
                }
                IFGT => {
                    let offset = frame.read_i16();
                    let v = frame.pop().as_int();
                    if v > 0 {
                        self.branch(instruction_pc, offset);
                    }
                }
                IFLE => {
                    let offset = frame.read_i16();
                    let v = frame.pop().as_int();
                    if v <= 0 {
                        self.branch(instruction_pc, offset);
                    }
                }

                IF_ICMPEQ => {
                    //  pop b, pop a, branch if a == b
                    let offset = frame.read_i16();
                    let b = frame.pop().as_int();
                    let a = frame.pop().as_int();
                    if a == b {
                        self.branch(instruction_pc, offset);
                    }
                }
                IF_ICMPNE => {
                    let offset = frame.read_i16();
                    let b = frame.pop().as_int();
                    let a = frame.pop().as_int();
                    if a != b {
                        self.branch(instruction_pc, offset);
                    }
                }
                IF_ICMPLT => {
                    let offset = frame.read_i16();
                    let b = frame.pop().as_int();
                    let a = frame.pop().as_int();
                    if a < b {
                        self.branch(instruction_pc, offset);
                    }
                }
                IF_ICMPGE => {
                    let offset = frame.read_i16();
                    let b = frame.pop().as_int();
                    let a = frame.pop().as_int();
                    if a >= b {
                        self.branch(instruction_pc, offset);
                    }
                }

                IFNULL => {
                    let offset = frame.read_i16();
                    let v = frame.pop();
                    if v.is_null() {
                        self.branch(instruction_pc, offset);
                    }
                }

                IFNONNULL => {
                    let offset = frame.read_i16();
                    let v = frame.pop();
                    if !v.is_null() {
                        self.branch(instruction_pc, offset);
                    }
                }

                // OBJECT CREATION

                // alloc a new object of a give type
                // GC INTERACTION:
                // 1. safepoint before alloc (gc might be waiting)
                // 2. alloc returns `NeedMinorGc`, caller triggers GC and retries
                // 3. push result as reference, now tracked by shadow stack
                NEW => {
                    let type_idx = frame.read_i16() as usize;
                    let type_desc = self.type_table[type_idx];

                    // safepoint before allocation, allocation is a natural poll point
                    drop(frame);
                    self.mutator.safepoint();

                    // alloc with retur loop
                    let obj_ptr = loop {
                        match self.mutator.alloc(type_desc) {
                            AllocResult::Ok(ptr) => {
                                break ptr.as_ptr();
                            }
                            AllocResult::NeedMinorGC => {
                                // TODO: `self.collector.collect_minor()`,wired by caller
                                // For now: signal via ExecResult (caller handles)
                                // In real implementation the thread runner owns collector
                                todo!("hook up collector reference here");
                            }
                            AllocResult::NeedMajorGC => {
                                todo!("hook up major GC");
                            }
                            AllocResult::OutOfMemory => {
                                return ExecResult::OutOfMemory;
                            }
                        }
                    };
                    // push the new object reference onto the operand stack
                    // it's now tracked by the shadow stack via operand_stack slots
                    self.call_stack
                        .last_mut()
                        .unwrap()
                        .push(Value::Reference(obj_ptr));
                }
                // FIELD ACCESS

                // GETFIELD: pop object ref, read a field, push the value.
                // PUTFIELD: pop value, pop object ref, write the field.
                //
                // GC INTERACTION for PUTFIELD:
                //   write_barrier() MUST be called before the write.
                //   If the object is in old gen and the value is in young gen,
                //   the card table entry for the object's card gets dirtied.
                //   Without this, minor GC misses the cross-gen pointer.
                GETFIELD => {
                    let field_offset = frame.read_u16() as usize;
                    let obj_ref = frame.pop().as_reference();

                    if obj_ref.is_null() {
                        return ExecResult::Exception("NullPointerException".into());
                    }

                    unsafe {
                        // object_start() + field_offset = field address
                        let field_ptr = (*obj_ref).object_start().add(field_offset);
                        // fields are stored as raw Value, read based on expected type
                        // For simplicity: read as i32 (for int fields)
                        // Real implementation: type-directed read from field descriptor
                        let val = (field_ptr as *const i32).read();
                        frame.push(Value::Int(val));
                    }
                }

                PUTFIELD => {
                    let field_offset = frame.read_u16() as usize;
                    let new_value = frame.pop(); // the value to write
                    let obj_ref = frame.pop().as_reference(); // the object to write into

                    if obj_ref.is_null() {
                        return ExecResult::Exception("NullPointerException".into());
                    }

                    match new_value {
                        Value::Reference(new_ref) => {
                            // WRITE BARRIER: mandatory for reference field writes
                            // This dirties the card if obj is old-gen and new_ref is young-gen
                            self.mutator.write_barrier(obj_ref, field_offset, new_ref);
                            // write_barrier performs the actual field write internally
                        }
                        Value::Int(v) => {
                            // Primitive write, no write barrier needed, GC doesn't care
                            unsafe {
                                let field_ptr =
                                    (*obj_ref).object_start().add(field_offset) as *mut i32;
                                field_ptr.write(v);
                            }
                        }
                        _ => {
                            todo!("other primitive field types");
                        }
                    }
                }

                // Static field access
                // Static fields live outside the GC heap in your statics table.
                // GETSTATIC: load from statics[index]
                // PUTSTATIC: store to statics[index]
                //
                // GC INTERACTION:
                //   Statics that hold references are registered as global roots
                //   via mutator.register_global() at class load time
                //   No write barrier for statics, they're always treated as roots,
                //   so the marker finds them directly.
                GETSTATIC => {
                    let index = frame.read_u16() as usize;
                    let val = self.statics[index];
                    frame.push(val);
                }
                PUTSTATIC => {
                    let index = frame.read_u16() as usize;
                    let val = frame.pop();
                    self.statics[index] = val;
                    // static holds a reference: no card table needed (it's a root)
                    // but if you have a separate static area in the GC heap,
                    // you'd treat it like old-gen and dirty cards accordingly
                }

                // METHOD INVOCATION
                // INVOKEVIRTUAL / INVOKESPECIAL: call an instance method.
                // INVOKESTATIC: call a static method.
                //
                // GC INTERACTION:
                //   1. Safepoint BEFORE pushing the new frame, GC needs stable roots.
                //   2. After the recursive execute() call returns, pop roots.
                //   3. Push return value onto current operand stack.
                //
                // For now: method resolution is a todo stub.
                // The method index maps to a (bytecode, max_locals, max_stack) triple.
                INVOKEVIRTUAL | INVOKESPECIAL | INVOKESTATIC => {
                    let method_index = frame.read_u16() as usize;
                    let num_args = frame.read_u8() as usize; // non-standard: we encode this

                    // collect args from operand stack (top of stack = last arg)
                    let mut args = Vec::with_capacity(num_args);
                    for _ in 0..num_args {
                        args.push(frame.pop());
                    }
                    args.reverse(); // stack gives them LIFO, method wants FIFO

                    // safepoint at method call boundary, natural poll point
                    drop(frame);
                    self.mutator.safepoint();

                    // resolve and invoke the method recursively
                    // TODO: look up method_index → bytecode + metadata
                    let result = self.invoke_method(method_index, args);

                    match result {
                        ExecResult::ReturnValue(v) => {
                            self.call_stack.last_mut().unwrap().push(v);
                        }
                        ExecResult::ReturnVoid => { /* nothing to push */ }
                        other => return other, // exception / OOM propagates up
                    }
                }

                // ── Returns ───────────────────────────────────────────────────
                // Signal the end of this method invocation.
                // The execute() function handles pop_frame() and call_stack.pop().
                RETURN => {
                    return ExecResult::ReturnVoid;
                }
                IRETURN => {
                    let v = self.call_stack.last_mut().unwrap().pop();
                    return ExecResult::ReturnValue(v);
                }
                ARETURN => {
                    // return a reference, the caller's frame receives it
                    let v = self.call_stack.last_mut().unwrap().pop();
                    return ExecResult::ReturnValue(v);
                }

                // ── Exceptions ────────────────────────────────────────────────
                ATHROW => {
                    let obj = self.call_stack.last_mut().unwrap().pop();
                    // TODO: exception dispatch, walk call stack looking for handler
                    return ExecResult::Exception(format!("throw {:?}", obj));
                }
                _ => {
                    return ExecResult::Exception(format!(
                        "unsupported opcode: 0x{:02x} at PC={}",
                        opcode, instruction_pc
                    ));
                }
            }
        }
    }

    fn invoke_method(&mut self, method_index: usize, args: Vec<Value>) -> ExecResult {
        todo!()
    }

    fn branch(&mut self, instruction_pc: usize, offset: i16) {
        todo!()
    }
}
