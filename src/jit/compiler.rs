use crate::{bail_jit, bail_trap, ensure_jit, ir::Op, prelude::*, Error, Result, Trap};

use super::{
    code_buffer::CodeBuffer, code_layout::CodeLayout, Stencil, StencilInstance, STENCIL_F32_ABS,
    STENCIL_F32_ADD, STENCIL_F32_CEIL, STENCIL_F32_CONST, STENCIL_F32_CONVERT_I32_S,
    STENCIL_F32_CONVERT_I32_U, STENCIL_F32_CONVERT_I64_S, STENCIL_F32_CONVERT_I64_U,
    STENCIL_F32_COPYSIGN, STENCIL_F32_DEMOTE_F64, STENCIL_F32_DIV, STENCIL_F32_EQ,
    STENCIL_F32_FLOOR, STENCIL_F32_GE, STENCIL_F32_GT, STENCIL_F32_LE, STENCIL_F32_LT,
    STENCIL_F32_MAX, STENCIL_F32_MIN, STENCIL_F32_MUL, STENCIL_F32_NE, STENCIL_F32_NEAREST,
    STENCIL_F32_NEG, STENCIL_F32_REINTERPRET_I32, STENCIL_F32_SQRT, STENCIL_F32_SUB,
    STENCIL_F32_TRUNC, STENCIL_F64_ABS, STENCIL_F64_ADD, STENCIL_F64_CEIL, STENCIL_F64_CONST,
    STENCIL_F64_CONVERT_I32_S, STENCIL_F64_CONVERT_I32_U, STENCIL_F64_CONVERT_I64_S,
    STENCIL_F64_CONVERT_I64_U, STENCIL_F64_COPYSIGN, STENCIL_F64_DIV, STENCIL_F64_EQ,
    STENCIL_F64_FLOOR, STENCIL_F64_GE, STENCIL_F64_GT, STENCIL_F64_LE, STENCIL_F64_LT,
    STENCIL_F64_MAX, STENCIL_F64_MIN, STENCIL_F64_MUL, STENCIL_F64_NE, STENCIL_F64_NEAREST,
    STENCIL_F64_NEG, STENCIL_F64_PROMOTE_F32, STENCIL_F64_REINTERPRET_I64, STENCIL_F64_SQRT,
    STENCIL_F64_SUB, STENCIL_F64_TRUNC, STENCIL_I32_ADD, STENCIL_I32_AND, STENCIL_I32_CLZ,
    STENCIL_I32_CONST, STENCIL_I32_CTZ, STENCIL_I32_DIV_S, STENCIL_I32_DIV_U, STENCIL_I32_EQ,
    STENCIL_I32_EQZ, STENCIL_I32_EXTEND16_S, STENCIL_I32_EXTEND8_S, STENCIL_I32_GE_S,
    STENCIL_I32_GE_U, STENCIL_I32_GT_S, STENCIL_I32_GT_U, STENCIL_I32_LE_S, STENCIL_I32_LE_U,
    STENCIL_I32_LT_S, STENCIL_I32_LT_U, STENCIL_I32_MUL, STENCIL_I32_NE, STENCIL_I32_OR,
    STENCIL_I32_POPCNT, STENCIL_I32_REINTERPRET_F32, STENCIL_I32_REM_S, STENCIL_I32_REM_U,
    STENCIL_I32_ROTL, STENCIL_I32_ROTR, STENCIL_I32_SHL, STENCIL_I32_SHR_S, STENCIL_I32_SHR_U,
    STENCIL_I32_SUB, STENCIL_I32_TRUNC_F32_S, STENCIL_I32_TRUNC_F32_U, STENCIL_I32_TRUNC_F64_S,
    STENCIL_I32_TRUNC_F64_U, STENCIL_I32_TRUNC_SAT_F32_S, STENCIL_I32_TRUNC_SAT_F32_U,
    STENCIL_I32_TRUNC_SAT_F64_S, STENCIL_I32_TRUNC_SAT_F64_U, STENCIL_I32_WRAP_I64,
    STENCIL_I32_XOR, STENCIL_I64_ADD, STENCIL_I64_AND, STENCIL_I64_CLZ, STENCIL_I64_CONST,
    STENCIL_I64_CTZ, STENCIL_I64_DIV_S, STENCIL_I64_DIV_U, STENCIL_I64_EQ, STENCIL_I64_EQZ,
    STENCIL_I64_EXTEND16_S, STENCIL_I64_EXTEND32_S, STENCIL_I64_EXTEND8_S,
    STENCIL_I64_EXTEND_I32_S, STENCIL_I64_EXTEND_I32_U, STENCIL_I64_GE_S, STENCIL_I64_GE_U,
    STENCIL_I64_GT_S, STENCIL_I64_GT_U, STENCIL_I64_LE_S, STENCIL_I64_LE_U, STENCIL_I64_LT_S,
    STENCIL_I64_LT_U, STENCIL_I64_MUL, STENCIL_I64_NE, STENCIL_I64_OR, STENCIL_I64_POPCNT,
    STENCIL_I64_REINTERPRET_F64, STENCIL_I64_REM_S, STENCIL_I64_REM_U, STENCIL_I64_ROTL,
    STENCIL_I64_ROTR, STENCIL_I64_SHL, STENCIL_I64_SHR_S, STENCIL_I64_SHR_U, STENCIL_I64_SUB,
    STENCIL_I64_TRUNC_F32_S, STENCIL_I64_TRUNC_F32_U, STENCIL_I64_TRUNC_F64_S,
    STENCIL_I64_TRUNC_F64_U, STENCIL_I64_TRUNC_SAT_F32_S, STENCIL_I64_TRUNC_SAT_F32_U,
    STENCIL_I64_TRUNC_SAT_F64_S, STENCIL_I64_TRUNC_SAT_F64_U, STENCIL_I64_XOR, STENCIL_LOCAL_GET,
    STENCIL_LOCAL_GET2, STENCIL_LOCAL_SET, STENCIL_LOCAL_TEE, STENCIL_RETURN_VALUE,
};

