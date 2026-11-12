use crate::{bail_jit, bail_trap, ensure_jit, ir::Op, prelude::*, Error, Result, Trap};

use super::{
    code_buffer::CodeBuffer,
    code_layout::CodeLayout,
    runtime::{self, BulkOperation, MemoryView, RuntimeContext},
    Stencil, StencilInstance, STENCIL_CALL, STENCIL_DATA_DROP, STENCIL_DROP, STENCIL_ELEM_DROP,
    STENCIL_F32_ABS, STENCIL_F32_ADD, STENCIL_F32_CEIL, STENCIL_F32_CONST,
    STENCIL_F32_CONVERT_I32_S, STENCIL_F32_CONVERT_I32_U, STENCIL_F32_CONVERT_I64_S,
    STENCIL_F32_CONVERT_I64_U, STENCIL_F32_COPYSIGN, STENCIL_F32_DEMOTE_F64, STENCIL_F32_DIV,
    STENCIL_F32_EQ, STENCIL_F32_EQ_JUMP_IF, STENCIL_F32_EQ_JUMP_IF_WITH_DROP, STENCIL_F32_FLOOR,
    STENCIL_F32_GE, STENCIL_F32_GE_JUMP_IF, STENCIL_F32_GE_JUMP_IF_WITH_DROP, STENCIL_F32_GT,
    STENCIL_F32_GT_JUMP_IF, STENCIL_F32_GT_JUMP_IF_WITH_DROP, STENCIL_F32_LE,
    STENCIL_F32_LE_JUMP_IF, STENCIL_F32_LE_JUMP_IF_WITH_DROP, STENCIL_F32_LT,
    STENCIL_F32_LT_JUMP_IF, STENCIL_F32_LT_JUMP_IF_WITH_DROP, STENCIL_F32_MAX, STENCIL_F32_MIN,
    STENCIL_F32_MUL, STENCIL_F32_NE, STENCIL_F32_NEAREST, STENCIL_F32_NEG, STENCIL_F32_NE_JUMP_IF,
    STENCIL_F32_NE_JUMP_IF_WITH_DROP, STENCIL_F32_REINTERPRET_I32, STENCIL_F32_SQRT,
    STENCIL_F32_SUB, STENCIL_F32_TRUNC, STENCIL_F64_ABS, STENCIL_F64_ADD, STENCIL_F64_CEIL,
    STENCIL_F64_CONST, STENCIL_F64_CONVERT_I32_S, STENCIL_F64_CONVERT_I32_U,
    STENCIL_F64_CONVERT_I64_S, STENCIL_F64_CONVERT_I64_U, STENCIL_F64_COPYSIGN, STENCIL_F64_DIV,
    STENCIL_F64_EQ, STENCIL_F64_EQ_JUMP_IF, STENCIL_F64_EQ_JUMP_IF_WITH_DROP, STENCIL_F64_FLOOR,
    STENCIL_F64_GE, STENCIL_F64_GE_JUMP_IF, STENCIL_F64_GE_JUMP_IF_WITH_DROP, STENCIL_F64_GT,
    STENCIL_F64_GT_JUMP_IF, STENCIL_F64_GT_JUMP_IF_WITH_DROP, STENCIL_F64_LE,
    STENCIL_F64_LE_JUMP_IF, STENCIL_F64_LE_JUMP_IF_WITH_DROP, STENCIL_F64_LT,
    STENCIL_F64_LT_JUMP_IF, STENCIL_F64_LT_JUMP_IF_WITH_DROP, STENCIL_F64_MAX, STENCIL_F64_MIN,
    STENCIL_F64_MUL, STENCIL_F64_NE, STENCIL_F64_NEAREST, STENCIL_F64_NEG, STENCIL_F64_NE_JUMP_IF,
    STENCIL_F64_NE_JUMP_IF_WITH_DROP, STENCIL_F64_PROMOTE_F32, STENCIL_F64_REINTERPRET_I64,
    STENCIL_F64_SQRT, STENCIL_F64_SUB, STENCIL_F64_TRUNC, STENCIL_GLOBAL_GET, STENCIL_GLOBAL_SET,
    STENCIL_I32_ADD, STENCIL_I32_AND, STENCIL_I32_CLZ, STENCIL_I32_CONST, STENCIL_I32_CTZ,
    STENCIL_I32_DIV_S, STENCIL_I32_DIV_U, STENCIL_I32_EQ, STENCIL_I32_EQZ, STENCIL_I32_EQZ_JUMP_IF,
    STENCIL_I32_EQZ_JUMP_IF_NOT, STENCIL_I32_EQZ_JUMP_IF_NOT_WITH_DROP,
    STENCIL_I32_EQZ_JUMP_IF_WITH_DROP, STENCIL_I32_EQ_JUMP_IF, STENCIL_I32_EQ_JUMP_IF_WITH_DROP,
    STENCIL_I32_EXTEND16_S, STENCIL_I32_EXTEND8_S, STENCIL_I32_GE_S, STENCIL_I32_GE_S_JUMP_IF,
    STENCIL_I32_GE_S_JUMP_IF_WITH_DROP, STENCIL_I32_GE_U, STENCIL_I32_GE_U_JUMP_IF,
    STENCIL_I32_GE_U_JUMP_IF_WITH_DROP, STENCIL_I32_GT_S, STENCIL_I32_GT_S_JUMP_IF,
    STENCIL_I32_GT_S_JUMP_IF_WITH_DROP, STENCIL_I32_GT_U, STENCIL_I32_GT_U_JUMP_IF,
    STENCIL_I32_GT_U_JUMP_IF_WITH_DROP, STENCIL_I32_LE_S, STENCIL_I32_LE_S_JUMP_IF,
    STENCIL_I32_LE_S_JUMP_IF_WITH_DROP, STENCIL_I32_LE_U, STENCIL_I32_LE_U_JUMP_IF,
    STENCIL_I32_LE_U_JUMP_IF_WITH_DROP, STENCIL_I32_LOAD, STENCIL_I32_LOAD16_S,
    STENCIL_I32_LOAD16_U, STENCIL_I32_LOAD8_S, STENCIL_I32_LOAD8_U, STENCIL_I32_LT_S,
    STENCIL_I32_LT_S_JUMP_IF, STENCIL_I32_LT_S_JUMP_IF_WITH_DROP, STENCIL_I32_LT_U,
    STENCIL_I32_LT_U_JUMP_IF, STENCIL_I32_LT_U_JUMP_IF_WITH_DROP, STENCIL_I32_MUL, STENCIL_I32_NE,
    STENCIL_I32_NE_JUMP_IF, STENCIL_I32_NE_JUMP_IF_WITH_DROP, STENCIL_I32_OR, STENCIL_I32_POPCNT,
    STENCIL_I32_REINTERPRET_F32, STENCIL_I32_REM_S, STENCIL_I32_REM_U, STENCIL_I32_ROTL,
    STENCIL_I32_ROTR, STENCIL_I32_SHL, STENCIL_I32_SHR_S, STENCIL_I32_SHR_U, STENCIL_I32_STORE,
    STENCIL_I32_STORE16, STENCIL_I32_STORE8, STENCIL_I32_SUB, STENCIL_I32_TRUNC_F32_S,
    STENCIL_I32_TRUNC_F32_U, STENCIL_I32_TRUNC_F64_S, STENCIL_I32_TRUNC_F64_U,
    STENCIL_I32_TRUNC_SAT_F32_S, STENCIL_I32_TRUNC_SAT_F32_U, STENCIL_I32_TRUNC_SAT_F64_S,
    STENCIL_I32_TRUNC_SAT_F64_U, STENCIL_I32_WRAP_I64, STENCIL_I32_XOR, STENCIL_I64_ADD,
    STENCIL_I64_AND, STENCIL_I64_CLZ, STENCIL_I64_CONST, STENCIL_I64_CTZ, STENCIL_I64_DIV_S,
    STENCIL_I64_DIV_U, STENCIL_I64_EQ, STENCIL_I64_EQZ, STENCIL_I64_EQZ_JUMP_IF,
    STENCIL_I64_EQZ_JUMP_IF_WITH_DROP, STENCIL_I64_EQ_JUMP_IF, STENCIL_I64_EQ_JUMP_IF_WITH_DROP,
    STENCIL_I64_EXTEND16_S, STENCIL_I64_EXTEND32_S, STENCIL_I64_EXTEND8_S,
    STENCIL_I64_EXTEND_I32_S, STENCIL_I64_EXTEND_I32_U, STENCIL_I64_GE_S, STENCIL_I64_GE_S_JUMP_IF,
    STENCIL_I64_GE_S_JUMP_IF_WITH_DROP, STENCIL_I64_GE_U, STENCIL_I64_GE_U_JUMP_IF,
    STENCIL_I64_GE_U_JUMP_IF_WITH_DROP, STENCIL_I64_GT_S, STENCIL_I64_GT_S_JUMP_IF,
    STENCIL_I64_GT_S_JUMP_IF_WITH_DROP, STENCIL_I64_GT_U, STENCIL_I64_GT_U_JUMP_IF,
    STENCIL_I64_GT_U_JUMP_IF_WITH_DROP, STENCIL_I64_LE_S, STENCIL_I64_LE_S_JUMP_IF,
    STENCIL_I64_LE_S_JUMP_IF_WITH_DROP, STENCIL_I64_LE_U, STENCIL_I64_LE_U_JUMP_IF,
    STENCIL_I64_LE_U_JUMP_IF_WITH_DROP, STENCIL_I64_LOAD, STENCIL_I64_LOAD16_S,
    STENCIL_I64_LOAD16_U, STENCIL_I64_LOAD32_S, STENCIL_I64_LOAD32_U, STENCIL_I64_LOAD8_S,
    STENCIL_I64_LOAD8_U, STENCIL_I64_LT_S, STENCIL_I64_LT_S_JUMP_IF,
    STENCIL_I64_LT_S_JUMP_IF_WITH_DROP, STENCIL_I64_LT_U, STENCIL_I64_LT_U_JUMP_IF,
    STENCIL_I64_LT_U_JUMP_IF_WITH_DROP, STENCIL_I64_MUL, STENCIL_I64_NE, STENCIL_I64_NE_JUMP_IF,
    STENCIL_I64_NE_JUMP_IF_WITH_DROP, STENCIL_I64_OR, STENCIL_I64_POPCNT,
    STENCIL_I64_REINTERPRET_F64, STENCIL_I64_REM_S, STENCIL_I64_REM_U, STENCIL_I64_ROTL,
    STENCIL_I64_ROTR, STENCIL_I64_SHL, STENCIL_I64_SHR_S, STENCIL_I64_SHR_U, STENCIL_I64_STORE,
    STENCIL_I64_STORE16, STENCIL_I64_STORE32, STENCIL_I64_STORE8, STENCIL_I64_SUB,
    STENCIL_I64_TRUNC_F32_S, STENCIL_I64_TRUNC_F32_U, STENCIL_I64_TRUNC_F64_S,
    STENCIL_I64_TRUNC_F64_U, STENCIL_I64_TRUNC_SAT_F32_S, STENCIL_I64_TRUNC_SAT_F32_U,
    STENCIL_I64_TRUNC_SAT_F64_S, STENCIL_I64_TRUNC_SAT_F64_U, STENCIL_I64_XOR, STENCIL_JUMP,
    STENCIL_JUMP_IF, STENCIL_JUMP_IF_NOT, STENCIL_JUMP_IF_NOT_WITH_DROP, STENCIL_JUMP_IF_WITH_DROP,
    STENCIL_JUMP_WITH_DROP, STENCIL_LOCAL_GET, STENCIL_LOCAL_GET2, STENCIL_LOCAL_GET_I32_LOAD,
    STENCIL_LOCAL_GET_I32_STORE, STENCIL_LOCAL_GET_I64_LOAD, STENCIL_LOCAL_GET_I64_STORE,
    STENCIL_LOCAL_GET_LOCAL_SET, STENCIL_LOCAL_GET_RETURN, STENCIL_LOCAL_SET, STENCIL_LOCAL_TEE,
    STENCIL_MEMORY_COPY, STENCIL_MEMORY_FILL, STENCIL_MEMORY_INIT, STENCIL_RETURN_VALUE,
    STENCIL_SELECT_VALUE, STENCIL_TABLE_INIT, STENCIL_UNREACHABLE,
};

