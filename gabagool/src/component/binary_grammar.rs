use crate::binary_grammar::{ImportDeclaration, ImportDescription, ParsedModule, SubType};
use crate::parser::Parser;
use crate::{flatten, parse_err, Error, Result};

#[derive(Debug)]
pub struct Component {
    pub(crate) flattened: flatten::FlattenedComponent,
}

impl Component {
    pub fn new(bytes: &[u8]) -> Result<Self> {
        let parsed = Parser::new(bytes).parse()?.try_as_component()?;
        let flattened = flatten::flatten(&parsed)?;

        Ok(Self { flattened })
    }
}

#[derive(Debug, Clone)]
pub struct ParsedComponent {
    pub sections: Vec<ComponentSection>,
}

#[derive(Debug, Clone)]
pub enum ComponentSection {
    CoreModule(Box<ParsedModule>),
    CoreInstance(Vec<CoreInstance>),
    CoreType(Vec<CoreType>),
    Component(ParsedComponent),
    Instance(Vec<ParsedComponentInstance>),
    Alias(Vec<Alias>),
    ComponentType(Vec<ComponentTypeDef>),
    Canonical(Vec<CanonicalDef>),
    Start(ComponentStart),
    Import(Vec<ComponentImport>),
    Export(Vec<ComponentExport>),
}

#[derive(Debug, Clone)]
pub struct ComponentStart {
    pub func_i: u32,
    pub args: Vec<u32>,
    pub results: u32,
}

#[derive(Debug, Clone)]
pub enum CoreType {
    SubType(SubType),
    Module(CoreModuleType),
}

#[derive(Debug, Clone)]
pub struct CoreModuleType {
    pub declarations: Vec<CoreModuleDecl>,
}

#[derive(Debug, Clone)]
pub enum CoreModuleDecl {
    Import(ImportDeclaration),
    Type(SubType),
    Alias(Alias),
    Export(CoreExportDecl),
}

#[derive(Debug, Clone)]
pub struct CoreExportDecl {
    pub name: String,
    pub description: ImportDescription,
}

#[derive(Debug, Clone)]
pub enum CanonicalDef {
    Lift {
        core_func_i: u32,
        opts: ParsedCanonOpts,
        type_i: u32,
    },
    Lower {
        func_i: u32,
        opts: ParsedCanonOpts,
    },
    ResourceNew(u32),
    ResourceDrop(u32),
    ResourceRep(u32),
    TaskCancel,
    SubtaskCancel {
        async_: bool,
    },
    BackpressureSet,
    TaskReturn {
        result_type: Option<ComponentValueKind>,
        opts: ParsedCanonOpts,
    },
    ContextGet {
        slot: u32,
    },
    ContextSet {
        slot: u32,
    },
    SubtaskDrop,
    BackpressureInc,
    BackpressureDec,
    StreamNew(u32),
    StreamRead {
        type_i: u32,
        opts: ParsedCanonOpts,
    },
    StreamWrite {
        type_i: u32,
        opts: ParsedCanonOpts,
    },
    StreamCancelRead {
        type_i: u32,
        async_: bool,
    },
    StreamCancelWrite {
        type_i: u32,
        async_: bool,
    },
    StreamDropReadable(u32),
    StreamDropWritable(u32),
    FutureNew(u32),
    FutureRead {
        type_i: u32,
        opts: ParsedCanonOpts,
    },
    FutureWrite {
        type_i: u32,
        opts: ParsedCanonOpts,
    },
    FutureCancelRead {
        type_i: u32,
        async_: bool,
    },
    FutureCancelWrite {
        type_i: u32,
        async_: bool,
    },
    FutureDropReadable(u32),
    FutureDropWritable(u32),
    ErrorContextNew(ParsedCanonOpts),
    ErrorContextDebugMessage(ParsedCanonOpts),
    ErrorContextDrop,
    WaitableSetNew,
    WaitableSetWait {
        cancel: bool,
        memory: u32,
    },
    WaitableSetPoll {
        cancel: bool,
        memory: u32,
    },
    WaitableSetDrop,
    WaitableJoin,
    ThreadYield {
        cancel: bool,
    },
    ThreadIndex,
    ThreadNewIndirect {
        type_i: u32,
        table: u32,
    },
    ThreadSuspendToSuspended {
        cancel: bool,
    },
    ThreadSuspend {
        cancel: bool,
    },
    ThreadUnsuspend,
    ThreadYieldToSuspended {
        cancel: bool,
    },
    ThreadSuspendTo {
        cancel: bool,
    },
    ThreadSpawnRef {
        shared: bool,
        type_i: u32,
    },
    ThreadSpawnIndirect {
        shared: bool,
        type_i: u32,
        table: u32,
    },
    ThreadAvailableParallelism {
        shared: bool,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ParsedCanonOpts {
    pub string_encoding: StringEncoding,
    pub memory: Option<u32>,
    pub realloc: Option<u32>,
    pub post_return: Option<u32>,
    pub async_: bool,
    pub callback: Option<u32>,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, Default)]
pub enum StringEncoding {
    #[default]
    Utf8,
    Utf16,
    Latin1Utf16,
}

#[derive(Debug, Clone)]
pub enum CoreInstance {
    Instantiate {
        module_i: u32,
        args: Vec<CoreInstantiateArg>,
    },
    FromExports(Vec<CoreInlineExport>),
}

#[derive(Debug, Clone)]
pub struct CoreInstantiateArg {
    pub name: String,
    pub instance_i: u32,
}

#[derive(Debug, Clone)]
pub struct CoreInlineExport {
    pub name: String,
    pub sort: CoreSort,
    pub i: u32,
}

#[derive(Debug, Clone, Copy)]
pub enum CoreSort {
    Func,
    Table,
    Memory,
    Global,
    Tag,
    Type,
    Module,
    Instance,
}

impl TryFrom<u8> for CoreSort {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        let out = match value {
            0x00 => Self::Func,
            0x01 => Self::Table,
            0x02 => Self::Memory,
            0x03 => Self::Global,
            0x04 => Self::Tag,
            0x10 => Self::Type,
            0x11 => Self::Module,
            0x12 => Self::Instance,
            b => parse_err!("unknown core sort: {b:#x}"),
        };

