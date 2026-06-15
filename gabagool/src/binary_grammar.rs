use crate::component::ParsedComponent;
use crate::{
    compiler::{self, ModuleCode},
    ir::CompilerMode,
    parse_err,
    parser::Parser,
    Result,
};
use std::sync::Arc;

/// A parsed and compiled WASM module ready to be instantiated
#[derive(Debug)]
pub struct Module {
    pub(crate) code: Arc<ModuleCode>,

    pub(crate) functions: Vec<Function>,
    pub(crate) tables: Vec<TableDef>,
    pub(crate) mems: Vec<MemoryType>,
    pub(crate) element_segments: Vec<ElementSegment>,
    pub(crate) globals: Vec<Global>,
    pub(crate) data_segments: Vec<DataSegment>,
    pub(crate) start: Option<u32>,
    pub(crate) import_declarations: Vec<ImportDeclaration>,
    pub(crate) exports: Vec<Export>,
    pub(crate) tags: Vec<Tag>,

    pub(crate) compile_mode: CompilerMode,
}

impl Module {
    pub fn new(bytes: &[u8]) -> Result<Self> {
        Self::new_with_mode(bytes, CompilerMode::Optimize)
    }

    pub fn new_with_mode(bytes: &[u8], compile_mode: CompilerMode) -> Result<Self> {
        let module = Parser::new(bytes).parse()?.try_as_module()?;
        Self::from_parsed(module, compile_mode)
    }

    pub fn from_parsed(parsed: ParsedModule, compile_mode: CompilerMode) -> Result<Self> {
        let code = compiler::compile(&parsed, compile_mode);

        Ok(Self {
            code: Arc::new(code),
            functions: parsed.functions,
            tables: parsed.tables,
            mems: parsed.mems,
            element_segments: parsed.element_segments,
            globals: parsed.globals,
            data_segments: parsed.data_segments,
            start: parsed.start,
            import_declarations: parsed.import_declarations,
            exports: parsed.exports,
            tags: parsed.tags,
            compile_mode,
        })
    }

    pub fn import_declarations(&self) -> &[ImportDeclaration] {
        &self.import_declarations
    }

    pub fn types(&self) -> &[SubType] {
        &self.code.types
    }
}

// todo: should we box ParsedModule?
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Parsed {
    Module(ParsedModule),
    Component(ParsedComponent),
}

impl Parsed {
    pub fn try_as_module(self) -> Result<ParsedModule> {
        let out = match self {
            Self::Module(m) => m,
            Self::Component(_) => parse_err!("expected module"),
        };

        Ok(out)
    }

    pub fn try_as_component(self) -> Result<ParsedComponent> {
        let out = match self {
            Self::Module(_) => parse_err!("expected component"),
            Self::Component(c) => c,
        };

        Ok(out)
    }
}

#[derive(Debug, Default, Clone)]
pub struct ParsedModule {
    pub types: Vec<SubType>,
    pub functions: Vec<Function>,
    pub tables: Vec<TableDef>,
    pub mems: Vec<MemoryType>,
    pub element_segments: Vec<ElementSegment>,
    pub globals: Vec<Global>,
    pub data_segments: Vec<DataSegment>,
    pub start: Option<u32>,
    pub import_declarations: Vec<ImportDeclaration>,
    pub exports: Vec<Export>,
    pub tags: Vec<Tag>,
    pub customs: Vec<CustomSection>,
}

