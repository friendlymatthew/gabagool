use crate::{ensure_jit, prelude::*, Result};

use crate::jit::{PatchEncoding, PatchValue, StencilInstance};

#[derive(Debug)]
pub struct DataSlotPatch {
    pub offset: usize,
    pub slot_offset: usize,
    pub encoding: PatchEncoding,
}

#[derive(Debug)]
pub struct CodeLayout {
    pub bytes: Vec<u8>,
    pub code_len: usize,
    pub code_offsets: Vec<usize>,
    pub data_slot_patches: Vec<DataSlotPatch>,
}

impl CodeLayout {
    pub fn try_new(instances: &[StencilInstance<'_>]) -> Result<Self> {
        let mut code_offsets = Vec::with_capacity(instances.len());
        let mut code_len = 0;

        for instance in instances {
            code_offsets.push(code_len);
            code_len += instance.stencil.code.len();
        }

        let slots_start = (code_len + 7) & !7;
        let mut data = Vec::new();
        let mut slots = Vec::with_capacity(instances.len());

        for instance in instances {
            let parameter_count = instance
                .stencil
                .patches
                .iter()
                .filter_map(|patch| match patch.value {
                    PatchValue::Parameter(index) => Some(index + 1),
                    PatchValue::Continue | PatchValue::Target => None,
                })
                .max()
                .unwrap_or(0);

            ensure_jit!(
                instance.parameters.len() == parameter_count,
                "parameter count mismatch"
            );

            slots.push(slots_start + data.len());

            for value in &instance.parameters {
                data.extend_from_slice(&value.to_le_bytes());
            }
        }

        let mut code = Vec::with_capacity(slots_start + data.len());
        let mut data_slot_patches = Vec::new();

        for (i, instance) in instances.iter().enumerate() {
            let stencil = instance.stencil;
            let start_offset = code.len();
            code.extend_from_slice(stencil.code);

            for patch in stencil.patches {
                ensure_jit!(
                    stencil.code.len() >= 4 && patch.offset <= stencil.code.len() - 4,
                    "patch is outside stencil"
                );

                ensure_jit!(
                    patch.addend == 0,
                    "nonzero patch addends are not supported yet"
                );

                if let PatchValue::Parameter(index) = patch.value {
                    ensure_jit!(
                        matches!(
                            patch.encoding,
                            PatchEncoding::GotPage21 | PatchEncoding::GotPageOff12
                        ),
                        "data-slot patch must use a GOT load encoding"
                    );

                    let slot_offset = slots[i] + index * 8;

                    data_slot_patches.push(DataSlotPatch {
                        offset: start_offset + patch.offset,
                        slot_offset,
                        encoding: patch.encoding,
                    });

                    continue;
                }

                let destination = match patch.value {
                    PatchValue::Continue => {
                        ensure_jit!(i + 1 < instances.len(), "another stencil must follow");
                        code_offsets[i + 1]
                    }
                    PatchValue::Target => {
                        let target = instance
                            .target
                            .ok_or_else(|| crate::Error::jit("jump stencil needs a target"))?;

                        *code_offsets
                            .get(target)
                            .ok_or_else(|| crate::Error::jit("jump target is outside function"))?
                    }
                    PatchValue::Parameter(_) => unreachable!(),
                };

                ensure_jit!(
                    matches!(patch.encoding, PatchEncoding::Branch26),
                    "encoding must be branch26"
                );

                let branch_position = start_offset + patch.offset;
                let distance = destination as i128 - branch_position as i128;

                ensure_jit!(distance % 4 == 0, "distance must be divisible by 4");

                let words = distance / 4;

                ensure_jit!(
                    (-(1_i128 << 25)..(1_i128 << 25)).contains(&words),
                    "branch distance exceeds signed 26-bit range"
                );

                let instruction = 0x1400_0000_u32 | (words as u32 & 0x03ff_ffff);

                code[branch_position..branch_position + 4]
                    .copy_from_slice(&instruction.to_le_bytes());
            }
        }

        if !data.is_empty() {
            code.resize(slots_start, 0);
            code.extend_from_slice(&data);
        }

        Ok(Self {
            bytes: code,
            code_len,
            code_offsets,
            data_slot_patches,
        })
    }

    pub fn resolve_data_slot_addresses(&self, memory: &mut [u8]) -> Result<()> {
        ensure_jit!(
            memory.len() == self.bytes.len(),
            "allocation size does not match layout bytes"
        );

        super::resolve_data_slot_addresses(memory, self.code_len, &self.data_slot_patches)
    }
}