        Ok(out)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ComponentSort {
    Core(CoreSort),
    Func,
    Value,
    Type,
    Component,
    Instance,
}

#[derive(Debug, Clone)]
pub enum Alias {
    CoreExport {
        sort: ComponentSort,
        instance_i: u32,
        name: String,
    },
    Export {
        sort: ComponentSort,
        instance_i: u32,
        name: String,
    },
    Outer {
        sort: ComponentSort,
        count: u32,
        i: u32,
    },
}

#[derive(Debug, Clone)]
pub enum ParsedComponentInstance {
    Instantiate {
        component_i: u32,
        args: Vec<ComponentInstantiateArg>,
    },
    FromExports(Vec<ComponentInlineExport>),
}

#[derive(Debug, Clone)]
pub struct ComponentInstantiateArg {
    pub name: String,
    pub sort: ComponentSort,
    pub i: u32,
}

#[derive(Debug, Clone)]
pub struct ComponentInlineExport {
    pub name: String,
    pub sort: ComponentSort,
    pub i: u32,
}

#[derive(Debug, Clone)]
pub struct ComponentImport {
    pub name: String,
    pub desc: ExternDesc,
}

#[derive(Debug, Clone)]
pub enum ExternDesc {
    CoreModule(u32),
    Func(u32),
    Type(TypeBound),
    Component(u32),
    Instance(u32),
}

#[derive(Debug, Clone)]
pub struct ComponentExport {
    pub name: String,
    pub sort: ComponentSort,
    pub i: u32,
    pub desc: Option<ExternDesc>,
}

#[derive(Debug, Clone)]
pub enum TypeBound {
    Eq(u32),
    SubResource,
}

#[derive(Debug, Clone)]
pub enum ComponentTypeDef {
    Defined(ComponentDefinedKind),
    Func(ComponentFuncKind),
    Component(Vec<ComponentTypeDecl>),
    Instance(Vec<InstanceTypeDecl>),
    Resource { dtor: Option<u32> },
}

#[derive(Debug, Clone)]
pub struct ComponentFuncKind {
    pub params: Vec<(String, ComponentValueKind)>,
    pub results: ComponentFuncResult,
}

#[derive(Debug, Clone)]
pub enum ComponentFuncResult {
    Unnamed(ComponentValueKind),
    Named(Vec<(String, ComponentValueKind)>),
}

#[derive(Debug, Clone)]
pub enum ComponentValueKind {
    Type(u32),
    Primitive(PrimitiveValueKind),
}

impl ComponentValueKind {
    pub fn flat_count(&self) -> usize {
        match self {
            Self::Primitive(PrimitiveValueKind::String) => 2,
            Self::Primitive(_) => 1,
            Self::Type(_) => todo!(),
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum PrimitiveValueKind {
    Bool,
    S8,
    U8,
    S16,
    U16,
    S32,
    U32,
    S64,
    U64,
    F32,
    F64,
    Char,
    String,
}

impl PrimitiveValueKind {
    pub fn from_byte(b: u8) -> Result<Self> {
        let out = match b {
            0x7f => Self::Bool,
            0x7e => Self::S8,
            0x7d => Self::U8,
            0x7c => Self::S16,
            0x7b => Self::U16,
            0x7a => Self::S32,
            0x79 => Self::U32,
            0x78 => Self::S64,
            0x77 => Self::U64,
            0x76 => Self::F32,
            0x75 => Self::F64,
            0x74 => Self::Char,
            0x73 => Self::String,
            _ => parse_err!("unknown primitive val type: {b:#x}"),
        };
        Ok(out)
    }
}

#[derive(Debug, Clone)]
pub enum ComponentDefinedKind {
    Primitive(PrimitiveValueKind),
    Record(Vec<(String, ComponentValueKind)>),
    Variant(Vec<VariantCase>),
    List(ComponentValueKind),
    Tuple(Vec<ComponentValueKind>),
    Flags(Vec<String>),
    Enum(Vec<String>),
    Option(ComponentValueKind),
    Result {
        ok: Option<ComponentValueKind>,
        err: Option<ComponentValueKind>,
    },
    Own(u32),
    Borrow(u32),
}

#[derive(Debug, Clone)]
pub struct VariantCase {
    pub name: String,
    pub ty: Option<ComponentValueKind>,
    pub refines: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum InstanceTypeDecl {
    CoreType(CoreType),
    Type(ComponentTypeDef),
    Alias(Alias),
    Export(ComponentExportDecl),
}

#[derive(Debug, Clone)]
pub enum ComponentTypeDecl {
    Instance(InstanceTypeDecl),
    Import(ComponentImport),
}

#[derive(Debug, Clone)]
pub struct ComponentExportDecl {
    pub name: String,
    pub desc: ExternDesc,
}
