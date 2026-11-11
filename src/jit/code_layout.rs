use crate::{ensure_jit, prelude::*, Result};

use crate::jit::{PatchEncoding, PatchValue, StencilInstance, StencilOperands};

#[derive(Debug)]
pub struct DataSlotPatch {
    pub offset: usize,
    pub slot_offset: usize,
    pub encoding: PatchEncoding,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::{Patch, Stencil};

    #[test]
    fn rejects_missing_second_local_index() {
        let stencil = Stencil {
            name: "requires_local_b",
            code: &[0; 4],
            patches: &[Patch {
                offset: 0,
                value: PatchValue::LocalB,
                encoding: PatchEncoding::GotPage21,
                addend: 0,
            }],
        };
        let instances = [StencilInstance {
            stencil: &stencil,
            operands: StencilOperands::LocalGet { local_i: 5 },
        }];

        assert!(matches!(
            CodeLayout::try_new(&instances),
            Err(crate::Error::Jit(message)) if message == "missing local-index operand"
        ));
    }
}

#[derive(Debug)]
pub struct CodeLayout {
    pub bytes: Vec<u8>,
    pub code_len: usize,
    pub data_slot_patches: Vec<DataSlotPatch>,
}

impl CodeLayout {
    pub fn try_new(instances: &[StencilInstance<'_>]) -> Result<Self> {
        let code_len: usize = instances
            .iter()
            .map(|instance| instance.stencil.code.len())
            .sum();

        let slots_start = (code_len + 7) & !7;
        let mut data = Vec::new();
        let mut slots = Vec::with_capacity(instances.len());

        for instance in instances {
            let has_local_patches = instance
                .stencil
                .patches
                .iter()
                .any(|patch| matches!(patch.value, PatchValue::LocalA | PatchValue::LocalB));

            match instance.operands {
                StencilOperands::None => {
                    ensure_jit!(
                        !has_local_patches,
                        "local index patches need local operands"
                    );

                    slots.push([None, None]);
                }
                StencilOperands::LocalGet { local_i } => {
                    ensure_jit!(
                        has_local_patches,
                        "local operands need a stencil with local index patches"
                    );

                    let a = slots_start + data.len();
                    data.extend_from_slice(&u64::from(local_i).to_le_bytes());

                    slots.push([Some(a), None]);
                }
                StencilOperands::LocalGet2 {
                    local_i_a,
                    local_i_b,
                } => {
                    ensure_jit!(
                        has_local_patches,
                        "local operands need a stencil with local index patches"
                    );

                    let a = slots_start + data.len();
                    data.extend_from_slice(&u64::from(local_i_a).to_le_bytes());

                    let b = slots_start + data.len();
                    data.extend_from_slice(&u64::from(local_i_b).to_le_bytes());

                    slots.push([Some(a), Some(b)]);
                }
            }
        }

        let mut code = Vec::with_capacity(slots_start + data.len());
        let mut data_slot_patches = Vec::new();

        for (i, instance) in instances.iter().enumerate() {
            let stencil = instance.stencil;
            let start_offset = code.len();
            code.extend_from_slice(stencil.code);
            let next_start_offset = code.len();

            for patch in stencil.patches {
                ensure_jit!(
                    stencil.code.len() >= 4 && patch.offset <= stencil.code.len() - 4,
                    "patch is outside stencil"
                );

                ensure_jit!(
                    patch.addend == 0,
                    "nonzero patch addends are not supported yet"
                );

                if matches!(patch.value, PatchValue::LocalA | PatchValue::LocalB) {
                    ensure_jit!(
                        matches!(
                            patch.encoding,
                            PatchEncoding::GotPage21 | PatchEncoding::GotPageOff12
                        ),
                        "local-index patch must use a GOT load encoding"
                    );

                    let [a, b] = slots[i];

                    let slot = if patch.value == PatchValue::LocalA {
                        a
                    } else {
                        b
                    };
                    let slot_offset =
                        slot.ok_or_else(|| crate::Error::jit("missing local-index operand"))?;

                    data_slot_patches.push(DataSlotPatch {
                        offset: start_offset + patch.offset,
                        slot_offset,
                        encoding: patch.encoding,
                    });
                    continue;
                }

                ensure_jit!(i + 1 < instances.len(), "another stencil must follow");

                ensure_jit!(
                    matches!(patch.encoding, PatchEncoding::Branch26),
                    "encoding must be branch26"
                );
                let branch_position = start_offset + patch.offset;

                let distance = next_start_offset - branch_position;

                ensure_jit!(
                    distance.is_multiple_of(4),
                    "distance must be divisible by 4"
                );

                ensure_jit!(
                    distance / 4 < (1 << 25),
                    "branch distance exceeds signed 26-bit range"
                );

                let instruction = 0x1400_0000_u32 | (distance / 4) as u32;

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
