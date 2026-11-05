use crate::{prelude::*, Caller, ExternalValue, HostOutcome, RawValue};

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

pub(super) fn fd_write<O: WasiOutput>(
    mut caller: Caller<'_, WasiCtx<O>>,
    args: &[RawValue],
) -> HostOutcome {
    let fd = args[0].as_i32() as u32;
    let iovs_ptr = args[1].as_i32() as u32 as usize;
    let iovs_len = args[2].as_i32() as u32 as usize;
    let nwritten_ptr = args[3].as_i32() as u32 as usize;

    if fd != 1 {
        return errno(BADF);
    }

    let (store, instance, ctx) = caller.parts_mut();
    let Some(addr) = store.exports(instance).iter().find_map(|export| {
        if export.name == "memory" {
            if let ExternalValue::Memory { addr } = export.value {
                return Some(addr);
            }
        }
        None
    }) else {
        return errno(FAULT);
    };
    let memory = &mut store.memories[addr].data;

    // Validate all buffers before output, then borrow their bytes directly.
    // The output context is borrowed independently from guest memory.
    let mut buffers = Vec::new();

    {
        let Some(table_len) = iovs_len.checked_mul(8) else {
            return errno(FAULT);
        };
        let Ok(table) = memory.read_bytes(iovs_ptr, table_len) else {
            return errno(FAULT);
        };

        if memory.read_bytes(nwritten_ptr, 4).is_err() {
            return errno(FAULT);
        }

        let mut total = 0u32;

        let (chunks, _remainder) = table.as_chunks::<8>();

        for &[p1, p2, p3, p4, l1, l2, l3, l4] in chunks {
            let ptr = u32::from_le_bytes([p1, p2, p3, p4]) as usize;
            let len = u32::from_le_bytes([l1, l2, l3, l4]);

            if len == 0 {
                continue;
            }

            let Some(next_total) = total.checked_add(len) else {
                return errno(OVERFLOW);
            };

            total = next_total;

            let Ok(bs) = memory.read_bytes(ptr, len as usize) else {
                return errno(FAULT);
            };

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

    if memory.write(nwritten_ptr, written.to_le_bytes()).is_err() {
        return errno(FAULT);
    }

    errno(SUCCESS)
}
