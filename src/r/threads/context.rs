//! x86-64 System V stack continuations. A suspended stack stays pinned and
//! must only be resumed on its original carrier. No Rust reference into its
//! active frames may be used by the parent while the continuation is running.

pub(super) use super::stack::Stack;

core::arch::global_asm!(
    ".global trueos_thread_context_swap",
    ".type trueos_thread_context_swap,@function",
    "trueos_thread_context_swap:",
    "push rbp",
    "push rbx",
    "push r12",
    "push r13",
    "push r14",
    "push r15",
    "sub rsp, 8",
    "stmxcsr [rsp]",
    "fnstcw [rsp + 4]",
    "mov [rdi], rsp",
    "mov rsp, [rsi]",
    "ldmxcsr [rsp]",
    "fldcw [rsp + 4]",
    "add rsp, 8",
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop rbx",
    "pop rbp",
    "ret",
    ".size trueos_thread_context_swap, .-trueos_thread_context_swap",
    ".global trueos_thread_context_start",
    ".type trueos_thread_context_start,@function",
    "trueos_thread_context_start:",
    "mov rdi, r12",
    "sub rsp, 8",
    "call r13",
    "ud2",
    ".size trueos_thread_context_start, .-trueos_thread_context_start",
);

unsafe extern "C" {
    fn trueos_thread_context_swap(from: *mut usize, to: *const usize);
    fn trueos_thread_context_start();
}

impl Stack {
    pub(super) unsafe fn initialize(
        &mut self,
        data: *mut (),
        entry: unsafe extern "C" fn(*mut ()) -> !,
    ) -> usize {
        // After ret, RSP is 8 modulo 16, as required on SysV function entry.
        let top = unsafe { self.ptr().as_ptr().add(self.len() & !15).sub(8) };
        let frame = unsafe { top.sub(64).cast::<usize>() };
        unsafe {
            frame.add(0).write(0x037f_0000_1f80); // x87 control word, MXCSR
            frame.add(1).write(0); // r15
            frame.add(2).write(0); // r14
            frame.add(3).write(entry as usize); // r13
            frame.add(4).write(data as usize); // r12
            frame.add(5).write(0); // rbx
            frame.add(6).write(0); // rbp
            frame
                .add(7)
                .write(trueos_thread_context_start as *const () as usize);
        }
        frame as usize
    }
}

/// Caller guarantees both stacks remain valid and exclusively owned until
/// control returns, and calls occur on the same carrier with no held lock.
pub(super) unsafe fn swap(from: *mut usize, to: *const usize) {
    unsafe { trueos_thread_context_swap(from, to) }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct State {
        parent: usize,
        child: usize,
        stage: usize,
        address: usize,
    }

    unsafe extern "C" fn entry(data: *mut ()) -> ! {
        let state = data.cast::<State>();
        let mut retained = [17usize; 128];
        unsafe {
            (*state).address = retained.as_ptr() as usize;
            (*state).stage = 1;
            swap(&mut (*state).child, &(*state).parent);
            assert_eq!((*state).stage, 2);
            retained[31] += 5;
            assert_eq!(retained[31], 22);
            (*state).stage = 3;
            swap(&mut (*state).child, &(*state).parent);
        }
        panic!("completed continuation resumed")
    }

    #[test]
    fn retains_independent_frames_across_multiple_resumes() {
        let mut stack = Stack::new(256 * 1024).unwrap();
        let mut state = State {
            parent: 0,
            child: 0,
            stage: 0,
            address: 0,
        };
        state.child = unsafe { stack.initialize((&mut state as *mut State).cast(), entry) };
        unsafe { swap(&mut state.parent, &state.child) };
        assert_eq!(state.stage, 1);
        assert!(state.address >= stack.ptr().as_ptr() as usize);
        assert!(state.address < stack.ptr().as_ptr() as usize + stack.len());
        state.stage = 2;
        unsafe { swap(&mut state.parent, &state.child) };
        assert_eq!(state.stage, 3);
    }
}
