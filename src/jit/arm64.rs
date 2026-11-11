use crate::{bail_jit, ensure_jit, prelude::*, Result};

use super::{code_layout::DataSlotPatch, PatchEncoding};

pub(super) fn resolve_data_slot_addresses(
    memory: &mut [u8],
    code_len: usize,
    patches: &[DataSlotPatch],
) -> Result<()> {
    let base = memory.as_ptr() as usize;

    ensure_jit!(
        base.is_multiple_of(4),
        "machine code must be 4-byte aligned"
    );

    ensure_jit!(code_len <= memory.len(), "code extends outside allocation");

    for patch in patches {
        ensure_jit!(
            code_len >= 4 && patch.offset <= code_len - 4 && patch.offset.is_multiple_of(4),
            "address patch is outside code or not instruction aligned"
        );

        ensure_jit!(
            memory.len() >= 8
                && patch.slot_offset >= code_len
                && patch.slot_offset <= memory.len() - 8,
            "local-index slot is outside data"
        );

        let location = base + patch.offset;
        let target = base + patch.slot_offset;

        ensure_jit!(
            target.is_multiple_of(8),
            "local-index slot must be 8-byte aligned"
        );
        let instruction =
            u32::from_le_bytes(memory[patch.offset..patch.offset + 4].try_into().unwrap());

        let patched = match patch.encoding {
            PatchEncoding::GotPage21 => {
                ensure_jit!(
                    instruction & 0x9f00_0000 == 0x9000_0000,
                    "expected ADRP instruction"
                );

                let pages = (target >> 12) as i64 - (location >> 12) as i64;

                ensure_jit!(
                    (-(1_i64 << 20)..(1_i64 << 20)).contains(&pages),
                    "local-index slot is outside ADRP range"
                );

                let immediate = (pages as u32) & 0x1f_ffff;

                (instruction & !((3 << 29) | (0x7ffff << 5)))
                    | ((immediate & 3) << 29)
                    | ((immediate >> 2) << 5)
            }
            PatchEncoding::GotPageOff12 => {
                ensure_jit!(
                    instruction & 0xffc0_0000 == 0xf940_0000,
                    "expected 64-bit LDR instruction"
                );

                let offset_in_page = (target & 0xfff) as u32;
                (instruction & !(0xfff << 10)) | ((offset_in_page / 8) << 10)
            }
            PatchEncoding::Branch26 => bail_jit!("branch is not a pending address patch"),
        };
        memory[patch.offset..patch.offset + 4].copy_from_slice(&patched.to_le_bytes());
    }
    Ok(())
}