const fn find_stencil(op: Op) -> Option<&'static Stencil> {
    Some(match op {
        Op::I32Const { .. } => &STENCIL_I32_CONST,
        Op::LocalGet { .. } => &STENCIL_LOCAL_GET,
        Op::LocalSet { .. } => &STENCIL_LOCAL_SET,
        Op::LocalTee { .. } => &STENCIL_LOCAL_TEE,
        Op::LocalGet2 { .. } => &STENCIL_LOCAL_GET2,
        Op::Return => &STENCIL_RETURN_VALUE,
        Op::I32Add => &STENCIL_I32_ADD,
        Op::I32Sub => &STENCIL_I32_SUB,
        Op::I32Mul => &STENCIL_I32_MUL,
        Op::I32DivSigned => &STENCIL_I32_DIV_S,
        Op::I32DivUnsigned => &STENCIL_I32_DIV_U,
        Op::I32RemainderSigned => &STENCIL_I32_REM_S,
        Op::I32RemainderUnsigned => &STENCIL_I32_REM_U,
        Op::I32EqZero => &STENCIL_I32_EQZ,
        Op::I32Eq => &STENCIL_I32_EQ,
        Op::I32Ne => &STENCIL_I32_NE,
        Op::I32LtSigned => &STENCIL_I32_LT_S,
        Op::I32LtUnsigned => &STENCIL_I32_LT_U,
        Op::I32GtSigned => &STENCIL_I32_GT_S,
        Op::I32GtUnsigned => &STENCIL_I32_GT_U,
        Op::I32LeSigned => &STENCIL_I32_LE_S,
        Op::I32LeUnsigned => &STENCIL_I32_LE_U,
        Op::I32GeSigned => &STENCIL_I32_GE_S,
        Op::I32GeUnsigned => &STENCIL_I32_GE_U,
        Op::I32CountLeadingZeros => &STENCIL_I32_CLZ,
        Op::I32CountTrailingZeros => &STENCIL_I32_CTZ,
        Op::I32PopCount => &STENCIL_I32_POPCNT,
        Op::I32And => &STENCIL_I32_AND,
        Op::I32Or => &STENCIL_I32_OR,
        Op::I32Xor => &STENCIL_I32_XOR,
        Op::I32Shl => &STENCIL_I32_SHL,
        Op::I32ShrUnsigned => &STENCIL_I32_SHR_U,
        Op::I32RotateLeft => &STENCIL_I32_ROTL,
        Op::I32RotateRight => &STENCIL_I32_ROTR,
        Op::I32ShrSigned => &STENCIL_I32_SHR_S,
        Op::I64CountLeadingZeros => &STENCIL_I64_CLZ,
        Op::I64CountTrailingZeros => &STENCIL_I64_CTZ,
        Op::I64PopCount => &STENCIL_I64_POPCNT,
        Op::I64And => &STENCIL_I64_AND,
        Op::I64Or => &STENCIL_I64_OR,
        Op::I64Xor => &STENCIL_I64_XOR,
        Op::I64Shl => &STENCIL_I64_SHL,
        Op::I64ShrUnsigned => &STENCIL_I64_SHR_U,
        Op::I64RotateLeft => &STENCIL_I64_ROTL,
        Op::I64RotateRight => &STENCIL_I64_ROTR,
        Op::I64ShrSigned => &STENCIL_I64_SHR_S,
        Op::I64Add => &STENCIL_I64_ADD,
        Op::I64Sub => &STENCIL_I64_SUB,
        Op::I64Mul => &STENCIL_I64_MUL,
        Op::I64DivSigned => &STENCIL_I64_DIV_S,
        Op::I64DivUnsigned => &STENCIL_I64_DIV_U,
        Op::I64RemainderSigned => &STENCIL_I64_REM_S,
        Op::I64RemainderUnsigned => &STENCIL_I64_REM_U,
        Op::I64EqZero => &STENCIL_I64_EQZ,
        Op::I64Eq => &STENCIL_I64_EQ,
        Op::I64Ne => &STENCIL_I64_NE,
        Op::I64LtSigned => &STENCIL_I64_LT_S,
        Op::I64LtUnsigned => &STENCIL_I64_LT_U,
        Op::I64GtSigned => &STENCIL_I64_GT_S,
        Op::I64GtUnsigned => &STENCIL_I64_GT_U,
        Op::I64LeSigned => &STENCIL_I64_LE_S,
        Op::I64LeUnsigned => &STENCIL_I64_LE_U,
        Op::I64GeSigned => &STENCIL_I64_GE_S,
        Op::I64GeUnsigned => &STENCIL_I64_GE_U,
        Op::F32Abs => &STENCIL_F32_ABS,
        Op::F32Neg => &STENCIL_F32_NEG,
        Op::F32Ceil => &STENCIL_F32_CEIL,
        Op::F32Floor => &STENCIL_F32_FLOOR,
        Op::F32Trunc => &STENCIL_F32_TRUNC,
        Op::F32Nearest => &STENCIL_F32_NEAREST,
        Op::F32Sqrt => &STENCIL_F32_SQRT,
        Op::F32Add => &STENCIL_F32_ADD,
        Op::F32Sub => &STENCIL_F32_SUB,
        Op::F32Mul => &STENCIL_F32_MUL,
        Op::F32Div => &STENCIL_F32_DIV,
        Op::F32Eq => &STENCIL_F32_EQ,
        Op::F32Ne => &STENCIL_F32_NE,
        Op::F32Lt => &STENCIL_F32_LT,
        Op::F32Gt => &STENCIL_F32_GT,
        Op::F32Le => &STENCIL_F32_LE,
        Op::F32Ge => &STENCIL_F32_GE,
        Op::F32Min => &STENCIL_F32_MIN,
        Op::F32Max => &STENCIL_F32_MAX,
        Op::F32CopySign => &STENCIL_F32_COPYSIGN,
        Op::F64Abs => &STENCIL_F64_ABS,
        Op::F64Neg => &STENCIL_F64_NEG,
        Op::F64Ceil => &STENCIL_F64_CEIL,
        Op::F64Floor => &STENCIL_F64_FLOOR,
        Op::F64Trunc => &STENCIL_F64_TRUNC,
        Op::F64Nearest => &STENCIL_F64_NEAREST,
        Op::F64Sqrt => &STENCIL_F64_SQRT,
        Op::F64Add => &STENCIL_F64_ADD,
        Op::F64Sub => &STENCIL_F64_SUB,
        Op::F64Mul => &STENCIL_F64_MUL,
        Op::F64Div => &STENCIL_F64_DIV,
        Op::F64Eq => &STENCIL_F64_EQ,
        Op::F64Ne => &STENCIL_F64_NE,
        Op::F64Lt => &STENCIL_F64_LT,
        Op::F64Gt => &STENCIL_F64_GT,
        Op::F64Le => &STENCIL_F64_LE,
        Op::F64Ge => &STENCIL_F64_GE,
        Op::F64Min => &STENCIL_F64_MIN,
        Op::F64Max => &STENCIL_F64_MAX,
        Op::F64CopySign => &STENCIL_F64_COPYSIGN,
        Op::I64Const { .. } => &STENCIL_I64_CONST,
        Op::F32Const { .. } => &STENCIL_F32_CONST,
        Op::F64Const { .. } => &STENCIL_F64_CONST,
        Op::I32WrapI64 => &STENCIL_I32_WRAP_I64,
        Op::I64ExtendI32Signed => &STENCIL_I64_EXTEND_I32_S,
        Op::I64ExtendI32Unsigned => &STENCIL_I64_EXTEND_I32_U,
        Op::I32ReinterpretF32 => &STENCIL_I32_REINTERPRET_F32,
        Op::I64ReinterpretF64 => &STENCIL_I64_REINTERPRET_F64,
        Op::F32ReinterpretI32 => &STENCIL_F32_REINTERPRET_I32,
        Op::F64ReinterpretI64 => &STENCIL_F64_REINTERPRET_I64,
        Op::I32Extend8Signed => &STENCIL_I32_EXTEND8_S,
        Op::I32Extend16Signed => &STENCIL_I32_EXTEND16_S,
        Op::I64Extend8Signed => &STENCIL_I64_EXTEND8_S,
        Op::I64Extend16Signed => &STENCIL_I64_EXTEND16_S,
        Op::I64Extend32Signed => &STENCIL_I64_EXTEND32_S,
        Op::F32ConvertI32Signed => &STENCIL_F32_CONVERT_I32_S,
        Op::F32ConvertI32Unsigned => &STENCIL_F32_CONVERT_I32_U,
        Op::F32ConvertI64Signed => &STENCIL_F32_CONVERT_I64_S,
        Op::F32ConvertI64Unsigned => &STENCIL_F32_CONVERT_I64_U,
        Op::F64ConvertI32Signed => &STENCIL_F64_CONVERT_I32_S,
        Op::F64ConvertI32Unsigned => &STENCIL_F64_CONVERT_I32_U,
        Op::F64ConvertI64Signed => &STENCIL_F64_CONVERT_I64_S,
        Op::F64ConvertI64Unsigned => &STENCIL_F64_CONVERT_I64_U,
        Op::F32DemoteF64 => &STENCIL_F32_DEMOTE_F64,
        Op::F64PromoteF32 => &STENCIL_F64_PROMOTE_F32,
        Op::I32TruncF32Signed => &STENCIL_I32_TRUNC_F32_S,
        Op::I32TruncSaturatedF32Signed => &STENCIL_I32_TRUNC_SAT_F32_S,
        Op::I32TruncF32Unsigned => &STENCIL_I32_TRUNC_F32_U,
        Op::I32TruncSaturatedF32Unsigned => &STENCIL_I32_TRUNC_SAT_F32_U,
        Op::I32TruncF64Signed => &STENCIL_I32_TRUNC_F64_S,
        Op::I32TruncSaturatedF64Signed => &STENCIL_I32_TRUNC_SAT_F64_S,
        Op::I32TruncF64Unsigned => &STENCIL_I32_TRUNC_F64_U,
        Op::I32TruncSaturatedF64Unsigned => &STENCIL_I32_TRUNC_SAT_F64_U,
        Op::I64TruncF32Signed => &STENCIL_I64_TRUNC_F32_S,
        Op::I64TruncSaturatedF32Signed => &STENCIL_I64_TRUNC_SAT_F32_S,
        Op::I64TruncF32Unsigned => &STENCIL_I64_TRUNC_F32_U,
        Op::I64TruncSaturatedF32Unsigned => &STENCIL_I64_TRUNC_SAT_F32_U,
        Op::I64TruncF64Signed => &STENCIL_I64_TRUNC_F64_S,
        Op::I64TruncSaturatedF64Signed => &STENCIL_I64_TRUNC_SAT_F64_S,
        Op::I64TruncF64Unsigned => &STENCIL_I64_TRUNC_F64_U,
        Op::I64TruncSaturatedF64Unsigned => &STENCIL_I64_TRUNC_SAT_F64_U,
        _ => return None,
    })
}

