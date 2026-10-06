use std::ffi::CString;
use std::io;
use std::ptr;

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
pub struct MmapBacking {
    fd: libc::c_int,
    ptr: *mut u8,
    len: usize,
    capacity: usize,
}

unsafe impl Send for MmapBacking {}
unsafe impl Sync for MmapBacking {}

impl MmapBacking {
    pub fn new(initial: usize, capacity: usize) -> io::Result<Self> {
        assert!(initial <= capacity);

        let fd = open_backing_fd()?;

        if unsafe { libc::ftruncate(fd, capacity as libc::off_t) } != 0 {
            let err = io::Error::last_os_error();
            unsafe {
                libc::close(fd);
            }
            return Err(err);
        }

        let raw = unsafe {
            libc::mmap(
                ptr::null_mut(),
                capacity,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };

        if raw == libc::MAP_FAILED {
            let err = io::Error::last_os_error();
            unsafe {
                libc::close(fd);
            }
            return Err(err);
        }

        Ok(Self {
            fd,
            ptr: raw.cast::<u8>(),
            len: initial,
            capacity,
        })
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn _capacity(&self) -> usize {
        self.capacity
    }

    pub const fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    pub const fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    pub fn resize(&mut self, new_len: usize, val: u8) {
        assert!(new_len <= self.capacity,);

        if new_len > self.len {
            let added = new_len - self.len;
            unsafe {
                let added_ptr = self.ptr.add(self.len);
                std::slice::from_raw_parts_mut(added_ptr, added).fill(val);
            }
        }

        self.len = new_len;
    }

    pub fn fork_private(&self, n: usize) -> io::Result<Vec<Self>> {
        let mut children = Vec::with_capacity(n);

        for _ in 0..n {
            let fd = unsafe { libc::dup(self.fd) };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }

            let raw = unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    self.capacity,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE,
                    fd,
                    0,
                )
            };

            if raw == libc::MAP_FAILED {
                let err = io::Error::last_os_error();
                unsafe {
                    libc::close(fd);
                }
                return Err(err);
            }

            children.push(Self {
                fd,
                ptr: raw.cast::<u8>(),
                len: self.len,
                capacity: self.capacity,
            });
        }

        Ok(children)
    }
}

impl Drop for MmapBacking {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr.cast::<libc::c_void>(), self.capacity);
            libc::close(self.fd);
        }
    }
}

impl Clone for MmapBacking {
    fn clone(&self) -> Self {
        let mut new =
            Self::new(self.len, self.capacity).expect("clone of mmap-backed memory failed");

        new.as_mut_slice().copy_from_slice(self.as_slice());

        new
    }
}

impl PartialEq for MmapBacking {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.as_slice() == other.as_slice()
    }
}

impl Eq for MmapBacking {}

#[cfg(target_os = "linux")]
fn open_backing_fd() -> io::Result<libc::c_int> {
    let name = CString::new("gabagool-memory").unwrap();
    let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };

    if fd < 0 {
        return Err(io::Error::last_os_error());
    }

    Ok(fd)
}

#[cfg(target_os = "macos")]
fn open_backing_fd() -> io::Result<libc::c_int> {
    // note: we use a tempfile rather than shm_open

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let pid = unsafe { libc::getpid() };
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = CString::new(format!("/tmp/gabagool.{pid}.{n}")).unwrap();

    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC,
            0o600,
        )
    };

    if fd < 0 {
        return Err(io::Error::last_os_error());
    }

    let unlink_rc = unsafe { libc::unlink(path.as_ptr()) };

    if unlink_rc != 0 {
        let err = io::Error::last_os_error();
        unsafe {
            libc::close(fd);
        }
        return Err(err);
    }

    Ok(fd)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn open_backing_fd() -> io::Result<libc::c_int> {
    unimplemented!()
}
