use crate::{interpreter::value::Value, object::header::GcHeader};

pub struct Frame {
    /// the bytecode being executed, slice into the method code buffer
    pub bytecode: Vec<u8>,

    /// program counter
    pub pc: usize,

    /// local variable array
    /// Slot 0 = `this` for instance methods (Reference)
    /// Slots 1..n = method parameters then declared locals
    /// size is fixed per method
    pub locals: Vec<Value>,

    /// values pushed/popped while instructions execution
    pub operand_stack: Vec<Value>,

    /// name of stack trace purposes
    pub method_name: &'static str,

    /// owner class of this method frame
    pub class_name: String,
}

impl Frame {
    pub fn new(
        bytecode: Vec<u8>,
        max_locals: usize,
        max_stack: usize,
        method_name: &'static str,
        class_name: String,
    ) -> Self {
        Self {
            bytecode,
            pc: 0,
            locals: vec![Value::Void; max_locals],
            operand_stack: Vec::with_capacity(max_stack),
            method_name,
            class_name,
        }
    }

    /// read one byte and inc
    pub fn read_u8(&mut self) -> u8 {
        let b = self.bytecode[self.pc];
        self.pc += 1;
        b
    }

    /// read big endian signed u16 (most JVM operands are 2 bytes)
    pub fn read_u16(&mut self) -> u16 {
        let hi = self.bytecode[self.pc] as u16;
        let lo = self.bytecode[self.pc + 1] as u16;
        self.pc += 2;

        (hi << 8) | lo
    }
    /// big-endian signed i16
    pub fn read_i16(&mut self) -> i16 {
        self.read_u16() as i16
    }

    /// apply a offset to PC
    /// offset is relative to the start of the branch instruction,
    /// not the current PC (which is already past the operand)
    /// pc = instruction_start + offset
    pub fn jump(&mut self, instruction_part: usize, offset: i16) {
        self.pc = (instruction_part as isize + offset as isize) as usize;
    }

    // operand stack....

    pub fn push(&mut self, value: Value) {
        self.operand_stack.push(value);
    }

    pub fn pop(&mut self) -> Value {
        self.operand_stack.pop().expect("operand stack underflow")
    }

    pub fn peek(&self) -> &Value {
        self.operand_stack.last().expect("operand stack empty")
    }

    /// duplicate top of stack, DUP opcode
    pub fn dup(&mut self) {
        let top = *self.peek();
        self.operand_stack.push(top);
    }

    // local vars

    pub fn load_local(&self, index: usize) -> Value {
        self.locals[index]
    }

    pub fn store_local(&mut self, index: usize, val: Value) {
        self.locals[index] = val;
    }

    // root extraction for gc

    /// collects all ref slots in this frame for the shadow stack
    /// called by the interpreter when pushing a `StackFrame` to `RootRegistry`
    ///
    /// the returned pointers are into self.locals and self.operand_stack
    /// the `Frame` must outlive the `StackFrame` registration
    pub fn reference_slots(&mut self) -> Vec<*mut *mut GcHeader> {
        let mut slots = Vec::new();

        for val in self.locals.iter_mut() {
            if let Value::Reference(ptr) = val {
                slots.push(ptr as *mut *mut GcHeader);
            }
        }

        for val in self.operand_stack.iter_mut() {
            if let Value::Reference(ptr) = val {
                slots.push(ptr as *mut *mut GcHeader);
            }
        }

        slots
    }
}
