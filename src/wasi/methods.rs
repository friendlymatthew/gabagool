use crate::{
    prelude::*, Caller, ExternalValue, GuestMemory, HostOutcome, Instance, RawValue, Store,
};

use super::{OutputError, WasiCtx, WasiResource, RIGHTS_FD_WRITE};

const SUCCESS: i32 = 0;
const AGAIN: i32 = 6;
const BADF: i32 = 8;
const FAULT: i32 = 21;
const INTR: i32 = 27;
const IO: i32 = 29;
const OVERFLOW: i32 = 61;
const NOTCAPABLE: i32 = 76;

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
    if entry.rights_base & RIGHTS_FD_WRITE == 0 {
        return errno(NOTCAPABLE);
    }
    let output = wasi_some!(entry.resource.output_mut(), BADF);

    // Validate all buffers before output, then borrow their bytes directly.
    // The output context is borrowed independently from guest memory.
    let mut buffers = Vec::new();

    {
        let table_len = wasi_some!(iovs_len.checked_mul(8), FAULT);
        let table = wasi_try!(memory.read_bytes(iovs_ptr, table_len), FAULT);

        wasi_try!(memory.read_bytes(nwritten_ptr, 4), FAULT);

        let mut total = 0u32;

        let (chunks, _remainder) = table.as_chunks::<8>();

        for &[p1, p2, p3, p4, l1, l2, l3, l4] in chunks {
            let ptr = u32::from_le_bytes([p1, p2, p3, p4]) as usize;
            let len = u32::from_le_bytes([l1, l2, l3, l4]);

            if len == 0 {
                continue;
            }

            total = wasi_some!(total.checked_add(len), OVERFLOW);

            let bs = wasi_try!(memory.read_bytes(ptr, len as usize), FAULT);

            buffers.push(bs);
        }
    }

    let mut written = 0u32;
    for bytes in buffers {
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
                    Err(OutputError::Interrupted) => INTR,
                    Err(OutputError::WouldBlock) => AGAIN,
                    _ => IO,
                });
            }
        }
    }

    wasi_try!(memory.write(nwritten_ptr, written.to_le_bytes()), FAULT);

    errno(SUCCESS)
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
