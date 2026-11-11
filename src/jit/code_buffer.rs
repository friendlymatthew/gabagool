use core::ptr::NonNull;

use crate::{bail_jit, ensure_jit, prelude::*, Result};

use crate::jit::code_layout::CodeLayout;

unsafe extern "C" {
    fn pthread_jit_write_protect_np(enabled: libc::c_int);
    fn sys_icache_invalidate(start: *mut libc::c_void, len: usize);
}

pub(super) struct CodeBuffer {
    ptr: NonNull<u8>,
    allocation_len: usize,
}

impl CodeBuffer {
    pub(super) fn new(layout: &CodeLayout) -> Result<Self> {
        ensure_jit!(layout.code_len > 0, "cannot execute empty code");
        ensure_jit!(layout.code_len <= layout.bytes.len(), "invalid code length");

        let memory = unsafe {
            libc::mmap(
                core::ptr::null_mut(),
                layout.bytes.len(),
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_JIT,
                -1,
                0,
            )
        };

        ensure_jit!(memory != libc::MAP_FAILED, "failed to allocate JIT memory");

        let Some(ptr) = NonNull::new(memory.cast::<u8>()) else {
            unsafe { libc::munmap(memory, layout.bytes.len()) };
            bail_jit!("JIT allocation returned a null address");
        };

        let buffer = Self {
            ptr,
            allocation_len: layout.bytes.len(),
        };

        unsafe { pthread_jit_write_protect_np(0) };
        let write_guard = WriteGuard;

        unsafe {
            core::ptr::copy_nonoverlapping(
                layout.bytes.as_ptr(),
                ptr.as_ptr(),
                buffer.allocation_len,
            );
            let writable = core::slice::from_raw_parts_mut(ptr.as_ptr(), buffer.allocation_len);
            layout.resolve_data_slot_addresses(writable)?;
            sys_icache_invalidate(memory, layout.code_len);
        }

        drop(write_guard);
        Ok(buffer)
    }

    /// The pointer is valid only while this buffer remains alive.
    pub(super) const fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }
}

struct WriteGuard;

impl Drop for WriteGuard {
    fn drop(&mut self) {
        unsafe { pthread_jit_write_protect_np(1) };
    }
}

impl Drop for CodeBuffer {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.ptr.as_ptr().cast(), self.allocation_len) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::{StencilInstance, STENCILS};

    #[test]
    fn adds_values_on_memory_stack() {
        let find = |name| STENCILS.iter().find(|s| s.name == name).unwrap();

        let instances = [
            StencilInstance {
                stencil: find("_i32_add"),
                parameters: vec![],
            },
            StencilInstance {
                stencil: find("_return_value"),
                parameters: vec![],
            },
        ];

        let layout = CodeLayout::try_new(&instances).unwrap();
        let buffer = CodeBuffer::new(&layout).unwrap();

        #[repr(C)]
        struct VMContext {
            locals: *mut u64,
            stack: *mut u64,
            stack_height: u64,
            trap: u64,
        }

        let mut stack = [30u64, 37];

        let mut ctx = VMContext {
            locals: core::ptr::null_mut(),
            stack: stack.as_mut_ptr(),
            stack_height: 2,
            trap: 0,
        };

        type Entry = unsafe extern "C" fn(*mut VMContext);

        unsafe {
            let entry: Entry = core::mem::transmute(buffer.as_ptr());
            entry(&mut ctx);
        }

        assert_eq!(ctx.stack_height, 1);
        assert_eq!(stack[0], 67);
    }

    #[test]
    fn local_get2_adds_on_memory_stack() {
        let find = |name| {
            STENCILS
                .iter()
                .find(|stencil| stencil.name == name)
                .unwrap()
        };

        let instances = [
            StencilInstance {
                stencil: find("_local_get2"),
                parameters: vec![2, 3],
            },
            StencilInstance {
                stencil: find("_i32_add"),
                parameters: vec![],
            },
            StencilInstance {
                stencil: find("_return_value"),
                parameters: vec![],
            },
        ];

        let layout = CodeLayout::try_new(&instances).unwrap();
        let buffer = CodeBuffer::new(&layout).unwrap();

        #[repr(C)]
        struct VMContext {
            locals: *mut u64,
            stack: *mut u64,
            stack_height: u64,
            trap: u64,
        }

        let mut locals = [999_u64, 888, 20, 22];

        let mut stack = [0_u64; 2];

        let mut context = VMContext {
            locals: locals.as_mut_ptr(),
            stack: stack.as_mut_ptr(),
            stack_height: 0,
            trap: 0,
        };

        type Entry = unsafe extern "C" fn(*mut VMContext);

        // SAFETY: indices are in bounds, two pushes fit, and the entry ABI
        // matches shared.h.
        unsafe {
            let entry: Entry = core::mem::transmute(buffer.as_ptr());

            entry(&mut context);
        }

        assert_eq!(context.stack_height, 1);
        assert_eq!(stack[0], 42);
    }
}