const fn find_stencil(op: Op) -> Option<&'static Stencil> {
    Some(match op {
        Op::I32Const { .. } => &STENCIL_I32_CONST,
        Op::LocalGet { .. } => &STENCIL_LOCAL_GET,
        Op::LocalSet { .. } => &STENCIL_LOCAL_SET,
        Op::LocalTee { .. } => &STENCIL_LOCAL_TEE,
        Op::LocalGet2 { .. } => &STENCIL_LOCAL_GET2,
        Op::Return => &STENCIL_RETURN_VALUE,
        Op::Drop => &STENCIL_DROP,
        Op::Select => &STENCIL_SELECT_VALUE,
        Op::LocalGetLocalSet { .. } => &STENCIL_LOCAL_GET_LOCAL_SET,
        Op::LocalGetReturn { .. } => &STENCIL_LOCAL_GET_RETURN,
        Op::Call { .. } => &STENCIL_CALL,
        Op::GlobalGet { .. } => &STENCIL_GLOBAL_GET,
        Op::GlobalSet { .. } => &STENCIL_GLOBAL_SET,
        Op::Unreachable => &STENCIL_UNREACHABLE,
        Op::MemoryInit { .. } => &STENCIL_MEMORY_INIT,
        Op::MemoryCopy { .. } => &STENCIL_MEMORY_COPY,
        Op::MemoryFill { .. } => &STENCIL_MEMORY_FILL,
        Op::DataDrop { .. } => &STENCIL_DATA_DROP,
        Op::TableInit { .. } => &STENCIL_TABLE_INIT,
        Op::ElemDrop { .. } => &STENCIL_ELEM_DROP,
        Op::Jump { drop: 0, .. } => &STENCIL_JUMP,
        Op::Jump { .. } => &STENCIL_JUMP_WITH_DROP,
        Op::JumpIf { drop: 0, .. } => &STENCIL_JUMP_IF,
        Op::JumpIf { .. } => &STENCIL_JUMP_IF_WITH_DROP,
        Op::JumpIfNot { drop: 0, .. } => &STENCIL_JUMP_IF_NOT,
        Op::JumpIfNot { .. } => &STENCIL_JUMP_IF_NOT_WITH_DROP,
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
        Op::I32Load { .. } => &STENCIL_I32_LOAD,
        Op::I32Load8Signed { .. } => &STENCIL_I32_LOAD8_S,
        Op::I32Load8Unsigned { .. } => &STENCIL_I32_LOAD8_U,
        Op::I32Load16Signed { .. } => &STENCIL_I32_LOAD16_S,
        Op::I32Load16Unsigned { .. } => &STENCIL_I32_LOAD16_U,
        Op::I32Store { .. } => &STENCIL_I32_STORE,
        Op::I32Store8 { .. } => &STENCIL_I32_STORE8,
        Op::I32Store16 { .. } => &STENCIL_I32_STORE16,
        Op::LocalGetI32Load { .. } => &STENCIL_LOCAL_GET_I32_LOAD,
        Op::LocalGetI32Store { .. } => &STENCIL_LOCAL_GET_I32_STORE,
        Op::I64Load { .. } => &STENCIL_I64_LOAD,
        Op::I64Load8Signed { .. } => &STENCIL_I64_LOAD8_S,
        Op::I64Load8Unsigned { .. } => &STENCIL_I64_LOAD8_U,
        Op::I64Load16Signed { .. } => &STENCIL_I64_LOAD16_S,
        Op::I64Load16Unsigned { .. } => &STENCIL_I64_LOAD16_U,
        Op::I64Load32Signed { .. } => &STENCIL_I64_LOAD32_S,
        Op::I64Load32Unsigned { .. } => &STENCIL_I64_LOAD32_U,
        Op::I64Store { .. } => &STENCIL_I64_STORE,
        Op::I64Store8 { .. } => &STENCIL_I64_STORE8,
        Op::I64Store16 { .. } => &STENCIL_I64_STORE16,
        Op::I64Store32 { .. } => &STENCIL_I64_STORE32,
        Op::LocalGetI64Load { .. } => &STENCIL_LOCAL_GET_I64_LOAD,
        Op::LocalGetI64Store { .. } => &STENCIL_LOCAL_GET_I64_STORE,
        Op::F32Load { .. } => &STENCIL_I32_LOAD,
        Op::LocalGetF32Load { .. } => &STENCIL_LOCAL_GET_I32_LOAD,
        Op::F32Store { .. } => &STENCIL_I32_STORE,
        Op::LocalGetF32Store { .. } => &STENCIL_LOCAL_GET_I32_STORE,
        Op::F64Load { .. } => &STENCIL_I64_LOAD,
        Op::LocalGetF64Load { .. } => &STENCIL_LOCAL_GET_I64_LOAD,
        Op::F64Store { .. } => &STENCIL_I64_STORE,
        Op::LocalGetF64Store { .. } => &STENCIL_LOCAL_GET_I64_STORE,
        Op::I32EqZeroJumpIf { drop: 0, .. } => &STENCIL_I32_EQZ_JUMP_IF,
        Op::I32EqZeroJumpIf { .. } => &STENCIL_I32_EQZ_JUMP_IF_WITH_DROP,
        Op::I32EqZeroJumpIfNot { drop: 0, .. } => &STENCIL_I32_EQZ_JUMP_IF_NOT,
        Op::I32EqZeroJumpIfNot { .. } => &STENCIL_I32_EQZ_JUMP_IF_NOT_WITH_DROP,
        Op::I32EqJumpIf { drop: 0, .. } => &STENCIL_I32_EQ_JUMP_IF,
        Op::I32EqJumpIf { .. } => &STENCIL_I32_EQ_JUMP_IF_WITH_DROP,
        Op::I32NeJumpIf { drop: 0, .. } => &STENCIL_I32_NE_JUMP_IF,
        Op::I32NeJumpIf { .. } => &STENCIL_I32_NE_JUMP_IF_WITH_DROP,
        Op::I32LtSignedJumpIf { drop: 0, .. } => &STENCIL_I32_LT_S_JUMP_IF,
        Op::I32LtSignedJumpIf { .. } => &STENCIL_I32_LT_S_JUMP_IF_WITH_DROP,
        Op::I32LtUnsignedJumpIf { drop: 0, .. } => &STENCIL_I32_LT_U_JUMP_IF,
        Op::I32LtUnsignedJumpIf { .. } => &STENCIL_I32_LT_U_JUMP_IF_WITH_DROP,
        Op::I32GtSignedJumpIf { drop: 0, .. } => &STENCIL_I32_GT_S_JUMP_IF,
        Op::I32GtSignedJumpIf { .. } => &STENCIL_I32_GT_S_JUMP_IF_WITH_DROP,
        Op::I32GtUnsignedJumpIf { drop: 0, .. } => &STENCIL_I32_GT_U_JUMP_IF,
        Op::I32GtUnsignedJumpIf { .. } => &STENCIL_I32_GT_U_JUMP_IF_WITH_DROP,
        Op::I32LeSignedJumpIf { drop: 0, .. } => &STENCIL_I32_LE_S_JUMP_IF,
        Op::I32LeSignedJumpIf { .. } => &STENCIL_I32_LE_S_JUMP_IF_WITH_DROP,
        Op::I32LeUnsignedJumpIf { drop: 0, .. } => &STENCIL_I32_LE_U_JUMP_IF,
        Op::I32LeUnsignedJumpIf { .. } => &STENCIL_I32_LE_U_JUMP_IF_WITH_DROP,
        Op::I32GeSignedJumpIf { drop: 0, .. } => &STENCIL_I32_GE_S_JUMP_IF,
        Op::I32GeSignedJumpIf { .. } => &STENCIL_I32_GE_S_JUMP_IF_WITH_DROP,
        Op::I32GeUnsignedJumpIf { drop: 0, .. } => &STENCIL_I32_GE_U_JUMP_IF,
        Op::I32GeUnsignedJumpIf { .. } => &STENCIL_I32_GE_U_JUMP_IF_WITH_DROP,
        Op::I64EqZeroJumpIf { drop: 0, .. } => &STENCIL_I64_EQZ_JUMP_IF,
        Op::I64EqZeroJumpIf { .. } => &STENCIL_I64_EQZ_JUMP_IF_WITH_DROP,
        Op::I64EqJumpIf { drop: 0, .. } => &STENCIL_I64_EQ_JUMP_IF,
        Op::I64EqJumpIf { .. } => &STENCIL_I64_EQ_JUMP_IF_WITH_DROP,
        Op::I64NeJumpIf { drop: 0, .. } => &STENCIL_I64_NE_JUMP_IF,
        Op::I64NeJumpIf { .. } => &STENCIL_I64_NE_JUMP_IF_WITH_DROP,
        Op::I64LtSignedJumpIf { drop: 0, .. } => &STENCIL_I64_LT_S_JUMP_IF,
        Op::I64LtSignedJumpIf { .. } => &STENCIL_I64_LT_S_JUMP_IF_WITH_DROP,
        Op::I64LtUnsignedJumpIf { drop: 0, .. } => &STENCIL_I64_LT_U_JUMP_IF,
        Op::I64LtUnsignedJumpIf { .. } => &STENCIL_I64_LT_U_JUMP_IF_WITH_DROP,
        Op::I64GtSignedJumpIf { drop: 0, .. } => &STENCIL_I64_GT_S_JUMP_IF,
        Op::I64GtSignedJumpIf { .. } => &STENCIL_I64_GT_S_JUMP_IF_WITH_DROP,
        Op::I64GtUnsignedJumpIf { drop: 0, .. } => &STENCIL_I64_GT_U_JUMP_IF,
        Op::I64GtUnsignedJumpIf { .. } => &STENCIL_I64_GT_U_JUMP_IF_WITH_DROP,
        Op::I64LeSignedJumpIf { drop: 0, .. } => &STENCIL_I64_LE_S_JUMP_IF,
        Op::I64LeSignedJumpIf { .. } => &STENCIL_I64_LE_S_JUMP_IF_WITH_DROP,
        Op::I64LeUnsignedJumpIf { drop: 0, .. } => &STENCIL_I64_LE_U_JUMP_IF,
        Op::I64LeUnsignedJumpIf { .. } => &STENCIL_I64_LE_U_JUMP_IF_WITH_DROP,
        Op::I64GeSignedJumpIf { drop: 0, .. } => &STENCIL_I64_GE_S_JUMP_IF,
        Op::I64GeSignedJumpIf { .. } => &STENCIL_I64_GE_S_JUMP_IF_WITH_DROP,
        Op::I64GeUnsignedJumpIf { drop: 0, .. } => &STENCIL_I64_GE_U_JUMP_IF,
        Op::I64GeUnsignedJumpIf { .. } => &STENCIL_I64_GE_U_JUMP_IF_WITH_DROP,
        Op::F32EqJumpIf { drop: 0, .. } => &STENCIL_F32_EQ_JUMP_IF,
        Op::F32EqJumpIf { .. } => &STENCIL_F32_EQ_JUMP_IF_WITH_DROP,
        Op::F32NeJumpIf { drop: 0, .. } => &STENCIL_F32_NE_JUMP_IF,
        Op::F32NeJumpIf { .. } => &STENCIL_F32_NE_JUMP_IF_WITH_DROP,
        Op::F32LtJumpIf { drop: 0, .. } => &STENCIL_F32_LT_JUMP_IF,
        Op::F32LtJumpIf { .. } => &STENCIL_F32_LT_JUMP_IF_WITH_DROP,
        Op::F32GtJumpIf { drop: 0, .. } => &STENCIL_F32_GT_JUMP_IF,
        Op::F32GtJumpIf { .. } => &STENCIL_F32_GT_JUMP_IF_WITH_DROP,
        Op::F32LeJumpIf { drop: 0, .. } => &STENCIL_F32_LE_JUMP_IF,
        Op::F32LeJumpIf { .. } => &STENCIL_F32_LE_JUMP_IF_WITH_DROP,
        Op::F32GeJumpIf { drop: 0, .. } => &STENCIL_F32_GE_JUMP_IF,
        Op::F32GeJumpIf { .. } => &STENCIL_F32_GE_JUMP_IF_WITH_DROP,
        Op::F64EqJumpIf { drop: 0, .. } => &STENCIL_F64_EQ_JUMP_IF,
        Op::F64EqJumpIf { .. } => &STENCIL_F64_EQ_JUMP_IF_WITH_DROP,
        Op::F64NeJumpIf { drop: 0, .. } => &STENCIL_F64_NE_JUMP_IF,
        Op::F64NeJumpIf { .. } => &STENCIL_F64_NE_JUMP_IF_WITH_DROP,
        Op::F64LtJumpIf { drop: 0, .. } => &STENCIL_F64_LT_JUMP_IF,
        Op::F64LtJumpIf { .. } => &STENCIL_F64_LT_JUMP_IF_WITH_DROP,
        Op::F64GtJumpIf { drop: 0, .. } => &STENCIL_F64_GT_JUMP_IF,
        Op::F64GtJumpIf { .. } => &STENCIL_F64_GT_JUMP_IF_WITH_DROP,
        Op::F64LeJumpIf { drop: 0, .. } => &STENCIL_F64_LE_JUMP_IF,
        Op::F64LeJumpIf { .. } => &STENCIL_F64_LE_JUMP_IF_WITH_DROP,
        Op::F64GeJumpIf { drop: 0, .. } => &STENCIL_F64_GE_JUMP_IF,
        Op::F64GeJumpIf { .. } => &STENCIL_F64_GE_JUMP_IF_WITH_DROP,
        _ => return None,
    })
}

