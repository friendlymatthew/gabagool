use crate::binary_grammar::{HeapType, ValueType};

const _: () = assert!(std::mem::size_of::<Op>() <= 16);

#[derive(Debug, Clone, Copy)]
pub struct JumpTableEntry {
    pub target: u32,
    pub drop: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatchKind {
    Catch,
    CatchRef,
    CatchAll,
    CatchAllRef,
}

#[derive(Debug, Clone, Copy)]
pub struct CompiledCatchClause {
    pub kind: CatchKind,
    pub tag_idx: u32,
    pub target: u32,
    pub n_values: u16,
    pub drop: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilerMode {
    Optimize,
    Debug,
}

#[derive(Debug, Clone)]
pub struct CompiledFunction {
    pub ops: Vec<Op>,
    pub type_index: u32,
    pub num_args: u32,
    // contains [args; num_args] [...local_types]
    pub local_types: Vec<ValueType>,
    pub(crate) max_stack_height: u32,
    /// maps each compiled op to the source instruction index that produced it
    pub source_positions: Vec<u32>,
}

#[repr(u16)]
#[derive(Clone, Copy, Debug)]
pub enum Op {
    Unreachable,
    Nop,
    Return,
    Jump {
        target: u32,
        keep: u16,
        drop: u16,
    },
    JumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    JumpIfNot {
        target: u32,
        keep: u16,
        drop: u16,
    },
    JumpTable {
        index: u32,
        keep: u16,
    },
    BrOnNull {
        target: u32,
        keep: u16,
        drop: u16,
    },
    BrOnNonNull {
        target: u32,
        keep: u16,
        drop: u16,
    },
    Call {
        func_idx: u32,
    },
    CallIndirect {
        type_idx: u32,
        table_idx: u32,
    },
    ReturnCall {
        func_idx: u32,
    },
    ReturnCallIndirect {
        type_idx: u32,
        table_idx: u32,
    },
    CallRef {
        type_idx: u32,
    },
    ReturnCallRef {
        type_idx: u32,
    },
    I32Const {
        value: i32,
    },
    I64Const {
        value: i64,
    },
    F32Const {
        value: f32,
    },
    F64Const {
        value: f64,
    },
    V128Const {
        table_idx: u32,
    },
    LocalGet {
        local_idx: u32,
    },
    LocalSet {
        local_idx: u32,
    },
    LocalTee {
        local_idx: u32,
    },
    GlobalGet {
        global_idx: u32,
    },
    GlobalSet {
        global_idx: u32,
    },
    Drop,
    Select,
    RefNull(HeapType),
    RefIsNull,
    RefEq,
    RefAsNonNull,
    RefFunc {
        func_idx: u32,
    },
    Throw {
        tag_idx: u32,
    },
    ThrowRef,
    TryCatchPush {
        handler_idx: u32,
    },
    TryCatchPop,
    TableGet {
        table_idx: u32,
    },
    TableSet {
        table_idx: u32,
    },
    TableInit {
        elem_idx: u32,
        table_idx: u32,
    },
    ElemDrop {
        elem_idx: u32,
    },
    TableCopy {
        dst_table_idx: u32,
        src_table_idx: u32,
    },
    TableGrow {
        table_idx: u32,
    },
    TableSize {
        table_idx: u32,
    },
    TableFill {
        table_idx: u32,
    },
    I32Load {
        offset: u32,
        memory: u32,
    },
    I64Load {
        offset: u32,
        memory: u32,
    },
    F32Load {
        offset: u32,
        memory: u32,
    },
    F64Load {
        offset: u32,
        memory: u32,
    },
    I32Load8Signed {
        offset: u32,
        memory: u32,
    },
    I32Load8Unsigned {
        offset: u32,
        memory: u32,
    },
    I32Load16Signed {
        offset: u32,
        memory: u32,
    },
    I32Load16Unsigned {
        offset: u32,
        memory: u32,
    },
    I64Load8Signed {
        offset: u32,
        memory: u32,
    },
    I64Load8Unsigned {
        offset: u32,
        memory: u32,
    },
    I64Load16Signed {
        offset: u32,
        memory: u32,
    },
    I64Load16Unsigned {
        offset: u32,
        memory: u32,
    },
    I64Load32Signed {
        offset: u32,
        memory: u32,
    },
    I64Load32Unsigned {
        offset: u32,
        memory: u32,
    },
    I32Store {
        offset: u32,
        memory: u32,
    },
    I64Store {
        offset: u32,
        memory: u32,
    },
    F32Store {
        offset: u32,
        memory: u32,
    },
    F64Store {
        offset: u32,
        memory: u32,
    },
    I32Store8 {
        offset: u32,
        memory: u32,
    },
    I32Store16 {
        offset: u32,
        memory: u32,
    },
    I64Store8 {
        offset: u32,
        memory: u32,
    },
    I64Store16 {
        offset: u32,
        memory: u32,
    },
    I64Store32 {
        offset: u32,
        memory: u32,
    },
    MemorySize {
        memory_idx: u32,
    },
    MemoryGrow {
        memory_idx: u32,
    },
    MemoryInit {
        data_idx: u32,
        memory_idx: u32,
    },
    DataDrop {
        data_idx: u32,
    },
    MemoryCopy {
        dst_memory_idx: u32,
        src_memory_idx: u32,
    },
    MemoryFill {
        memory_idx: u32,
    },
    I32EqZero,
    I32Eq,
    I32Ne,
    I32LtSigned,
    I32LtUnsigned,
    I32GtSigned,
    I32GtUnsigned,
    I32LeSigned,
    I32LeUnsigned,
    I32GeSigned,
    I32GeUnsigned,
    I64EqZero,
    I64Eq,
    I64Ne,
    I64LtSigned,
    I64LtUnsigned,
    I64GtSigned,
    I64GtUnsigned,
    I64LeSigned,
    I64LeUnsigned,
    I64GeSigned,
    I64GeUnsigned,
    F32Eq,
    F32Ne,
    F32Lt,
    F32Gt,
    F32Le,
    F32Ge,
    F64Eq,
    F64Ne,
    F64Lt,
    F64Gt,
    F64Le,
    F64Ge,
    I32CountLeadingZeros,
    I32CountTrailingZeros,
    I32PopCount,
    I32Add,
    I32Sub,
    I32Mul,
    I32DivSigned,
    I32DivUnsigned,
    I32RemainderSigned,
    I32RemainderUnsigned,
    I32And,
    I32Or,
    I32Xor,
    I32Shl,
    I32ShrSigned,
    I32ShrUnsigned,
    I32RotateLeft,
    I32RotateRight,
    I64CountLeadingZeros,
    I64CountTrailingZeros,
    I64PopCount,
    I64Add,
    I64Sub,
    I64Mul,
    I64DivSigned,
    I64DivUnsigned,
    I64RemainderSigned,
    I64RemainderUnsigned,
    I64And,
    I64Or,
    I64Xor,
    I64Shl,
    I64ShrSigned,
    I64ShrUnsigned,
    I64RotateLeft,
    I64RotateRight,
    F32Abs,
    F32Neg,
    F32Ceil,
    F32Floor,
    F32Trunc,
    F32Nearest,
    F32Sqrt,
    F32Add,
    F32Sub,
    F32Mul,
    F32Div,
    F32Min,
    F32Max,
    F32CopySign,
    F64Abs,
    F64Neg,
    F64Ceil,
    F64Floor,
    F64Trunc,
    F64Nearest,
    F64Sqrt,
    F64Add,
    F64Sub,
    F64Mul,
    F64Div,
    F64Min,
    F64Max,
    F64CopySign,
    I32WrapI64,
    I32TruncF32Signed,
    I32TruncF32Unsigned,
    I32TruncF64Signed,
    I32TruncF64Unsigned,
    I64ExtendI32Signed,
    I64ExtendI32Unsigned,
    I64TruncF32Signed,
    I64TruncF32Unsigned,
    I64TruncF64Signed,
    I64TruncF64Unsigned,
    F32ConvertI32Signed,
    F32ConvertI32Unsigned,
    F32ConvertI64Signed,
    F32ConvertI64Unsigned,
    F32DemoteF64,
    F64ConvertI32Signed,
    F64ConvertI32Unsigned,
    F64ConvertI64Signed,
    F64ConvertI64Unsigned,
    F64PromoteF32,
    I32ReinterpretF32,
    I64ReinterpretF64,
    F32ReinterpretI32,
    F64ReinterpretI64,
    I32Extend8Signed,
    I32Extend16Signed,
    I64Extend8Signed,
    I64Extend16Signed,
    I64Extend32Signed,
    I32TruncSaturatedF32Signed,
    I32TruncSaturatedF32Unsigned,
    I32TruncSaturatedF64Signed,
    I32TruncSaturatedF64Unsigned,
    I64TruncSaturatedF32Signed,
    I64TruncSaturatedF32Unsigned,
    I64TruncSaturatedF64Signed,
    I64TruncSaturatedF64Unsigned,
    V128Load {
        offset: u32,
        memory: u32,
    },
    V128Load8x8Signed {
        offset: u32,
        memory: u32,
    },
    V128Load8x8Unsigned {
        offset: u32,
        memory: u32,
    },
    V128Load16x4Signed {
        offset: u32,
        memory: u32,
    },
    V128Load16x4Unsigned {
        offset: u32,
        memory: u32,
    },
    V128Load32x2Signed {
        offset: u32,
        memory: u32,
    },
    V128Load32x2Unsigned {
        offset: u32,
        memory: u32,
    },
    V128Load8Splat {
        offset: u32,
        memory: u32,
    },
    V128Load16Splat {
        offset: u32,
        memory: u32,
    },
    V128Load32Splat {
        offset: u32,
        memory: u32,
    },
    V128Load64Splat {
        offset: u32,
        memory: u32,
    },
    V128Load32Zero {
        offset: u32,
        memory: u32,
    },
    V128Load64Zero {
        offset: u32,
        memory: u32,
    },
    V128Store {
        offset: u32,
        memory: u32,
    },
    V128Load8Lane {
        offset: u32,
        memory: u32,
        lane: u8,
    },
    V128Load16Lane {
        offset: u32,
        memory: u32,
        lane: u8,
    },
    V128Load32Lane {
        offset: u32,
        memory: u32,
        lane: u8,
    },
    V128Load64Lane {
        offset: u32,
        memory: u32,
        lane: u8,
    },
    V128Store8Lane {
        offset: u32,
        memory: u32,
        lane: u8,
    },
    V128Store16Lane {
        offset: u32,
        memory: u32,
        lane: u8,
    },
    V128Store32Lane {
        offset: u32,
        memory: u32,
        lane: u8,
    },
    V128Store64Lane {
        offset: u32,
        memory: u32,
        lane: u8,
    },
    I8x16Shuffle {
        table_idx: u32,
    },
    I8x16ExtractLaneSigned(u8),
    I8x16ExtractLaneUnsigned(u8),
    I8x16ReplaceLane(u8),
    I16x8ExtractLaneSigned(u8),
    I16x8ExtractLaneUnsigned(u8),
    I16x8ReplaceLane(u8),
    I32x4ExtractLane(u8),
    I32x4ReplaceLane(u8),
    I64x2ExtractLane(u8),
    I64x2ReplaceLane(u8),
    F32x4ExtractLane(u8),
    F32x4ReplaceLane(u8),
    F64x2ExtractLane(u8),
    F64x2ReplaceLane(u8),
    I8x16Swizzle,
    I8x16Splat,
    I16x8Splat,
    I32x4Splat,
    I64x2Splat,
    F32x4Splat,
    F64x2Splat,
    I8x16Eq,
    I8x16Ne,
    I8x16LtSigned,
    I8x16LtUnsigned,
    I8x16GtSigned,
    I8x16GtUnsigned,
    I8x16LeSigned,
    I8x16LeUnsigned,
    I8x16GeSigned,
    I8x16GeUnsigned,
    I16x8Eq,
    I16x8Ne,
    I16x8LtSigned,
    I16x8LtUnsigned,
    I16x8GtSigned,
    I16x8GtUnsigned,
    I16x8LeSigned,
    I16x8LeUnsigned,
    I16x8GeSigned,
    I16x8GeUnsigned,
    I32x4Eq,
    I32x4Ne,
    I32x4LtSigned,
    I32x4LtUnsigned,
    I32x4GtSigned,
    I32x4GtUnsigned,
    I32x4LeSigned,
    I32x4LeUnsigned,
    I32x4GeSigned,
    I32x4GeUnsigned,
    I64x2Eq,
    I64x2Ne,
    I64x2LtSigned,
    I64x2GtSigned,
    I64x2LeSigned,
    I64x2GeSigned,
    F32x4Eq,
    F32x4Ne,
    F32x4Lt,
    F32x4Gt,
    F32x4Le,
    F32x4Ge,
    F64x2Eq,
    F64x2Ne,
    F64x2Lt,
    F64x2Gt,
    F64x2Le,
    F64x2Ge,
    V128Not,
    V128And,
    V128AndNot,
    V128Or,
    V128Xor,
    V128BitSelect,
    V128AnyTrue,
    I8x16Abs,
    I8x16Neg,
    I8x16PopCount,
    I8x16AllTrue,
    I8x16BitMask,
    I8x16NarrowI16x8Signed,
    I8x16NarrowI16x8Unsigned,
    I8x16Shl,
    I8x16ShrSigned,
    I8x16ShrUnsigned,
    I8x16Add,
    I8x16AddSaturatedSigned,
    I8x16AddSaturatedUnsigned,
    I8x16Sub,
    I8x16SubSaturatedSigned,
    I8x16SubSaturatedUnsigned,
    I8x16MinSigned,
    I8x16MinUnsigned,
    I8x16MaxSigned,
    I8x16MaxUnsigned,
    I8x16AvgRangeUnsigned,
    I16x8ExtAddPairWiseI8x16Signed,
    I16x8ExtAddPairWiseI8x16Unsigned,
    I16x8Abs,
    I16x8Neg,
    I16xQ15MulRangeSaturatedSigned,
    I16x8AllTrue,
    I16x8BitMask,
    I16x8NarrowI32x4Signed,
    I16x8NarrowI32x4Unsigned,
    I16x8ExtendLowI8x16Unsigned,
    I16x8ExtendHighI8x16Unsigned,
    I16x8ExtendLowI8x16Signed,
    I16x8ExtendHighI8x16Signed,
    I16x8Shl,
    I16x8ShrSigned,
    I16x8ShrUnsigned,
    I16x8Add,
    I16x8AddSaturatedSigned,
    I16x8AddSaturatedUnsigned,
    I16x8Sub,
    I16x8SubSaturatedSigned,
    I16x8SubSaturatedUnsigned,
    I16x8Mul,
    I16x8MinSigned,
    I16x8MinUnsigned,
    I16x8MaxSigned,
    I16x8MaxUnsigned,
    I16x8AvgRangeUnsigned,
    I16x8ExtMulLowI8x16Signed,
    I16x8ExtMulHighI8x16Signed,
    I16x8ExtMulLowI8x16Unsigned,
    I16x8ExtMulHighI8x16Unsigned,
    I32x4ExtAddPairWiseI16x8Signed,
    I32x4ExtAddPairWiseI16x8Unsigned,
    I32x4Abs,
    I32x4Neg,
    I32x4AllTrue,
    I32x4BitMask,
    I32x4ExtendLowI16x8Signed,
    I32x4ExtendHighI16x8Signed,
    I32x4ExtendLowI16x8Unsigned,
    I32x4ExtendHighI16x8Unsigned,
    I32x4Shl,
    I32x4ShrSigned,
    I32x4ShrUnsigned,
    I32x4Add,
    I32x4Sub,
    I32x4Mul,
    I32x4MinSigned,
    I32x4MinUnsigned,
    I32x4MaxSigned,
    I32x4MaxUnsigned,
    I32x4DotI16x8Signed,
    I32x4ExtMulLowI16x8Signed,
    I32x4ExtMulHighI16x8Signed,
    I32x4ExtMulLowI16x8Unsigned,
    I32x4ExtMulHighI16x8Unsigned,
    I64x2Abs,
    I64x2Neg,
    I64x2AllTrue,
    I64x2BitMask,
    I64x2ExtendLowI32x4Signed,
    I64x2ExtendHighI32x4Signed,
    I64x2ExtendLowI32x4Unsigned,
    I64x2ExtendHighI32x4Unsigned,
    I64x2Shl,
    I64x2ShrSigned,
    I64x2ShrUnsigned,
    I64x2Add,
    I64x2Sub,
    I64x2Mul,
    I64x2ExtMulLowI32x4Signed,
    I64x2ExtMulHighI32x4Signed,
    I64x2ExtMulLowI32x4Unsigned,
    I64x2ExtMulHighI32x4Unsigned,
    F32x4Ceil,
    F32x4Floor,
    F32x4Trunc,
    F32x4Nearest,
    F32x4Abs,
    F32x4Neg,
    F32x4Sqrt,
    F32x4Add,
    F32x4Sub,
    F32x4Mul,
    F32x4Div,
    F32x4Min,
    F32x4Max,
    F32x4PMin,
    F32x4PMax,
    F64x2Ceil,
    F64x2Floor,
    F64x2Trunc,
    F64x2Nearest,
    F64x2Abs,
    F64x2Neg,
    F64x2Sqrt,
    F64x2Add,
    F64x2Sub,
    F64x2Mul,
    F64x2Div,
    F64x2Min,
    F64x2Max,
    F64x2PMin,
    F64x2PMax,
    I32x4TruncSaturatedF32x4Signed,
    I32x4TruncSaturatedF32x4Unsigned,
    F32x4ConvertI32x4Signed,
    F32x4ConvertI32x4Unsigned,
    I32x4TruncSaturatedF64x2SignedZero,
    I32x4TruncSaturatedF64x2UnsignedZero,
    F64x2ConvertLowI32x4Signed,
    F64x2ConvertLowI32x4Unsigned,
    F32x4DemoteF64x2Zero,
    F64x2PromoteLowF32x4,
    I8x16RelaxedSwizzle,
    I32x4RelaxedTruncF32x4Signed,
    I32x4RelaxedTruncF32x4Unsigned,
    I32x4RelaxedTruncF64x2SignedZero,
    I32x4RelaxedTruncF64x2UnsignedZero,
    F32x4RelaxedMadd,
    F32x4RelaxedNmadd,
    F64x2RelaxedMadd,
    F64x2RelaxedNmadd,
    I8x16RelaxedLaneselect,
    I16x8RelaxedLaneselect,
    I32x4RelaxedLaneselect,
    I64x2RelaxedLaneselect,
    F32x4RelaxedMin,
    F32x4RelaxedMax,
    F64x2RelaxedMin,
    F64x2RelaxedMax,
    I16x8RelaxedQ15mulrSigned,
    I16x8RelaxedDotI8x16I7x16Signed,
    I32x4RelaxedDotI8x16I7x16AddSigned,
    I32EqZeroJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I32EqZeroJumpIfNot {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I32EqJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I32NeJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I32LtSignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I32LtUnsignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I32GtSignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I32GtUnsignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I32LeSignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I32LeUnsignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I32GeSignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I32GeUnsignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I64EqZeroJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I64EqJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I64NeJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I64LtSignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I64LtUnsignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I64GtSignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I64GtUnsignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I64LeSignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I64LeUnsignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I64GeSignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    I64GeUnsignedJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    F32EqJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    F32NeJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    F32LtJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    F32GtJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    F32LeJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    F32GeJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    F64EqJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    F64NeJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    F64LtJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    F64GtJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    F64LeJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    F64GeJumpIf {
        target: u32,
        keep: u16,
        drop: u16,
    },
    LocalGet2 {
        local_idx_a: u32,
        local_idx_b: u32,
    },
    LocalGetReturn {
        local_idx: u32,
    },
    LocalGetI32Load {
        local_idx: u32,
        offset: u32,
        memory: u32,
    },
    LocalGetI64Load {
        local_idx: u32,
        offset: u32,
        memory: u32,
    },
    LocalGetF32Load {
        local_idx: u32,
        offset: u32,
        memory: u32,
    },
    LocalGetF64Load {
        local_idx: u32,
        offset: u32,
        memory: u32,
    },
    LocalGetI32Store {
        local_idx: u32,
        offset: u32,
        memory: u32,
    },
    LocalGetI64Store {
        local_idx: u32,
        offset: u32,
        memory: u32,
    },
    LocalGetF32Store {
        local_idx: u32,
        offset: u32,
        memory: u32,
    },
    LocalGetF64Store {
        local_idx: u32,
        offset: u32,
        memory: u32,
    },
    LocalGetLocalSet {
        local_get_idx: u32,
        local_set_idx: u32,
    },
}

impl Op {
    pub const fn jump_target(&self) -> Option<u32> {
        match self {
            Self::Jump { target, .. }
            | Self::JumpIf { target, .. }
            | Self::JumpIfNot { target, .. }
            | Self::BrOnNull { target, .. }
            | Self::BrOnNonNull { target, .. }
            | Self::I32EqZeroJumpIf { target, .. }
            | Self::I32EqZeroJumpIfNot { target, .. }
            | Self::I32EqJumpIf { target, .. }
            | Self::I32NeJumpIf { target, .. }
            | Self::I32LtSignedJumpIf { target, .. }
            | Self::I32LtUnsignedJumpIf { target, .. }
            | Self::I32GtSignedJumpIf { target, .. }
            | Self::I32GtUnsignedJumpIf { target, .. }
            | Self::I32LeSignedJumpIf { target, .. }
            | Self::I32LeUnsignedJumpIf { target, .. }
            | Self::I32GeSignedJumpIf { target, .. }
            | Self::I32GeUnsignedJumpIf { target, .. }
            | Self::I64EqZeroJumpIf { target, .. }
            | Self::I64EqJumpIf { target, .. }
            | Self::I64NeJumpIf { target, .. }
            | Self::I64LtSignedJumpIf { target, .. }
            | Self::I64LtUnsignedJumpIf { target, .. }
            | Self::I64GtSignedJumpIf { target, .. }
            | Self::I64GtUnsignedJumpIf { target, .. }
            | Self::I64LeSignedJumpIf { target, .. }
            | Self::I64LeUnsignedJumpIf { target, .. }
            | Self::I64GeSignedJumpIf { target, .. }
            | Self::I64GeUnsignedJumpIf { target, .. }
            | Self::F32EqJumpIf { target, .. }
            | Self::F32NeJumpIf { target, .. }
            | Self::F32LtJumpIf { target, .. }
            | Self::F32GtJumpIf { target, .. }
            | Self::F32LeJumpIf { target, .. }
            | Self::F32GeJumpIf { target, .. }
            | Self::F64EqJumpIf { target, .. }
            | Self::F64NeJumpIf { target, .. }
            | Self::F64LtJumpIf { target, .. }
            | Self::F64GtJumpIf { target, .. }
            | Self::F64LeJumpIf { target, .. }
            | Self::F64GeJumpIf { target, .. } => Some(*target),
            _ => None,
        }
    }
}