pub struct JitCompiledFunction {
    code: CodeBuffer,
    required_locals: u64,
    max_stack_height: usize,
    result_count: usize,
}

#[repr(C)]
struct VMContext {
    locals: *mut u64,
    stack: *mut u64,
    stack_height: u64,
    trap: u64,
}

impl JitCompiledFunction {
    pub const fn result_count(&self) -> usize {
        self.result_count
    }

    pub(crate) fn execute_vm(
        &self,
        locals: &mut [crate::RawValue],
        stack: &mut [crate::RawValue],
    ) -> Result<usize> {
        unsafe {
            let locals =
                core::slice::from_raw_parts_mut(locals.as_mut_ptr().cast::<u64>(), locals.len());

            let stack =
                core::slice::from_raw_parts_mut(stack.as_mut_ptr().cast::<u64>(), stack.len());

            self.execute(locals, stack)
        }
    }

    pub(super) fn execute(&self, locals: &mut [u64], stack: &mut [u64]) -> Result<usize> {
        ensure_jit!(
            locals.len() as u64 >= self.required_locals,
            "not enough locals"
        );

        ensure_jit!(
            stack.len() >= self.max_stack_height,
            "operand stack is too small"
        );

        let mut context = VMContext {
            locals: locals.as_mut_ptr(),
            stack: stack.as_mut_ptr(),
            stack_height: 0,
            trap: 0,
        };

        type Entry = unsafe extern "C" fn(*mut VMContext);

        unsafe {
            let entry: Entry = core::mem::transmute(self.code.as_ptr());
            entry(&mut context);
        }

        match context.trap {
            0 => {}
            1 => bail_trap!(Trap::IntegerDivideByZero),
            2 => bail_trap!(Trap::IntegerOverflow),
            3 => bail_trap!(Trap::InvalidConversionToInteger),
            code => bail_jit!("unknown trap code {code}"),
        }

        ensure_jit!(
            context.stack_height as usize == self.result_count,
            "unexpected result stack height"
        );

        Ok(self.result_count)
    }
}