#[derive(Debug, Clone)]
pub enum ModuleSection {
    Custom(CustomSection),
    Type(TypeSection),
    Import(ImportSection),
    Function(FunctionSection),
    Table(TableSection),
    Memory(MemorySection),
    Global(GlobalSection),
    Export(ExportSection),
    Start(u32),
    Element(ElementSection),
    Code(CodeSection),
    Data(DataSection),
    DataCount(u32),
    Tag(TagSection),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum HeapType {
    Func,     // 0x70
    Extern,   // 0x6F
    Any,      // 0x6E
    Eq,       // 0x6D
    I31,      // 0x6C
    Struct,   // 0x6B
    Array,    // 0x6A
    Exn,      // 0x69
    None,     // 0x71
    NoExtern, // 0x72
    NoFunc,   // 0x73
    NoExn,    // 0x74
    TypeIndex(u32),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AddrType {
    I32,
    I64,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RefType {
    FuncRef,
    ExternRef,
    Ref { nullable: bool, heap_type: HeapType },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueType {
    I32,
    I64,
    F32,
    F64,
    V128,
    Ref(RefType),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultType(pub Vec<ValueType>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionType(pub ResultType, pub ResultType);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    pub min: u64,
    pub max: u64,
}

#[derive(Debug, Clone)]
pub struct MemoryType {
    pub addr_type: AddrType,
    pub limit: Limit,
}

#[derive(Debug, Copy, Clone)]
pub struct TableType {
    pub element_reference_type: RefType,
    pub addr_type: AddrType,
    pub limit: Limit,
}

#[derive(Debug, Clone)]
pub enum StorageType {
    Val(ValueType),
    I8,
    I16,
}

#[derive(Debug, Clone)]
pub struct FieldType {
    pub storage_type: StorageType,
    pub mutability: Mutability,
}

#[derive(Debug, Clone)]
pub struct StructType {
    pub fields: Vec<FieldType>,
}

#[derive(Debug, Clone)]
pub struct ArrayType {
    pub field_type: FieldType,
}

#[derive(Debug, Clone)]
pub enum CompositeType {
    Func(FunctionType),
    Struct(StructType),
    Array(ArrayType),
}

#[derive(Debug, Clone)]
pub struct SubType {
    pub is_final: bool,
    pub supertypes: Vec<u32>,
    pub composite_type: CompositeType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mutability {
    Const,
    Var,
}

#[derive(Debug, Clone)]
pub struct GlobalType {
    pub value_type: ValueType,
    pub mutability: Mutability,
}

#[derive(Debug, Clone)]
pub struct CustomSection {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TypeSection {
    pub types: Vec<SubType>,
}

#[derive(Debug, Clone)]
pub enum ImportDescription {
    Func(u32),
    Table(TableType),
    Mem(MemoryType),
    Global(GlobalType),
    Tag(u32),
}

#[derive(Debug, Clone)]
pub struct ImportDeclaration {
    pub module: String,
    pub name: String,
    pub description: ImportDescription,
}

#[derive(Debug, Clone)]
pub struct ImportSection {
    pub import_declarations: Vec<ImportDeclaration>,
}

#[derive(Debug, Clone)]
pub struct FunctionSection {
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct TableDef {
    pub table_type: TableType,
    pub init: Vec<Instruction>,
}

#[derive(Debug, Clone)]
pub struct TableSection {
    pub tables: Vec<TableDef>,
}

#[derive(Debug, Clone)]
pub struct MemorySection {
    pub memories: Vec<MemoryType>,
}

#[derive(Debug, Clone)]
pub struct Global {
    pub global_type: GlobalType,
    pub initial_expression: Vec<Instruction>,
}

#[derive(Debug, Clone)]
pub struct GlobalSection {
    pub globals: Vec<Global>,
}

#[derive(Debug, Clone, Copy)]
pub struct Tag {
    pub type_index: u32,
}

#[derive(Debug, Clone)]
pub struct TagSection {
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone)]
pub enum CatchClause {
    Catch { tag: u32, label: u32 },
    CatchRef { tag: u32, label: u32 },
    CatchAll { label: u32 },
    CatchAllRef { label: u32 },
}

#[derive(Debug, Clone)]
pub enum BlockType {
    Empty,
    SingleValue(ValueType),
    TypeIndex(i32),
}

#[derive(Debug, Clone)]
pub struct MemArg {
    pub align: u32,
    pub offset: u64,
    pub memory: u32,
}

pub const TERM_END_BYTE: u8 = 0x0B;
pub const TERM_ELSE_BYTE: u8 = 0x05;

#[derive(Debug, Clone)]
pub enum Instruction {
    Unreachable,
    Nop,
    Block(BlockType, Vec<Self>),
    Loop(BlockType, Vec<Self>),
    IfElse(BlockType, Vec<Self>, Vec<Self>),
    Br(u32),
    BrIf(u32),
    BrTable(Vec<u32>, u32),
    Return,
    Throw(u32),
    ThrowRef,
    Call(u32),
    CallIndirect(u32, u32),
    ReturnCall(u32),
    ReturnCallIndirect(u32, u32),
    CallRef(u32),
    ReturnCallRef(u32),
    TryTable(BlockType, Vec<CatchClause>, Vec<Self>),
    BrOnNull(u32),
    BrOnNonNull(u32),
    RefNull(HeapType),
    RefIsNull,
    RefEq,
    RefAsNonNull,
    RefFunc(u32),
    Drop,
    Select(Vec<ValueType>),
    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),
    GlobalGet(u32),
    GlobalSet(u32),
    TableGet(u32),
    TableSet(u32),
    TableInit(u32, u32),
    ElemDrop(u32),
    TableCopy(u32, u32),
    TableGrow(u32),
    TableSize(u32),
    TableFill(u32),
    I32Load(MemArg),
    I64Load(MemArg),
    F32Load(MemArg),
    F64Load(MemArg),
    I32Load8Signed(MemArg),
    I32Load8Unsigned(MemArg),
    I32Load16Signed(MemArg),
    I32Load16Unsigned(MemArg),
    I64Load8Signed(MemArg),
    I64Load8Unsigned(MemArg),
    I64Load16Signed(MemArg),
    I64Load16Unsigned(MemArg),
    I64Load32Signed(MemArg),
    I64Load32Unsigned(MemArg),
    I32Store(MemArg),
    I64Store(MemArg),
    F32Store(MemArg),
    F64Store(MemArg),
    I32Store8(MemArg),
    I32Store16(MemArg),
    I64Store8(MemArg),
    I64Store16(MemArg),
    I64Store32(MemArg),
    MemorySize(u32),
    MemoryGrow(u32),
    MemoryInit(u32, u32),
    DataDrop(u32),
    MemoryCopy(u32, u32),
    MemoryFill(u32),
    I32Const(i32),
    I64Const(i64),
    F32Const(f32),
    F64Const(f64),
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
    V128Load(MemArg),
    V128Load8x8Signed(MemArg),
    V128Load8x8Unsigned(MemArg),
    V128Load16x4Unsigned(MemArg),
    V128Load16x4Signed(MemArg),
    V128Load32x2Signed(MemArg),
    V128Load32x2Unsigned(MemArg),
    V128Load8Splat(MemArg),
    V128Load16Splat(MemArg),
    V128Load32Splat(MemArg),
    V128Load64Splat(MemArg),
    V128Load32Zero(MemArg),
    V128Load64Zero(MemArg),
    V128Store(MemArg),
    V128Load8Lane(MemArg, u8),
    V128Load16Lane(MemArg, u8),
    V128Load32Lane(MemArg, u8),
    V128Load64Lane(MemArg, u8),
    V128Store8Lane(MemArg, u8),
    V128Store16Lane(MemArg, u8),
    V128Store32Lane(MemArg, u8),
    V128Store64Lane(MemArg, u8),
    V128Const(i128),
    I8x16Shuffle([u8; 16]),
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
    F32X4Eq,
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
    F64xPromoteLowF32x4,
    StructNew(u32),
    StructNewDefault(u32),
    StructGet(u32, u32),
    StructGetSigned(u32, u32),
    StructGetUnsigned(u32, u32),
    StructSet(u32, u32),
    ArrayNew(u32),
    ArrayNewDefault(u32),
    ArrayNewFixed(u32, u32),
    ArrayNewData(u32, u32),
    ArrayNewElem(u32, u32),
    ArrayGet(u32),
    ArrayGetSigned(u32),
    ArrayGetUnsigned(u32),
    ArraySet(u32),
    ArrayLen,
    ArrayFill(u32),
    ArrayCopy(u32, u32),
    ArrayInitData(u32, u32),
    ArrayInitElem(u32, u32),
    RefTest(HeapType),
    RefTestNull(HeapType),
    RefCast(HeapType),
    RefCastNull(HeapType),
    BrOnCast(u8, u32, HeapType, HeapType),
    BrOnCastFail(u8, u32, HeapType, HeapType),
    AnyConvertExtern,
    ExternConvertAny,
    RefI31,
    I31GetSigned,
    I31GetUnsigned,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstructionLocation {
    pub code_offset: u64,
}

#[derive(Debug, Clone)]
pub enum ExportDescription {
    Func(u32),
    Table(u32),
    Mem(u32),
    Global(u32),
    Tag(u32),
}

#[derive(Debug, Clone)]
pub struct Export {
    pub name: String,
    pub description: ExportDescription,
}

#[derive(Debug, Clone)]
pub struct ExportSection {
    pub exports: Vec<Export>,
}

#[derive(Debug, Clone)]
pub enum ElementMode {
    Passive,
    Active {
        table_index: u32,
        offset: Vec<Instruction>,
    },
    Declarative,
}

#[derive(Debug, Clone)]
pub struct ElementSegment {
    pub ref_type: RefType,
    pub expression: Vec<Vec<Instruction>>,
    pub mode: ElementMode,
}

#[derive(Debug, Clone)]
pub struct ElementSection {
    pub elements: Vec<ElementSegment>,
}

#[derive(Debug, Clone)]
pub struct Local {
    pub count: u32,
    pub value_type: ValueType,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub type_index: u32,
    pub locals: Vec<Local>,
    pub body: Vec<Instruction>,
    pub instruction_locations: Vec<InstructionLocation>,
}

#[derive(Debug, Clone)]
pub struct CodeSection {
    pub codes: Vec<Function>,
}

#[derive(Debug, Clone)]
pub enum DataMode {
    Active {
        memory: u32,
        offset: Vec<Instruction>,
    },
    Passive,
}

#[derive(Debug, Clone)]
pub struct DataSegment {
    pub bytes: Vec<u8>,
    pub mode: DataMode,
}

#[derive(Debug, Clone)]
pub struct DataSection {
    pub data_segments: Vec<DataSegment>,
}
