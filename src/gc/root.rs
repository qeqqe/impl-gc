use crate::object::header::GcHeader;

///  Single frame on the interpreter's shadow stack.
/// Each slot is a pointer to a pointer, the outer ptr is the location
/// of a reference slot in the interpreter frame, the inner ptr is the
/// GC object it currently holds
pub struct StackFrame {
    pub slots: Vec<*mut *mut GcHeader>,
}

pub struct RootRegistry {
    /// outer: root slot location, inner: current heap ptr
    globals: Vec<*mut *mut GcHeader>,
    /// interpreter's call stack
    shadow_stack: Vec<StackFrame>,
}

impl RootRegistry {
    pub fn new() -> Self {
        Self {
            globals: Vec::new(),
            shadow_stack: Vec::new(),
        }
    }

    /// register a global/static root slot
    /// `root` is the address of the slot that holds the GC pointer not the GC pointer itself
    pub fn register_global(&mut self, root: *mut *mut GcHeader) {
        debug_assert!(!root.is_null());
        self.globals.push(root);
    }

    /// unregister a global root (e.g. class unloaded, global cleared)
    pub fn unregister_global(&mut self, root: *mut *mut GcHeader) {
        self.globals.retain(|&r| r != root);
    }

    /// called by the interpreter when entering a new call frame,
    /// `frame` lists which slots in that frame hold GC references
    pub fn push_frame(&mut self, frame: StackFrame) {
        self.shadow_stack.push(frame);
    }

    /// called by the interpreter when returning from a frame
    pub fn pop_frame(&mut self) {
        self.shadow_stack.pop();
    }

    /// iterate all roots, used exclusively by the mark phase
    /// Yields the actual `*mut GcHeader` values (dereferenced from the slots)
    pub fn iter_roots(&self) -> impl Iterator<Item = *mut GcHeader> + '_ {
        let globals = self.globals.iter().filter_map(|&slot| {
            let ptr = unsafe { *slot };
            if ptr.is_null() { None } else { Some(ptr) }
        });

        let frame_slots = self.shadow_stack.iter().flat_map(|frame| {
            frame.slots.iter().filter_map(|&slot| {
                let ptr = unsafe { *slot };
                if ptr.is_null() { None } else { Some(ptr) }
            })
        });

        globals.chain(frame_slots)
    }
}
