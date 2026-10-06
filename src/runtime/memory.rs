use crate::module::MemoryType;
use std::ffi::CString;
use std::io::{self, ErrorKind};
use std::ptr;

#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
struct MmapBacking {
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

#[derive(Debug)]
pub struct MemoryInstance {
    pub memory_type: MemoryType,
    pub data: GuestMemory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestMemory {
    backing: Backing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Backing {
    Owned(Vec<u8>),
    #[cfg(unix)]
    Mmap(MmapBacking),
}

impl GuestMemory {
    pub fn new(size: usize) -> Self {
        Self {
            backing: Backing::Owned(vec![0u8; size]),
        }
    }

    pub const fn from_vec(bytes: Vec<u8>) -> Self {
        Self {
            backing: Backing::Owned(bytes),
        }
    }

    /// create an mmap-backed linear memory
    ///
    /// initial is the logical size in bytes
    /// capacity is the maximum size the memory can ever reach via resize
    #[cfg(unix)]
    pub fn with_mmap(initial: usize, capacity: usize) -> std::io::Result<Self> {
        Ok(Self {
            backing: Backing::Mmap(MmapBacking::new(initial, capacity)?),
        })
    }

    const fn slice(&self) -> &[u8] {
        match &self.backing {
            Backing::Owned(v) => v.as_slice(),
            #[cfg(unix)]
            Backing::Mmap(m) => m.as_slice(),
        }
    }

    const fn slice_mut(&mut self) -> &mut [u8] {
        match &mut self.backing {
            Backing::Owned(v) => v.as_mut_slice(),
            #[cfg(unix)]
            Backing::Mmap(m) => m.as_mut_slice(),
        }
    }

    pub const fn len(&self) -> usize {
        match &self.backing {
            Backing::Owned(v) => v.len(),
            #[cfg(unix)]
            Backing::Mmap(m) => m.len(),
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn read_u8(&self, ptr: usize) -> u8 {
        self.slice()[ptr]
    }

    pub fn read_u16(&self, ptr: usize) -> u16 {
        u16::from_le_bytes(self.slice()[ptr..ptr + 2].try_into().unwrap())
    }

    pub fn read_u32(&self, ptr: usize) -> u32 {
        u32::from_le_bytes(self.slice()[ptr..ptr + 4].try_into().unwrap())
    }

    pub fn read_u64(&self, ptr: usize) -> u64 {
        u64::from_le_bytes(self.slice()[ptr..ptr + 8].try_into().unwrap())
    }

    pub fn read_bytes(&self, ptr: usize, len: usize) -> &[u8] {
        &self.slice()[ptr..ptr + len]
    }

    pub fn write_u8(&mut self, ptr: usize, val: u8) {
        self.slice_mut()[ptr] = val;
    }

    pub fn write_u16(&mut self, ptr: usize, val: u16) {
        self.slice_mut()[ptr..ptr + 2].copy_from_slice(&val.to_le_bytes());
    }

    pub fn write_u32(&mut self, ptr: usize, val: u32) {
        self.slice_mut()[ptr..ptr + 4].copy_from_slice(&val.to_le_bytes());
    }

    pub fn write_u64(&mut self, ptr: usize, val: u64) {
        self.slice_mut()[ptr..ptr + 8].copy_from_slice(&val.to_le_bytes());
    }

    pub fn write_bytes(&mut self, ptr: usize, data: &[u8]) {
        self.slice_mut()[ptr..ptr + data.len()].copy_from_slice(data);
    }

    pub fn fill(&mut self, ptr: usize, len: usize, val: u8) {
        self.slice_mut()[ptr..ptr + len].fill(val);
    }

    pub fn resize(&mut self, new_len: usize, val: u8) {
        match &mut self.backing {
            Backing::Owned(v) => v.resize(new_len, val),
            #[cfg(unix)]
            Backing::Mmap(m) => m.resize(new_len, val),
        }
    }

    pub fn read_fixed<const N: usize>(&self, ptr: usize) -> [u8; N] {
        self.slice()[ptr..ptr + N].try_into().unwrap()
    }

    pub fn copy_within(&mut self, src_start: usize, src_end: usize, dest: usize) {
        self.slice_mut().copy_within(src_start..src_end, dest);
    }

    pub const fn as_slice(&self) -> &[u8] {
        self.slice()
    }

    pub const fn is_mmap(&self) -> bool {
        match &self.backing {
            Backing::Owned(_) => false,
            #[cfg(unix)]
            Backing::Mmap(_) => true,
        }
    }

    #[cfg(unix)]
    pub fn fork_private(&self, n: usize) -> std::io::Result<Vec<Self>> {
        match &self.backing {
            Backing::Mmap(m) => Ok(m
                .fork_private(n)?
                .into_iter()
                .map(|m| Self {
                    backing: Backing::Mmap(m),
                })
                .collect::<Vec<_>>()),
            Backing::Owned(_) => Err(io::Error::new(
                ErrorKind::InvalidInput,
                "fork_private requires mmap-backed GuestMemory; use with_mmap",
            )),
        }
    }
}

#[cfg(all(test, unix))]
mod mmap_tests {
    use super::*;

    const PAGE: usize = 64 * 1024;

    #[test]
    fn basic_read_write() {
        let mut mem = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        mem.write_u32(0, 0xDEAD_BEEF);
        mem.write_u8(100, 42);
        mem.write_u64(200, 0x0123_4567_89AB_CDEF);

        assert_eq!(mem.read_u32(0), 0xDEAD_BEEF);
        assert_eq!(mem.read_u8(100), 42);
        assert_eq!(mem.read_u64(200), 0x0123_4567_89AB_CDEF);
    }

    #[test]
    fn fresh_memory_is_zero() {
        let mem = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        assert!(mem.as_slice().iter().all(|&b| b == 0));
    }

    #[test]
    fn resize_grow_zeros_new_region() {
        let mut mem = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        mem.write_u32(100, 0xCAFE_BABE);

        mem.resize(2 * PAGE, 0);
        assert_eq!(mem.len(), 2 * PAGE);
        assert_eq!(mem.read_u32(100), 0xCAFE_BABE);
        assert_eq!(mem.read_u8(PAGE), 0);
        assert_eq!(mem.read_u8(2 * PAGE - 1), 0);
    }

    #[test]
    fn resize_grow_with_nonzero_val() {
        let mut mem = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        mem.resize(2 * PAGE, 0xAA);
        assert_eq!(mem.read_u8(PAGE), 0xAA);
        assert_eq!(mem.read_u8(2 * PAGE - 1), 0xAA);
    }

    #[test]
    fn read_write_bytes_and_fill() {
        let mut mem = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        mem.write_bytes(50, &[1, 2, 3, 4, 5]);
        assert_eq!(mem.read_bytes(50, 5), &[1, 2, 3, 4, 5]);

        mem.fill(50, 5, 0xFF);
        assert_eq!(mem.read_bytes(50, 5), &[0xFF; 5]);
    }

    #[test]
    fn copy_within_works() {
        let mut mem = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        mem.write_bytes(0, &[1, 2, 3, 4, 5]);
        mem.copy_within(0, 5, 100);
        assert_eq!(mem.read_bytes(100, 5), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn clone_is_independent() {
        let mut mem = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        mem.write_u32(0, 0xAAAA_AAAA);
        let cloned = mem.clone();

        mem.write_u32(0, 0xBBBB_BBBB);
        assert_eq!(mem.read_u32(0), 0xBBBB_BBBB);
        assert_eq!(cloned.read_u32(0), 0xAAAA_AAAA);
    }

    #[test]
    #[should_panic]
    fn resize_beyond_capacity_panics() {
        let mut mem = GuestMemory::with_mmap(PAGE, 4 * PAGE).unwrap();
        mem.resize(8 * PAGE, 0);
    }

    #[test]
    fn many_allocations_no_fd_leak() {
        for _ in 0..1024 {
            let _mem = GuestMemory::with_mmap(PAGE, PAGE).unwrap();
        }
    }

    #[test]
    fn fork_children_see_parent_state() {
        let mut parent = GuestMemory::with_mmap(4 * PAGE, 16 * PAGE).unwrap();
        parent.write_u32(0, 0x1111_1111);
        parent.write_u32(PAGE, 0x2222_2222);
        parent.write_u32(3 * PAGE - 8, 0x3333_3333);

        let children = parent.fork_private(4).unwrap();

        for child in &children {
            assert_eq!(child.read_u32(0), 0x1111_1111);
            assert_eq!(child.read_u32(PAGE), 0x2222_2222);
            assert_eq!(child.read_u32(3 * PAGE - 8), 0x3333_3333);
            assert_eq!(child.len(), parent.len());
        }
    }

    #[test]
    fn fork_children_are_isolated_from_each_other() {
        let mut parent = GuestMemory::with_mmap(4 * PAGE, 16 * PAGE).unwrap();
        parent.write_u32(0, 0xAAAA_AAAA);

        let mut children = parent.fork_private(3).unwrap();

        children[0].write_u32(0, 0xBBBB_BBBB);
        children[1].write_u32(0, 0xCCCC_CCCC);
        // children[2] does not write

        assert_eq!(children[0].read_u32(0), 0xBBBB_BBBB);
        assert_eq!(children[1].read_u32(0), 0xCCCC_CCCC);
        assert_eq!(children[2].read_u32(0), 0xAAAA_AAAA);
    }

    #[test]
    fn fork_child_writes_do_not_leak_to_parent() {
        let mut parent = GuestMemory::with_mmap(4 * PAGE, 16 * PAGE).unwrap();
        parent.write_u32(0, 0xAAAA_AAAA);
        parent.write_u32(PAGE, 0xBBBB_BBBB);

        let mut children = parent.fork_private(2).unwrap();

        children[0].write_u32(0, 0xDEAD_BEEF);
        children[1].write_u32(PAGE, 0xCAFE_BABE);

        // parent unchanged after children's writes
        assert_eq!(parent.read_u32(0), 0xAAAA_AAAA);
        assert_eq!(parent.read_u32(PAGE), 0xBBBB_BBBB);
    }

    #[test]
    fn fork_child_can_resize_independently() {
        let mut parent = GuestMemory::with_mmap(2 * PAGE, 16 * PAGE).unwrap();
        parent.write_u32(0, 0x1234_5678);

        let mut children = parent.fork_private(2).unwrap();

        children[0].resize(4 * PAGE, 0);
        // children[1] keeps original len

        assert_eq!(children[0].len(), 4 * PAGE);
        assert_eq!(children[1].len(), 2 * PAGE);
        assert_eq!(children[0].read_u32(0), 0x1234_5678);
        assert_eq!(children[0].read_u8(3 * PAGE), 0);
        assert_eq!(parent.len(), 2 * PAGE);
    }

    #[test]
    fn fork_zero_children_is_empty() {
        let parent = GuestMemory::with_mmap(PAGE, 16 * PAGE).unwrap();
        let children = parent.fork_private(0).unwrap();
        assert_eq!(children.len(), 0);
    }

    #[test]
    fn fork_owned_memory_errors() {
        let parent = GuestMemory::new(PAGE);
        let err = parent.fork_private(2).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}