pub struct JitCompiledFunction {
    code: CodeBuffer,
    code_offsets: Vec<usize>,
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
    runtime: *mut core::ffi::c_void,
    bulk_operation: BulkOperation,
    memories: *const MemoryView,
    globals: *const *mut u64,
    exit_reason: u64,
    call_function: u64,
    resume_pc: u64,
}

pub(crate) enum JitExit {
    Returned {
        stack_height: usize,
    },
    Call {
        stack_height: usize,
        function: u32,
        resume_pc: usize,
    },
}

impl JitCompiledFunction {
    pub const fn result_count(&self) -> usize {
        self.result_count
    }

    pub(crate) fn execute_vm(
        &self,
        locals: &mut [crate::RawValue],
        stack: &mut [crate::RawValue],
        runtime: &mut RuntimeContext<'_>,
        pc: usize,
        stack_height: usize,
    ) -> Result<JitExit> {
        unsafe {
            let locals =
                core::slice::from_raw_parts_mut(locals.as_mut_ptr().cast::<u64>(), locals.len());

            let stack =
                core::slice::from_raw_parts_mut(stack.as_mut_ptr().cast::<u64>(), stack.len());

            self.execute_with_runtime(locals, stack, Some(runtime), pc, stack_height)
        }
    }

    #[cfg(test)]
    pub(super) fn execute(&self, locals: &mut [u64], stack: &mut [u64]) -> Result<usize> {
        match self.execute_with_runtime(locals, stack, None, 0, 0)? {
            JitExit::Returned { stack_height } => Ok(stack_height),
            JitExit::Call { .. } => bail_jit!("calls must be coordinated by the store"),
        }
    }

