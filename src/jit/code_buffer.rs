use std::ptr::{self, NonNull};

extern "C" {
    fn sys_icache_invalidate(start: *mut libc::c_void, size: libc::size_t);
    fn pthread_jit_write_protect_np(enabled: libc::c_int);
}

#[derive(Debug)]
pub struct CodeBuffer {
    ptr: NonNull<u8>,
    len: usize,
    offset: usize,
}

impl CodeBuffer {
    pub fn new(size: usize) -> Self {
        let raw = unsafe {
            libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_JIT,
                -1,
                0,
            )
        };

        let ptr = NonNull::new(raw as *mut u8).expect("mmap failed");

        unsafe { pthread_jit_write_protect_np(0) };

        Self {
            ptr,
            len: size,
            offset: 0,
        }
    }

    pub const fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    #[allow(clippy::missing_const_for_fn)]
    pub fn emit(&mut self, stencil_bytes: &[u8]) {
        unsafe {
            ptr::copy_nonoverlapping(
                stencil_bytes.as_ptr(),
                self.ptr.as_ptr().add(self.offset),
                stencil_bytes.len(),
            );
        }

        self.offset += stencil_bytes.len();
    }

    pub fn make_executable(&self) {
        unsafe {
            pthread_jit_write_protect_np(1);
            sys_icache_invalidate(
                self.ptr.as_ptr() as *mut libc::c_void,
                self.len as libc::size_t,
            );
        }
    }
}

impl Drop for CodeBuffer {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr.as_ptr() as *mut libc::c_void, self.len);
        }
    }
}