pub fn supports(ops: &[Op]) -> bool {
    ops.iter().all(|&op| find_stencil(op).is_some())
}

pub fn compile(ops: &[Op]) -> Result<JitCompiledFunction> {
    ensure_jit!(
        matches!(ops.last(), Some(Op::Return)),
        "function must end with Return"
    );

    let mut instances = Vec::with_capacity(ops.len());

    let mut stack_height = 0_usize;
    let mut max_stack_height = 0;
    let mut required_locals = 0_u64;

    for (pc, &op) in ops.iter().enumerate() {
        let stencil = find_stencil(op)
            .ok_or_else(|| Error::jit(format!("unsupported operation at IR {pc}: {op:?}")))?;

        let parameters = match op {
            Op::I32Const { value } => {
                stack_height += 1;

                vec![u64::from(value as u32)]
            }
            Op::I64Const { value } => {
                stack_height += 1;

                vec![value as u64]
            }
            Op::F32Const { value } => {
                stack_height += 1;

                vec![u64::from(value.to_bits())]
            }
            Op::F64Const { value } => {
                stack_height += 1;

                vec![value.to_bits()]
            }
            Op::LocalGet { local_i } => {
                required_locals = required_locals.max(u64::from(local_i) + 1);
                stack_height += 1;

                vec![u64::from(local_i)]
            }
            Op::LocalSet { local_i } => {
                ensure_jit!(
                    stack_height >= 1,
                    "stack underflow at IR {pc}: LocalSet needs one value"
                );

                required_locals = required_locals.max(u64::from(local_i) + 1);
                stack_height -= 1;

                vec![u64::from(local_i)]
            }
            Op::LocalTee { local_i } => {
                ensure_jit!(
                    stack_height >= 1,
                    "stack underflow at IR {pc}: LocalTee needs one value"
                );

                required_locals = required_locals.max(u64::from(local_i) + 1);

                vec![u64::from(local_i)]
            }
            Op::LocalGet2 {
                local_i_a,
                local_i_b,
            } => {
                required_locals = required_locals.max(u64::from(local_i_a.max(local_i_b)) + 1);
                stack_height += 2;
                vec![u64::from(local_i_a), u64::from(local_i_b)]
            }
            Op::I32EqZero
            | Op::I32CountLeadingZeros
            | Op::I32CountTrailingZeros
            | Op::I32PopCount
            | Op::I64CountLeadingZeros
            | Op::I64CountTrailingZeros
            | Op::I64PopCount
            | Op::I64EqZero
            | Op::F32Abs
            | Op::F32Neg
            | Op::F32Ceil
            | Op::F32Floor
            | Op::F32Trunc
            | Op::F32Nearest
            | Op::F32Sqrt
            | Op::F64Abs
            | Op::F64Neg
            | Op::F64Ceil
            | Op::F64Floor
            | Op::F64Trunc
            | Op::F64Nearest
            | Op::F64Sqrt
            | Op::I32WrapI64
            | Op::I64ExtendI32Signed
            | Op::I64ExtendI32Unsigned
            | Op::I32ReinterpretF32
            | Op::I64ReinterpretF64
            | Op::F32ReinterpretI32
            | Op::F64ReinterpretI64
            | Op::I32Extend8Signed
            | Op::I32Extend16Signed
            | Op::I64Extend8Signed
            | Op::I64Extend16Signed
            | Op::I64Extend32Signed
            | Op::F32ConvertI32Signed
            | Op::F32ConvertI32Unsigned
            | Op::F32ConvertI64Signed
            | Op::F32ConvertI64Unsigned
            | Op::F64ConvertI32Signed
            | Op::F64ConvertI32Unsigned
            | Op::F64ConvertI64Signed
            | Op::F64ConvertI64Unsigned
            | Op::F32DemoteF64
            | Op::F64PromoteF32
            | Op::I32TruncF32Signed
            | Op::I32TruncSaturatedF32Signed
            | Op::I32TruncF32Unsigned
            | Op::I32TruncSaturatedF32Unsigned
            | Op::I32TruncF64Signed
            | Op::I32TruncSaturatedF64Signed
            | Op::I32TruncF64Unsigned
            | Op::I32TruncSaturatedF64Unsigned
            | Op::I64TruncF32Signed
            | Op::I64TruncSaturatedF32Signed
            | Op::I64TruncF32Unsigned
            | Op::I64TruncSaturatedF32Unsigned
            | Op::I64TruncF64Signed
            | Op::I64TruncSaturatedF64Signed
            | Op::I64TruncF64Unsigned
            | Op::I64TruncSaturatedF64Unsigned => {
                ensure_jit!(
                    stack_height >= 1,
                    "stack underflow at IR {pc}: {op:?} needs one value"
                );

                Vec::new()
            }
            Op::I32Add
            | Op::I32And
            | Op::I32Or
            | Op::I32Xor
            | Op::I32Shl
            | Op::I32ShrUnsigned
            | Op::I32RotateLeft
            | Op::I32RotateRight
            | Op::I32ShrSigned
            | Op::I64And
            | Op::I64Or
            | Op::I64Xor
            | Op::I64Shl
            | Op::I64ShrUnsigned
            | Op::I64RotateLeft
            | Op::I64RotateRight
            | Op::I64ShrSigned
            | Op::I64Add
            | Op::I64Sub
            | Op::I64Mul
            | Op::I64DivSigned
            | Op::I64DivUnsigned
            | Op::I64RemainderSigned
            | Op::I64RemainderUnsigned
            | Op::I64Eq
            | Op::I64Ne
            | Op::I64LtSigned
            | Op::I64LtUnsigned
            | Op::I64GtSigned
            | Op::I64GtUnsigned
            | Op::I64LeSigned
            | Op::I64LeUnsigned
            | Op::I64GeSigned
            | Op::I64GeUnsigned
            | Op::F32Add
            | Op::F32Sub
            | Op::F32Mul
            | Op::F32Div
            | Op::F32Eq
            | Op::F32Ne
            | Op::F32Lt
            | Op::F32Gt
            | Op::F32Le
            | Op::F32Ge
            | Op::F32Min
            | Op::F32Max
            | Op::F32CopySign
            | Op::F64Add
            | Op::F64Sub
            | Op::F64Mul
            | Op::F64Div
            | Op::F64Eq
            | Op::F64Ne
            | Op::F64Lt
            | Op::F64Gt
            | Op::F64Le
            | Op::F64Ge
            | Op::F64Min
            | Op::F64Max
            | Op::F64CopySign
            | Op::I32Sub
            | Op::I32Mul
            | Op::I32DivSigned
            | Op::I32DivUnsigned
            | Op::I32RemainderSigned
            | Op::I32RemainderUnsigned
            | Op::I32Eq
            | Op::I32Ne
            | Op::I32LtSigned
            | Op::I32LtUnsigned
            | Op::I32GtSigned
            | Op::I32GtUnsigned
            | Op::I32LeSigned
            | Op::I32LeUnsigned
            | Op::I32GeSigned
            | Op::I32GeUnsigned => {
                ensure_jit!(
                    stack_height >= 2,
                    "stack underflow at IR {pc}: {op:?} needs two values"
                );

                stack_height -= 1;
                Vec::new()
            }
            Op::Return => {
                ensure_jit!(
                    pc + 1 == ops.len(),
                    "instructions after Return at IR {pc} are not supported"
                );
                Vec::new()
            }
            _ => unreachable!("unsupported operations were rejected above"),
        };

        max_stack_height = max_stack_height.max(stack_height);

        instances.push(StencilInstance {
            stencil,
            parameters,
        });
    }

    let layout = CodeLayout::try_new(&instances)?;

    Ok(JitCompiledFunction {
        code: CodeBuffer::new(&layout)?,
        required_locals,
        max_stack_height,
        result_count: stack_height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executes_i32_comparisons() {
        let comparisons: &[(Op, fn(i32, i32) -> bool)] = &[
            (Op::I32Eq, |a, b| a == b),
            (Op::I32Ne, |a, b| a != b),
            (Op::I32LtSigned, |a, b| a < b),
            (Op::I32LtUnsigned, |a, b| (a as u32) < (b as u32)),
            (Op::I32GtSigned, |a, b| a > b),
            (Op::I32GtUnsigned, |a, b| (a as u32) > (b as u32)),
            (Op::I32LeSigned, |a, b| a <= b),
            (Op::I32LeUnsigned, |a, b| (a as u32) <= (b as u32)),
            (Op::I32GeSigned, |a, b| a >= b),
            (Op::I32GeUnsigned, |a, b| (a as u32) >= (b as u32)),
        ];

        for &(op, expected) in comparisons {
            let ops = [
                Op::LocalGet2 {
                    local_i_a: 0,
                    local_i_b: 1,
                },
                op,
                Op::Return,
            ];

            assert!(supports(&ops));

            let function = compile(&ops).unwrap();

            assert_eq!(function.result_count(), 1);

            for (a, b) in [(0, 0), (1, 2), (2, 1), (-1, 1), (i32::MIN, i32::MAX)] {
                let mut locals = [u64::from(a as u32), u64::from(b as u32)];
                let mut stack = [0; 2];

                assert_eq!(function.execute(&mut locals, &mut stack).unwrap(), 1);
                assert_eq!(stack[0], u64::from(expected(a, b)), "{op:?}: {a}, {b}");
            }
        }
    }

    #[test]
    fn executes_i32_eqz() {
        let ops = [Op::LocalGet { local_i: 0 }, Op::I32EqZero, Op::Return];

        assert!(supports(&ops));

        let function = compile(&ops).unwrap();

        assert_eq!(function.max_stack_height, 1);

        for value in [0, 1, -1, i32::MIN] {
            let mut locals = [u64::from(value as u32)];
            let mut stack = [0];

            assert_eq!(function.execute(&mut locals, &mut stack).unwrap(), 1);
            assert_eq!(stack[0], u64::from(value == 0));
        }
    }

    #[test]
    fn sets_and_reads_local() {
        let ops = [
            Op::I32Const { value: 42 },
            Op::LocalSet { local_i: 3 },
            Op::LocalGet { local_i: 3 },
            Op::Return,
        ];
        assert!(supports(&ops));

        let function = compile(&ops).unwrap();

        assert_eq!(function.required_locals, 4);
        assert_eq!(function.max_stack_height, 1);

        let mut locals = [0; 4];
        let mut stack = [0; 1];

        assert_eq!(function.execute(&mut locals, &mut stack).unwrap(), 1);
        assert_eq!(locals, [0, 0, 0, 42]);
        assert_eq!(stack[0], 42);
    }

    #[test]
    fn compiles_and_executes_i32_constants() {
        for (a, b, expected) in [(20, 22, 42_u64), (-1, 1, 0), (i32::MIN, 0, 0x8000_0000)] {
            let ops = [
                Op::I32Const { value: a },
                Op::I32Const { value: b },
                Op::I32Add,
                Op::Return,
            ];

            assert!(supports(&ops));

            let function = compile(&ops).unwrap();

            let mut stack = [0_u64; 2];

            assert_eq!(function.execute(&mut [], &mut stack).unwrap(), 1);
            assert_eq!(stack[0], expected);
        }
    }
}