    fn execute_with_runtime(
        &self,
        locals: &mut [u64],
        stack: &mut [u64],
        runtime: Option<&mut RuntimeContext<'_>>,
        pc: usize,
        stack_height: usize,
    ) -> Result<JitExit> {
        ensure_jit!(
            locals.len() as u64 >= self.required_locals,
            "not enough locals"
        );

        ensure_jit!(
            stack.len() >= self.max_stack_height,
            "operand stack is too small"
        );

        // Keep the common single-memory case on the native stack. Views are
        // rebuilt for every entry and are never persisted in code or snapshots.
        let mut runtime = runtime;
        let mut inline_views = [MemoryView::EMPTY];
        let mut many_views = Vec::new();

        let memories = if let Some(runtime) = runtime.as_deref_mut() {
            match runtime.module.mem_addrs.len() {
                0 => core::ptr::null(),
                1 => {
                    inline_views[0] = runtime.memory_view(0);
                    inline_views.as_ptr()
                }
                count => {
                    many_views.extend((0..count).map(|index| runtime.memory_view(index)));
                    many_views.as_ptr()
                }
            }
        } else {
            core::ptr::null()
        };

        let mut inline_globals = [core::ptr::null_mut(); 8];
        let mut many_globals = Vec::new();

        let globals = if let Some(runtime) = runtime.as_deref_mut() {
            let count = runtime.module.global_addrs.len();
            let views = if count <= inline_globals.len() {
                &mut inline_globals[..count]
            } else {
                many_globals.resize(count, core::ptr::null_mut());
                many_globals.as_mut_slice()
            };

            runtime.global_values(views);
            views.as_ptr()
        } else {
            core::ptr::null()
        };

        let mut context = VMContext {
            locals: locals.as_mut_ptr(),
            stack: stack.as_mut_ptr(),
            stack_height: stack_height as u64,
            trap: 0,
            runtime: runtime.map_or(core::ptr::null_mut(), |context| {
                (context as *mut RuntimeContext<'_>).cast()
            }),
            bulk_operation: runtime::bulk_operation,
            memories,
            globals,
            exit_reason: 0,
            call_function: 0,
            resume_pc: 0,
        };

        type Entry = unsafe extern "C" fn(*mut VMContext);

        let offset = *self
            .code_offsets
            .get(pc)
            .ok_or_else(|| Error::jit("native resume PC is outside function"))?;

        unsafe {
            let entry: Entry = core::mem::transmute(self.code.as_ptr().add(offset));
            entry(&mut context);
        }

        match context.trap {
            0 => {}
            1 => bail_trap!(Trap::IntegerDivideByZero),
            2 => bail_trap!(Trap::IntegerOverflow),
            3 => bail_trap!(Trap::InvalidConversionToInteger),
            4 => bail_trap!(Trap::OutOfBoundsMemoryAccess),
            5 => bail_trap!(Trap::OutOfBoundsTableAccess),
            6 => bail_trap!(Trap::Unreachable),
            255 => bail_jit!("bulk operation needs a valid runtime context"),
            code => bail_jit!("unknown trap code {code}"),
        }

        let stack_height = context.stack_height as usize;

        match context.exit_reason {
            0 => Ok(JitExit::Returned { stack_height }),
            1 => Ok(JitExit::Call {
                stack_height,
                function: context.call_function as u32,
                resume_pc: context.resume_pc as usize,
            }),
            code => bail_jit!("unknown native exit reason {code}"),
        }
    }
}

pub fn supports(ops: &[Op]) -> bool {
    ops.iter().all(|&op| find_stencil(op).is_some())
}

pub fn compile(
    ops: &[Op],
    max_stack_height: usize,
    result_count: usize,
) -> Result<JitCompiledFunction> {
    let mut instances = Vec::with_capacity(ops.len());

    let mut required_locals = 0_u64;

    for (pc, &op) in ops.iter().enumerate() {
        let stencil = find_stencil(op)
            .ok_or_else(|| Error::jit(format!("unsupported operation at IR {pc}: {op:?}")))?;

        let parameters = match op {
            Op::Call { func_i } => vec![u64::from(func_i), (pc + 1) as u64],
            Op::GlobalGet { global_i } | Op::GlobalSet { global_i } => {
                vec![u64::from(global_i)]
            }
            Op::MemoryCopy {
                dst_memory_i,
                src_memory_i,
            } => {
                vec![u64::from(src_memory_i), u64::from(dst_memory_i)]
            }
            Op::MemoryFill { memory_i } => vec![u64::from(memory_i)],
            Op::MemoryInit { data_i, memory_i } => vec![u64::from(data_i), u64::from(memory_i)],
            Op::TableInit { elem_i, table_i } => vec![u64::from(elem_i), u64::from(table_i)],
            Op::DataDrop { data_i } => vec![u64::from(data_i)],
            Op::ElemDrop { elem_i } => vec![u64::from(elem_i)],
            Op::I32Load { offset, memory }
            | Op::I32Load8Signed { offset, memory }
            | Op::I32Load8Unsigned { offset, memory }
            | Op::I32Load16Signed { offset, memory }
            | Op::I32Load16Unsigned { offset, memory }
            | Op::I32Store { offset, memory }
            | Op::I32Store8 { offset, memory }
            | Op::I32Store16 { offset, memory }
            | Op::I64Load { offset, memory }
            | Op::I64Load8Signed { offset, memory }
            | Op::I64Load8Unsigned { offset, memory }
            | Op::I64Load16Signed { offset, memory }
            | Op::I64Load16Unsigned { offset, memory }
            | Op::I64Load32Signed { offset, memory }
            | Op::I64Load32Unsigned { offset, memory }
            | Op::I64Store { offset, memory }
            | Op::I64Store8 { offset, memory }
            | Op::I64Store16 { offset, memory }
            | Op::I64Store32 { offset, memory }
            | Op::F32Load { offset, memory }
            | Op::F32Store { offset, memory }
            | Op::F64Load { offset, memory }
            | Op::F64Store { offset, memory } => {
                vec![u64::from(memory), u64::from(offset)]
            }
            Op::LocalGetI32Load {
                local_i,
                offset,
                memory,
            }
            | Op::LocalGetI32Store {
                local_i,
                offset,
                memory,
            }
            | Op::LocalGetI64Load {
                local_i,
                offset,
                memory,
            }
            | Op::LocalGetI64Store {
                local_i,
                offset,
                memory,
            }
            | Op::LocalGetF32Load {
                local_i,
                offset,
                memory,
            }
            | Op::LocalGetF32Store {
                local_i,
                offset,
                memory,
            }
            | Op::LocalGetF64Load {
                local_i,
                offset,
                memory,
            }
            | Op::LocalGetF64Store {
                local_i,
                offset,
                memory,
            } => {
                required_locals = required_locals.max(u64::from(local_i) + 1);

                vec![u64::from(memory), u64::from(offset), u64::from(local_i)]
            }
            Op::I32Const { value } => vec![u64::from(value as u32)],
            Op::I64Const { value } => vec![value as u64],
            Op::F32Const { value } => vec![u64::from(value.to_bits())],
            Op::F64Const { value } => vec![value.to_bits()],
            Op::LocalGet { local_i }
            | Op::LocalSet { local_i }
            | Op::LocalTee { local_i }
            | Op::LocalGetReturn { local_i } => {
                required_locals = required_locals.max(u64::from(local_i) + 1);

                vec![u64::from(local_i)]
            }
            Op::LocalGetLocalSet {
                local_get_i,
                local_set_i,
            } => {
                required_locals = required_locals.max(u64::from(local_get_i.max(local_set_i)) + 1);

                vec![u64::from(local_get_i), u64::from(local_set_i)]
            }
            Op::LocalGet2 {
                local_i_a,
                local_i_b,
            } => {
                required_locals = required_locals.max(u64::from(local_i_a.max(local_i_b)) + 1);

                vec![u64::from(local_i_a), u64::from(local_i_b)]
            }
            Op::Jump { keep, drop, .. }
            | Op::JumpIf { keep, drop, .. }
            | Op::JumpIfNot { keep, drop, .. }
            | Op::I32EqZeroJumpIf { keep, drop, .. }
            | Op::I32EqZeroJumpIfNot { keep, drop, .. }
            | Op::I32EqJumpIf { keep, drop, .. }
            | Op::I32NeJumpIf { keep, drop, .. }
            | Op::I32LtSignedJumpIf { keep, drop, .. }
            | Op::I32LtUnsignedJumpIf { keep, drop, .. }
            | Op::I32GtSignedJumpIf { keep, drop, .. }
            | Op::I32GtUnsignedJumpIf { keep, drop, .. }
            | Op::I32LeSignedJumpIf { keep, drop, .. }
            | Op::I32LeUnsignedJumpIf { keep, drop, .. }
            | Op::I32GeSignedJumpIf { keep, drop, .. }
            | Op::I32GeUnsignedJumpIf { keep, drop, .. }
            | Op::I64EqZeroJumpIf { keep, drop, .. }
            | Op::I64EqJumpIf { keep, drop, .. }
            | Op::I64NeJumpIf { keep, drop, .. }
            | Op::I64LtSignedJumpIf { keep, drop, .. }
            | Op::I64LtUnsignedJumpIf { keep, drop, .. }
            | Op::I64GtSignedJumpIf { keep, drop, .. }
            | Op::I64GtUnsignedJumpIf { keep, drop, .. }
            | Op::I64LeSignedJumpIf { keep, drop, .. }
            | Op::I64LeUnsignedJumpIf { keep, drop, .. }
            | Op::I64GeSignedJumpIf { keep, drop, .. }
            | Op::I64GeUnsignedJumpIf { keep, drop, .. }
            | Op::F32EqJumpIf { keep, drop, .. }
            | Op::F32NeJumpIf { keep, drop, .. }
            | Op::F32LtJumpIf { keep, drop, .. }
            | Op::F32GtJumpIf { keep, drop, .. }
            | Op::F32LeJumpIf { keep, drop, .. }
            | Op::F32GeJumpIf { keep, drop, .. }
            | Op::F64EqJumpIf { keep, drop, .. }
            | Op::F64NeJumpIf { keep, drop, .. }
            | Op::F64LtJumpIf { keep, drop, .. }
            | Op::F64GtJumpIf { keep, drop, .. }
            | Op::F64LeJumpIf { keep, drop, .. }
            | Op::F64GeJumpIf { keep, drop, .. }
                if drop != 0 =>
            {
                vec![u64::from(keep), u64::from(drop)]
            }
            _ => Vec::new(),
        };

        let target = op.jump_target().map(|target| target as usize);

        instances.push(StencilInstance {
            stencil,
            parameters,
            target,
        });
    }

    let layout = CodeLayout::try_new(&instances)?;

    Ok(JitCompiledFunction {
        code: CodeBuffer::new(&layout)?,
        code_offsets: layout.code_offsets,
        required_locals,
        max_stack_height,
        result_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_removes_only_the_top_value() {
        let ops = [
            Op::I32Const { value: 10 },
            Op::I32Const { value: 999 },
            Op::Drop,
            Op::I32Const { value: 32 },
            Op::I32Add,
            Op::Return,
        ];

        assert!(supports(&ops));

        let function = compile(&ops, 2, 1).unwrap();
        let mut stack = [0; 2];

        assert_eq!(function.execute(&mut [], &mut stack).unwrap(), 1);
        assert_eq!(stack[0], 42);
    }

    #[test]
    fn select_preserves_bits_and_uses_an_i32_condition() {
        let ops = [
            Op::LocalGet2 {
                local_i_a: 0,
                local_i_b: 1,
            },
            Op::LocalGet { local_i: 2 },
            Op::Select,
            Op::Return,
        ];

        assert!(supports(&ops));

        let function = compile(&ops, 3, 1).unwrap();

        for condition in [0_u64, 1, u32::MAX as u64, 1_u64 << 32] {
            let first = 0x7ff8_0000_0000_1234;
            let second = 0x8000_0000_0000_0000;
            let mut locals = [first, second, condition];
            let mut stack = [0; 3];

            assert_eq!(function.execute(&mut locals, &mut stack).unwrap(), 1);
            assert_eq!(stack[0], if condition as u32 != 0 { first } else { second });
        }
    }

    #[test]
    fn fused_local_copy_leaves_operand_stack_unchanged() {
        for destination in [1, 3] {
            let ops = [
                Op::I32Const { value: 42 },
                Op::LocalGetLocalSet {
                    local_get_i: 1,
                    local_set_i: destination,
                },
                Op::Return,
            ];

            assert!(supports(&ops));

            let function = compile(&ops, 1, 1).unwrap();
            let value = 0x89ab_cdef_0123_4567;
            let mut locals = [0, value, 0, 0];
            let mut stack = [0];

            assert_eq!(function.required_locals, u64::from(destination.max(1)) + 1);
            assert_eq!(function.execute(&mut locals, &mut stack).unwrap(), 1);
            assert_eq!(stack[0], 42);
            assert_eq!(locals[destination as usize], value);
        }
    }

    #[test]
    fn fused_local_return_exits_immediately() {
        let ops = [
            Op::I32Const { value: 99 },
            Op::LocalGetReturn { local_i: 3 },
            Op::I32Const { value: 777 },
            Op::Return,
        ];

        assert!(supports(&ops));

        let function = compile(&ops, 2, 1).unwrap();
        let mut locals = [0, 0, 0, 42];
        let mut stack = [0; 2];

        // The store's normal return handling keeps the result and drops the 99.
        assert_eq!(function.execute(&mut locals, &mut stack).unwrap(), 2);
        assert_eq!(stack, [99, 42]);
    }

    #[test]
    fn fused_local_return_works_as_a_callee() {
        let bytes = wat::parse_str(
            r#"
            (module
                (func $identity (param i32) (result i32) local.get 0)
                (func (export "run") (result i32)
                    i32.const 10 i32.const 32 call $identity i32.add))
        "#,
        )
        .unwrap();
        let module = crate::Module::try_new(&bytes).unwrap();
        let mut store = crate::Store::new();
        let instance = store.instantiate(&module, vec![]).unwrap();
        let output = store
            .invoke_no_args(instance, "run")
            .unwrap()
            .into_completed()
            .unwrap();

        assert_eq!(output[0].as_i32(), 42);
    }

    #[test]
    fn conditional_jumps_take_target_or_continue() {
        for invert in [false, true] {
            let branch = if invert {
                Op::JumpIfNot {
                    target: 4,
                    keep: 0,
                    drop: 0,
                }
            } else {
                Op::JumpIf {
                    target: 4,
                    keep: 0,
                    drop: 0,
                }
            };
            let ops = [
                Op::LocalGet { local_i: 0 },
                branch,
                Op::I32Const { value: 10 },
                Op::Return,
                Op::I32Const { value: 20 },
                Op::Return,
            ];

            assert!(supports(&ops));

            let function = compile(&ops, 1, 1).unwrap();

            for condition in [0_u64, 1, u32::MAX as u64, 1_u64 << 32] {
                let taken = (condition as u32 != 0) != invert;
                let mut locals = [condition];
                let mut stack = [0];

                assert_eq!(function.execute(&mut locals, &mut stack).unwrap(), 1);
                assert_eq!(stack[0], if taken { 20 } else { 10 });
            }
        }
    }

    #[test]
    fn conditional_jumps_adjust_stack_only_when_taken() {
        for invert in [false, true] {
            let branch = if invert {
                Op::JumpIfNot {
                    target: 9,
                    keep: 3,
                    drop: 2,
                }
            } else {
                Op::JumpIf {
                    target: 9,
                    keep: 3,
                    drop: 2,
                }
            };
            let ops = [
                Op::I32Const { value: 100 },
                Op::I32Const { value: 200 },
                Op::I32Const { value: 10 },
                Op::I32Const { value: 20 },
                Op::I32Const { value: 30 },
                Op::LocalGet { local_i: 0 },
                branch,
                Op::I32Add,
                Op::I32Add,
                Op::I32Add,
                Op::I32Add,
                Op::Return,
            ];

            let function = compile(&ops, 6, 1).unwrap();

            for condition in [0_u64, 1] {
                let taken = (condition != 0) != invert;
                let mut locals = [condition];
                let mut stack = [0; 6];

                assert_eq!(function.execute(&mut locals, &mut stack).unwrap(), 1);
                assert_eq!(stack[0], if taken { 60 } else { 360 });
            }
        }
    }

    #[test]
    fn conditional_backedges_execute_countdown_loops() {
        for invert in [false, true] {
            let mut ops = vec![
                Op::LocalGet { local_i: 0 },
                Op::I32Const { value: 1 },
                Op::I32Sub,
                Op::LocalTee { local_i: 0 },
            ];

            if invert {
                ops.push(Op::I32EqZero);
                ops.push(Op::JumpIfNot {
                    target: 0,
                    keep: 0,
                    drop: 0,
                });
            } else {
                ops.push(Op::JumpIf {
                    target: 0,
                    keep: 0,
                    drop: 0,
                });
            }

            ops.extend_from_slice(&[Op::LocalGet { local_i: 0 }, Op::Return]);

            let function = compile(&ops, 2, 1).unwrap();
            let mut locals = [10_000];
            let mut stack = [0; 2];

            assert_eq!(function.execute(&mut locals, &mut stack).unwrap(), 1);
            assert_eq!(stack[0], 0);
            assert_eq!(locals[0], 0);
        }
    }

    #[test]
    fn jump_skips_unreachable_instructions() {
        let ops = [
            Op::I32Const { value: 20 },
            Op::Jump {
                target: 4,
                keep: 1,
                drop: 0,
            },
            Op::I32Add,
            Op::LocalSet { local_i: 0 },
            Op::I32Const { value: 22 },
            Op::I32Add,
            Op::Return,
        ];

        assert!(supports(&ops));

        let function = compile(&ops, 2, 1).unwrap();
        let mut locals = [99];
        let mut stack = [0; 2];

        assert_eq!(function.max_stack_height, 2);
        assert_eq!(function.execute(&mut locals, &mut stack).unwrap(), 1);
        assert_eq!(stack[0], 42);
        assert_eq!(locals[0], 99);
    }

    #[test]
    fn jump_can_target_an_earlier_instruction() {
        let ops = [
            Op::Jump {
                target: 3,
                keep: 0,
                drop: 0,
            },
            Op::LocalGet { local_i: 0 },
            Op::Return,
            Op::I32Const { value: 42 },
            Op::LocalSet { local_i: 0 },
            Op::Jump {
                target: 1,
                keep: 0,
                drop: 0,
            },
            Op::Return,
        ];

        let function = compile(&ops, 1, 1).unwrap();
        let mut locals = [0];
        let mut stack = [0];

        assert_eq!(function.execute(&mut locals, &mut stack).unwrap(), 1);
        assert_eq!(stack[0], 42);
    }

    #[test]
    fn jump_preserves_kept_values_when_dropping_stack_entries() {
        let ops = [
            Op::I32Const { value: 100 },
            Op::I32Const { value: 200 },
            Op::I32Const { value: 10 },
            Op::I32Const { value: 20 },
            Op::I32Const { value: 30 },
            Op::Jump {
                target: 7,
                keep: 3,
                drop: 2,
            },
            Op::I32Const { value: 99 },
            Op::I32Add,
            Op::I32Add,
            Op::Return,
        ];

        let function = compile(&ops, 5, 1).unwrap();
        let mut stack = [0; 5];

        assert_eq!(function.execute(&mut [], &mut stack).unwrap(), 1);
        assert_eq!(stack[0], 60);
    }

    #[test]
    fn rejects_invalid_jump_target() {
        let ops = [
            Op::Jump {
                target: 2,
                keep: 0,
                drop: 0,
            },
            Op::Return,
        ];

        assert!(matches!(compile(&ops, 0, 0), Err(Error::Jit(_))));
    }

    #[test]
    fn uses_metadata_without_requiring_a_reachable_return() {
        let ops = [
            Op::Jump {
                target: 0,
                keep: 0,
                drop: 0,
            },
            Op::Return,
        ];

        let function = compile(&ops, 0, 1).unwrap();

        assert_eq!(function.max_stack_height, 0);
        assert_eq!(function.result_count(), 1);
    }

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

            let function = compile(&ops, 2, 1).unwrap();

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

        let function = compile(&ops, 1, 1).unwrap();

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

        let function = compile(&ops, 1, 1).unwrap();

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

            let function = compile(&ops, 2, 1).unwrap();

            let mut stack = [0_u64; 2];

            assert_eq!(function.execute(&mut [], &mut stack).unwrap(), 1);
            assert_eq!(stack[0], expected);
        }
    }
}
