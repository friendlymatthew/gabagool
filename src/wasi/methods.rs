use crate::{
    prelude::*,
    wasi::{
        ctx::FDFLAGS_ALL,
        traits::{ClockError, ClockId},
    },
    Caller, ExternalValue, GuestMemory, HostOutcome, Instance, RawValue, Store,
};

use super::{WasiCtx, WasiIOError, WasiResource};

const SUCCESS: i32 = 0;
const AGAIN: i32 = 6;
const BADF: i32 = 8;
const FAULT: i32 = 21;
const INTR: i32 = 27;
const IO: i32 = 29;
const OVERFLOW: i32 = 61;
const NOTCAPABLE: i32 = 76;
const INVAL: i32 = 28;
const NOTSUP: i32 = 58;

macro_rules! wasi_some {
    ($expr:expr, $errno:expr $(,)?) => {
        match $expr {
            Some(value) => value,
            None => return $crate::wasi::methods::errno($errno),
        }
    };
}

macro_rules! wasi_try {
    ($expr:expr, $errno:expr $(,)?) => {
        match $expr {
            Ok(value) => value,
            Err(_) => return $crate::wasi::methods::errno($errno),
        }
    };
}

pub(super) fn fd_filestat_set_size<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let size = args[1].as_i64() as u64;
    let entry = wasi_some!(caller.data_mut().descriptors_mut().get_mut(&fd), BADF);
    if !entry.can_filestat_set_size() {
        return errno(NOTCAPABLE);
    }

    errno(match entry.resource.set_size(size) {
        Ok(()) => SUCCESS,
        Err(WasiIOError::Io) => IO,
        Err(WasiIOError::Interrupted) => INTR,
        Err(WasiIOError::WouldBlock) => AGAIN,
        Err(WasiIOError::Unsupported) => NOTSUP,
    })
}

pub(super) fn fd_filestat_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let addr = args[1].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();
    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    if !entry.can_filestat_get() {
        return errno(NOTCAPABLE);
    }

    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    wasi_try!(memory.read_bytes(addr, 64), FAULT);

    let stat = match entry.resource.filestat() {
        Ok(stat) => stat,
        Err(error) => {
            return errno(match error {
                WasiIOError::Io => IO,
                WasiIOError::Interrupted => INTR,
                WasiIOError::WouldBlock => AGAIN,
                WasiIOError::Unsupported => NOTSUP,
            })
        }
    };

    wasi_try!(memory.write(addr, stat.to_bytes()), FAULT);

    errno(SUCCESS)
}

pub(super) fn fd_fdstat_set_flags<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let flags = args[1].as_i32() as u32;

    if flags & !u32::from(FDFLAGS_ALL) != 0 {
        return errno(INVAL);
    }
    let flags = flags as u16;

    let entry = wasi_some!(caller.data_mut().descriptors_mut().get_mut(&fd), BADF);
    if !entry.can_set_flags() {
        return errno(NOTCAPABLE);
    }

    match entry.resource.set_fdflags(flags) {
        Ok(()) => {
            entry.flags = flags;
            errno(SUCCESS)
        }
        Err(error) => errno(match error {
            WasiIOError::Io => IO,
            WasiIOError::Interrupted => INTR,
            WasiIOError::WouldBlock => AGAIN,
            WasiIOError::Unsupported => NOTSUP,
        }),
    }
}

pub(super) fn fd_datasync<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let entry = wasi_some!(caller.data_mut().descriptors_mut().get_mut(&fd), BADF);
    if !entry.can_datasync() {
        return errno(NOTCAPABLE);
    }

    errno(match entry.resource.datasync() {
        Ok(()) => SUCCESS,
        Err(WasiIOError::Io) => IO,
        Err(WasiIOError::Interrupted) => INTR,
        Err(WasiIOError::WouldBlock) => AGAIN,
        Err(WasiIOError::Unsupported) => NOTSUP,
    })
}

pub(super) fn fd_advise<R: WasiResource>(
    caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let _offset = args[1].as_i64() as u64;
    let _len = args[2].as_i64() as u64;
    let advice = args[3].as_i32() as u32;

    if advice > 5 {
        return errno(INVAL);
    }

    let entry = wasi_some!(caller.data().descriptors().get(&fd), BADF);
    if !entry.can_advise() {
        return errno(NOTCAPABLE);
    }

    errno(SUCCESS)
}

pub(super) fn fd_fdstat_set_rights<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let base_rights = args[1].as_i64() as u64;
    let inherited_rights = args[2].as_i64() as u64;

    let ctx = caller.data_mut();

    let d = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    if (base_rights & !d.rights_base) != 0 || (inherited_rights & !d.rights_inheriting) != 0 {
        return errno(NOTCAPABLE);
    }

    d.rights_base = base_rights;
    d.rights_inheriting = inherited_rights;

    errno(SUCCESS)
}

