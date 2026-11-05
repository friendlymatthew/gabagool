use crate::{
    prelude::*, Caller, ExternalValue, GuestMemory, HostOutcome, Instance, RawValue, Store,
};

use super::{OutputError, WasiCtx, WasiOutput};

const SUCCESS: i32 = 0;
const AGAIN: i32 = 6;
const BADF: i32 = 8;
const FAULT: i32 = 21;
const INTR: i32 = 27;
const IO: i32 = 29;
const OVERFLOW: i32 = 61;

fn errno(code: i32) -> HostOutcome {
    HostOutcome::Return(vec![RawValue::from(code)])
}

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

pub(super) fn environ_get<O: WasiOutput>(
    mut caller: Caller<'_, WasiCtx<O>>,
    args: &[RawValue],
) -> HostOutcome {
    let environ_ptr = args[0].as_pointer();
    let mut environ_buf_ptr = args[1].as_i32() as u32;

    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let envs = ctx.environment();

    let table_len = wasi_some!(envs.len().checked_mul(4), OVERFLOW);
    let buffer_len = envs.iter().try_fold(0u32, |total, env| {
        let len = u32::try_from(env.len()).ok()?;
        total.checked_add(len)?.checked_add(1)
    });
    let buffer_len = wasi_some!(buffer_len, OVERFLOW);

    wasi_try!(memory.read_bytes(environ_ptr, table_len), FAULT);

    wasi_try!(
        memory.read_bytes(environ_buf_ptr as usize, buffer_len as usize),
        FAULT
    );

    wasi_some!(environ_buf_ptr.checked_add(buffer_len), OVERFLOW);

    for (index, env) in envs.iter().enumerate() {
        wasi_try!(
            memory.write(environ_ptr + index * 4, environ_buf_ptr.to_le_bytes()),
            FAULT
        );
        wasi_try!(
            memory.write_bytes(environ_buf_ptr as usize, env.as_bytes()),
            FAULT
        );

        let nul_ptr = environ_buf_ptr + env.len() as u32;
        wasi_try!(memory.write(nul_ptr as usize, [0]), FAULT);
        environ_buf_ptr = nul_ptr + 1;
    }

    errno(SUCCESS)
}

pub(super) fn environ_sizes_get<O: WasiOutput>(
    mut caller: Caller<'_, WasiCtx<O>>,
    args: &[RawValue],
) -> HostOutcome {
    let num_envs_guest_address = args[0].as_pointer();
    let total_string_buffer_guest_address = args[1].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let envs = ctx.environment();
    let num_envs = wasi_try!(u32::try_from(envs.len()), OVERFLOW);

    let total_string_output = envs.iter().try_fold(0u32, |total, env| {
        let len = u32::try_from(env.len()).ok()?;
        total.checked_add(len)?.checked_add(1)
    });

    let total_string_output = wasi_some!(total_string_output, OVERFLOW);

    wasi_try!(
        memory.write(num_envs_guest_address, num_envs.to_le_bytes()),
        FAULT
    );

    wasi_try!(
        memory.write(
            total_string_buffer_guest_address,
            total_string_output.to_le_bytes(),
        ),
        FAULT
    );

    errno(SUCCESS)
}

pub(super) const fn proc_exit<O: WasiOutput>(
    _: Caller<'_, WasiCtx<O>>,
    args: &[RawValue],
) -> HostOutcome {
    HostOutcome::Exit(args[0].as_i32() as u32)
}

pub(super) fn args_get<O: WasiOutput>(
    mut caller: Caller<'_, WasiCtx<O>>,
    args: &[RawValue],
) -> HostOutcome {
    let argv_ptr = args[0].as_pointer();
    let mut argv_buf_ptr = args[1].as_i32() as u32;

    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let args = ctx.args();

    let table_len = wasi_some!(args.len().checked_mul(4), OVERFLOW);
    let buffer_len = args.iter().try_fold(0u32, |total, arg| {
        let len = u32::try_from(arg.len()).ok()?;
        total.checked_add(len)?.checked_add(1)
    });
    let buffer_len = wasi_some!(buffer_len, OVERFLOW);

    wasi_try!(memory.read_bytes(argv_ptr, table_len), FAULT);

    wasi_try!(
        memory.read_bytes(argv_buf_ptr as usize, buffer_len as usize),
        FAULT
    );

    wasi_some!(argv_buf_ptr.checked_add(buffer_len), OVERFLOW);

    for (index, arg) in args.iter().enumerate() {
        wasi_try!(
            memory.write(argv_ptr + index * 4, argv_buf_ptr.to_le_bytes()),
            FAULT
        );
        wasi_try!(
            memory.write_bytes(argv_buf_ptr as usize, arg.as_bytes()),
            FAULT
        );

        let nul_ptr = argv_buf_ptr + arg.len() as u32;
        wasi_try!(memory.write(nul_ptr as usize, [0]), FAULT);
        argv_buf_ptr = nul_ptr + 1;
    }

    errno(SUCCESS)
}

pub(super) fn args_sizes_get<O: WasiOutput>(
    mut caller: Caller<'_, WasiCtx<O>>,
    args: &[RawValue],
) -> HostOutcome {
    let num_args_guest_address = args[0].as_pointer();
    let total_string_buffer_guest_address = args[1].as_pointer();

    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

    let args = ctx.args();
    let num_args = wasi_try!(u32::try_from(args.len()), OVERFLOW);

    let total_string_output = args.iter().try_fold(0u32, |total, arg| {
        let len = u32::try_from(arg.len()).ok()?;
        total.checked_add(len)?.checked_add(1)
    });

    let total_string_output = wasi_some!(total_string_output, OVERFLOW);

    wasi_try!(
        memory.write(num_args_guest_address, num_args.to_le_bytes()),
        FAULT
    );

    wasi_try!(
        memory.write(
            total_string_buffer_guest_address,
            total_string_output.to_le_bytes(),
        ),
        FAULT
    );

    errno(SUCCESS)
}

pub(super) fn fd_write<O: WasiOutput>(
    mut caller: Caller<'_, WasiCtx<O>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let iovs_ptr = args[1].as_pointer();
    let iovs_len = args[2].as_pointer();
    let nwritten_ptr = args[3].as_pointer();

    if fd != 1 {
        return errno(BADF);
    }

    let (store, instance, ctx) = caller.parts_mut();
    let memory = wasi_some!(guest_memory(store, instance), FAULT);

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
        match ctx.stdout_mut().write(bytes) {
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