pub(super) fn clock_res_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let clock_id = args[0].as_i32() as u32;
    let addr = args[1].as_pointer();

    let clock_id = wasi_try!(ClockId::try_from(clock_id), INVAL);

    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let nanos = match ctx.clock_mut().resolution_ns(clock_id) {
        Ok(nanos) => nanos.get(),
        Err(error) => {
            return errno(match error {
                ClockError::Unsupported => INVAL,
                ClockError::Unavailable => IO,
                ClockError::Overflow => OVERFLOW,
            })
        }
    };

    wasi_try!(memory.write(addr, nanos.to_le_bytes()), FAULT);

    errno(SUCCESS)
}

pub(super) fn clock_time_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let clock_id = args[0].as_i32() as u32;
    let precision_nanos = args[1].as_i64() as u64;
    let guest_addr = args[2].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let clock_id = wasi_try!(ClockId::try_from(clock_id), INVAL);

    let nanos = match ctx.clock_mut().now_nanos(clock_id, precision_nanos) {
        Ok(nanos) => nanos,
        Err(error) => {
            return errno(match error {
                ClockError::Unsupported => INVAL,
                ClockError::Unavailable => IO,
                ClockError::Overflow => OVERFLOW,
            })
        }
    };

    wasi_try!(memory.write(guest_addr, nanos.to_le_bytes()), FAULT);

    errno(SUCCESS)
}

pub(super) fn random_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let addr = args[0].as_pointer();
    let len = args[1].as_i32() as u32;

    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let buf = wasi_try!(memory.read_bytes_mut(addr, len as usize), FAULT);

    let rand = ctx.random_mut();
    wasi_try!(rand.fill_bytes(buf), IO);

    errno(SUCCESS)
}

pub(super) fn fd_close<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;

    let ctx = caller.data_mut();

    match ctx.descriptors_mut().remove(&fd) {
        Some(_) => errno(SUCCESS),
        None => errno(BADF),
    }
}

pub fn fd_read<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let iovs_ptr = args[1].as_pointer();
    let iovs_len = args[2].as_i32() as u32 as usize;
    let nread_ptr = args[3].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();

    let file_entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);

    if !file_entry.can_read() {
        return errno(NOTCAPABLE);
    }

    let input = wasi_some!(file_entry.resource.input_mut(), BADF);
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let buffers = match decode_iovecs(memory, iovs_ptr, iovs_len) {
        Ok(buffers) => buffers,
        Err(code) => return errno(code),
    };

    // first check before consuming input
    wasi_try!(memory.read_bytes(nread_ptr, 4), FAULT);

    let mut total_read = 0u32;

    for (ptr, len) in buffers {
        let buffer = wasi_try!(memory.read_bytes_mut(ptr as usize, len as usize), FAULT);

        match input.read(buffer) {
            Ok(n) if n <= buffer.len() => {
                total_read += n as u32;

                if n < buffer.len() {
                    break;
                }
            }
            result => {
                if total_read != 0 {
                    break;
                }

                return errno(match result {
                    Err(WasiIOError::Interrupted) => INTR,
                    Err(WasiIOError::WouldBlock) => AGAIN,
                    _ => IO,
                });
            }
        }
    }

    wasi_try!(memory.write(nread_ptr, total_read.to_le_bytes()), FAULT);

    errno(SUCCESS)
}

pub(super) fn fd_fdstat_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let addr = args[1].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();
    let entry = wasi_some!(ctx.descriptors().get(&fd), BADF);

    let mut buf = [0u8; 24];
    buf[0] = entry.resource.file_type() as u8;
    buf[2..4].copy_from_slice(&entry.flags.to_le_bytes());
    buf[8..16].copy_from_slice(&entry.rights_base.to_le_bytes());
    buf[16..24].copy_from_slice(&entry.rights_inheriting.to_le_bytes());

    let memory = wasi_some!(guest_memory(store, instance), FAULT);
    wasi_try!(memory.write_bytes(addr, &buf), FAULT);

    errno(SUCCESS)
}

pub(super) fn environ_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    string_list_get(
        memory,
        ctx.environment(),
        args[0].as_pointer(),
        args[1].as_i32() as u32,
    )
}

pub(super) fn environ_sizes_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    string_list_sizes_get(
        memory,
        ctx.environment(),
        args[0].as_pointer(),
        args[1].as_pointer(),
    )
}

pub(super) fn args_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    string_list_get(
        memory,
        ctx.args(),
        args[0].as_pointer(),
        args[1].as_i32() as u32,
    )
}

pub(super) fn args_sizes_get<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    string_list_sizes_get(
        memory,
        ctx.args(),
        args[0].as_pointer(),
        args[1].as_pointer(),
    )
}

pub(super) const fn proc_exit<R: WasiResource>(
    _: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    HostOutcome::Exit(args[0].as_i32() as u32)
}

fn string_list_buffer_len(strings: &[String]) -> Option<u32> {
    strings.iter().try_fold(0u32, |total, string| {
        let len = u32::try_from(string.len()).ok()?;
        total.checked_add(len)?.checked_add(1)
    })
}

fn string_list_sizes_get(
    memory: &mut GuestMemory,
    strings: &[String],
    count_ptr: usize,
    buffer_size_ptr: usize,
) -> HostOutcome {
    let count = wasi_try!(u32::try_from(strings.len()), OVERFLOW);
    let buffer_len = wasi_some!(string_list_buffer_len(strings), OVERFLOW);

    wasi_try!(memory.write(count_ptr, count.to_le_bytes()), FAULT);
    wasi_try!(
        memory.write(buffer_size_ptr, buffer_len.to_le_bytes()),
        FAULT
    );

    errno(SUCCESS)
}

fn string_list_get(
    memory: &mut GuestMemory,
    strings: &[String],
    table_ptr: usize,
    mut buffer_ptr: u32,
) -> HostOutcome {
    let table_len = wasi_some!(strings.len().checked_mul(4), OVERFLOW);
    let buffer_len = wasi_some!(string_list_buffer_len(strings), OVERFLOW);

    wasi_try!(memory.read_bytes(table_ptr, table_len), FAULT);
    wasi_try!(
        memory.read_bytes(buffer_ptr as usize, buffer_len as usize),
        FAULT
    );

    wasi_some!(buffer_ptr.checked_add(buffer_len), OVERFLOW);

    for (index, string) in strings.iter().enumerate() {
        wasi_try!(
            memory.write(table_ptr + index * 4, buffer_ptr.to_le_bytes()),
            FAULT
        );
        wasi_try!(
            memory.write_bytes(buffer_ptr as usize, string.as_bytes()),
            FAULT
        );
        let nul_ptr = buffer_ptr + string.len() as u32;
        wasi_try!(memory.write(nul_ptr as usize, [0]), FAULT);
        buffer_ptr = nul_ptr + 1;
    }

    errno(SUCCESS)
}

pub(super) fn fd_write<R: WasiResource>(
    mut caller: Caller<'_, WasiCtx<R>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let iovs_ptr = args[1].as_pointer();
    let iovs_len = args[2].as_pointer();
    let nwritten_ptr = args[3].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let entry = wasi_some!(ctx.descriptors_mut().get_mut(&fd), BADF);
    if !entry.can_write() {
        return errno(NOTCAPABLE);
    }
    let output = wasi_some!(entry.resource.output_mut(), BADF);

    let buffers = match decode_iovecs(memory, iovs_ptr, iovs_len) {
        Ok(buffers) => buffers,
        Err(code) => return errno(code),
    };
    wasi_try!(memory.read_bytes(nwritten_ptr, 4), FAULT);

    let mut written = 0u32;
    for (ptr, len) in buffers {
        let bytes = wasi_try!(memory.read_bytes(ptr as usize, len as usize), FAULT);
        match output.write(bytes) {
            Ok(bytes_written) if bytes_written <= bytes.len() => {
                written += bytes_written as u32;
                if bytes_written < bytes.len() {
                    break;
                }
            }
            result => {
                if written != 0 {
                    break;
                }

                return errno(match result {
                    Err(WasiIOError::Interrupted) => INTR,
                    Err(WasiIOError::WouldBlock) => AGAIN,
                    _ => IO,
                });
            }
        }
    }

    wasi_try!(memory.write(nwritten_ptr, written.to_le_bytes()), FAULT);

    errno(SUCCESS)
}

fn decode_iovecs(
    memory: &GuestMemory,
    table_ptr: usize,
    count: usize,
) -> core::result::Result<Vec<(u32, u32)>, i32> {
    let table_len = count.checked_mul(8).ok_or(FAULT)?;
    let table = memory.read_bytes(table_ptr, table_len).map_err(|_| FAULT)?;
    let mut buffers = Vec::new();
    let mut total = 0u32;

    let (chunks, _) = table.as_chunks::<8>();
    for &[p1, p2, p3, p4, l1, l2, l3, l4] in chunks {
        let ptr = u32::from_le_bytes([p1, p2, p3, p4]);
        let len = u32::from_le_bytes([l1, l2, l3, l4]);
        if len == 0 {
            continue;
        }
        total = total.checked_add(len).ok_or(OVERFLOW)?;
        memory
            .read_bytes(ptr as usize, len as usize)
            .map_err(|_| FAULT)?;
        buffers.push((ptr, len));
    }

    Ok(buffers)
}

fn guest_memory(store: &mut Store, instance: Instance) -> Option<&mut GuestMemory> {
    store
        .exports(instance)
        .iter()
        .find_map(|export| {
            if export.name == "memory" {
                if let ExternalValue::Memory { addr } = export.value {
                    return Some(addr);
                }
            }
            None
        })
        .map(|addr| &mut store.memories[addr].data)
}

fn errno(code: i32) -> HostOutcome {
    HostOutcome::Return(vec![RawValue::from(code)])
}
