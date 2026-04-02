use std::cmp::min;
use std::collections::VecDeque;

use crate::error::{Error, Result};
use crate::{
    ensure, parse_err, Alias, CanonOpts, CanonicalDef, ComponentDefinedType, ComponentExport,
    ComponentExportDecl, ComponentFuncResult, ComponentFuncType, ComponentImport,
    ComponentInlineExport, ComponentInstantiateArg, ComponentSection, ComponentSort,
    ComponentStart, ComponentTypeDecl, ComponentTypeDef, ComponentValType, CoreExportDecl,
    CoreInlineExport, CoreInstance, CoreInstantiateArg, CoreModuleDecl, CoreModuleType, CoreType,
    ExternDesc, InstanceTypeDecl, Parsed, ParsedComponent, ParsedComponentInstance,
    PrimitiveValType, StringEncoding, TypeBound, VariantCase,
};

use crate::binary_grammar::{
    AddrType, ArrayType, BlockType, CatchClause, CodeSection, CompositeType, CustomSection,
    DataMode, DataSection, DataSegment, ElementMode, ElementSection, ElementSegment, Export,
    ExportDescription, ExportSection, FieldType, Function, FunctionSection, FunctionType, Global,
    GlobalSection, GlobalType, HeapType, ImportDeclaration, ImportDescription, ImportSection,
    Instruction, Limit, Local, MemArg, MemorySection, MemoryType, ModuleSection, Mutability,
    ParsedModule, RefType, ResultType, StorageType, StructType, SubType, TableDef, TableSection,
    TableType, Tag, TagSection, TypeSection, ValueType, TERM_ELSE_BYTE, TERM_END_BYTE,
};
use crate::leb128::{self, MAX_LEB128_LEN_32, MAX_LEB128_LEN_64};

#[derive(Debug)]
pub struct Parser<'a> {
    cursor: usize,
    buffer: &'a [u8],
    function_types: VecDeque<u32>,
    uses_data_count_instructions: bool,
}

impl<'a> Parser<'a> {
    pub const fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer,
            cursor: 0,
            function_types: VecDeque::new(),
            uses_data_count_instructions: false,
        }
    }

    pub fn parse(&mut self) -> Result<Parsed> {
        let (version, layer) = self.parse_preamble()?;

        let out = match (version, layer) {
            (0x01, 0x00) => Parsed::Module(self.parse_module()?),
            (0x0d, 0x01) => Parsed::Component(self.parse_component()?),
            _ => parse_err!("unrecognized preamble. version {version} layer {layer}"),
        };

        Ok(out)
    }

    fn parse_component(&mut self) -> Result<ParsedComponent> {
        let mut sections = Vec::new();

        while self.cursor < self.buffer.len() {
            let id = self.read_u8()?;
            let body_len = self.read_u32()?;
            let expected_section_end = self.cursor + body_len as usize;

            match id {
                0 => {
                    self.cursor = expected_section_end;
                }
                1 => {
                    let full_buffer = self.buffer;
                    self.buffer = &full_buffer[..expected_section_end];
                    let Parsed::Module(module) = self.parse()? else {
                        parse_err!("expected core module in section 1");
                    };
                    sections.push(ComponentSection::CoreModule(Box::new(module)));
                    self.buffer = full_buffer;
                }
                2 => {
                    sections.push(ComponentSection::CoreInstance(
                        self.parse_vec(Self::parse_core_instance)?,
                    ));
                }
                3 => {
                    sections.push(ComponentSection::CoreType(
                        self.parse_vec(Self::parse_core_type)?,
                    ));
                }
                4 => {
                    let full_buffer = self.buffer;
                    self.buffer = &full_buffer[..expected_section_end];
                    let Parsed::Component(component) = self.parse()? else {
                        parse_err!("expected component in section 4");
                    };
                    sections.push(ComponentSection::Component(component));
                    self.buffer = full_buffer;
                }
                5 => {
                    sections.push(ComponentSection::Instance(
                        self.parse_vec(Self::parse_component_instance)?,
                    ));
                }
                6 => {
                    sections.push(ComponentSection::Alias(self.parse_vec(Self::parse_alias)?));
                }
                7 => match self.parse_vec(Self::parse_component_type_def) {
                    Ok(defs) => sections.push(ComponentSection::ComponentType(defs)),
                    Err(_) => self.cursor = expected_section_end,
                },
                8 => match self.parse_vec(Self::parse_canonical_def) {
                    Ok(defs) => sections.push(ComponentSection::Canonical(defs)),
                    Err(_) => self.cursor = expected_section_end,
                },
                9 => {
                    sections.push(ComponentSection::Start(ComponentStart {
                        func_idx: self.read_u32()?,
                        args: self.parse_vec(Self::read_u32)?,
                        results: self.read_u32()?,
                    }));
                }
                10 => {
                    sections.push(ComponentSection::Import(
                        self.parse_vec(Self::parse_component_import)?,
                    ));
                }
                11 => {
                    sections.push(ComponentSection::Export(
                        self.parse_vec(Self::parse_component_export)?,
                    ));
                }
                12 => {
                    self.cursor = expected_section_end;
                }
                foreign_section_id => parse_err!("unrecognized section id {foreign_section_id}"),
            };

            ensure!(
                self.cursor == expected_section_end,
                Error::Parse(format!(
                    "component section {id} size mismatch: expected to end at {expected_section_end}, got {}",
                    self.cursor
                ))
            );
        }

        Ok(ParsedComponent { sections })
    }

    fn parse_core_instance(&mut self) -> Result<CoreInstance> {
        let out = match self.read_u8()? {
            0 => CoreInstance::Instantiate {
                module_idx: self.read_u32()?,
                args: self.parse_vec(|p| {
                    let name = p.parse_name()?;
                    let sort = p.read_u8()?;
                    ensure!(
                        sort == 0x12,
                        Error::Parse(format!(
                            "expected instance sort (0x12) in core instantiate arg, got {sort:#x}"
                        ))
                    );
                    Ok(CoreInstantiateArg {
                        name,
                        instance_idx: p.read_u32()?,
                    })
                })?,
            },
            1 => CoreInstance::FromExports(self.parse_vec(|p| {
                Ok(CoreInlineExport {
                    name: p.parse_name()?,
                    sort: p.read_u8()?.try_into()?,
                    idx: p.read_u32()?,
                })
            })?),
            b => parse_err!("unknown core instance type: {b:#x}"),
        };

        Ok(out)
    }

    fn parse_component_instance(&mut self) -> Result<ParsedComponentInstance> {
        let out = match self.read_u8()? {
            0 => ParsedComponentInstance::Instantiate {
                component_idx: self.read_u32()?,
                args: self.parse_vec(|p| {
                    Ok(ComponentInstantiateArg {
                        name: p.parse_name()?,
                        sort: p.parse_component_sort()?,
                        idx: p.read_u32()?,
                    })
                })?,
            },
            1 => ParsedComponentInstance::FromExports(self.parse_vec(|p| {
                Ok(ComponentInlineExport {
                    name: p.parse_component_name()?,
                    sort: p.parse_component_sort()?,
                    idx: p.read_u32()?,
                })
            })?),
            b => parse_err!("unknown component instance type: {b:#x}"),
        };

        Ok(out)
    }

    fn parse_component_sort(&mut self) -> Result<ComponentSort> {
        let out = match self.read_u8()? {
            0x00 => ComponentSort::Core(self.read_u8()?.try_into()?),
            0x01 => ComponentSort::Func,
            0x02 => ComponentSort::Value,
            0x03 => ComponentSort::Type,
            0x04 => ComponentSort::Component,
            0x05 => ComponentSort::Instance,
            b => parse_err!("unknown component sort: {b:#x}"),
        };

        Ok(out)
    }

    fn parse_alias(&mut self) -> Result<Alias> {
        let sort = self.parse_component_sort()?;
        let out = match self.read_u8()? {
            0 => Alias::Export {
                sort,
                instance_idx: self.read_u32()?,
                name: self.parse_name()?,
            },
            1 => Alias::CoreExport {
                sort,
                instance_idx: self.read_u32()?,
                name: self.parse_name()?,
            },
            2 => Alias::Outer {
                sort,
                count: self.read_u32()?,
                idx: self.read_u32()?,
            },
            b => parse_err!("unknown alias target: {b:#x}"),
        };

        Ok(out)
    }

    fn parse_component_import(&mut self) -> Result<ComponentImport> {
        Ok(ComponentImport {
            name: self.parse_component_name()?,
            desc: self.parse_extern_desc()?,
        })
    }

    fn parse_component_export(&mut self) -> Result<ComponentExport> {
        Ok(ComponentExport {
            name: self.parse_component_name()?,
            sort: self.parse_component_sort()?,
            idx: self.read_u32()?,
            desc: match self.read_u8()? {
                0x00 => None,
                0x01 => Some(self.parse_extern_desc()?),
                b => parse_err!("unknown extern desc option: {b:#x}"),
            },
        })
    }

    fn parse_component_name(&mut self) -> Result<String> {
        let kind = self.read_u8()?;
        let name = self.parse_name()?;
        if kind == 1 {
            let _version = self.parse_name()?;
        }

        Ok(name)
    }

    fn parse_extern_desc(&mut self) -> Result<ExternDesc> {
        let b = self.read_u8()?;
        self.parse_extern_desc_with_byte(b)
    }

    fn parse_extern_desc_with_byte(&mut self, b: u8) -> Result<ExternDesc> {
        let out = match b {
            0x00 => {
                let sort = self.read_u8()?;
                ensure!(
                    sort == 0x11,
                    Error::Parse(format!("expected core module sort (0x11), got {sort:#x}"))
                );
                ExternDesc::CoreModule(self.read_u32()?)
            }
            0x01 => ExternDesc::Func(self.read_u32()?),
            0x03 => {
                let bound = match self.read_u8()? {
                    0 => TypeBound::Eq(self.read_u32()?),
                    1 => TypeBound::SubResource,
                    b => parse_err!("unknown type bound: {b:#x}"),
                };
                ExternDesc::Type(bound)
            }
            0x04 => ExternDesc::Component(self.read_u32()?),
            0x05 => ExternDesc::Instance(self.read_u32()?),
            b => parse_err!("unknown extern desc: {b:#x}"),
        };

        Ok(out)
    }

    fn parse_canonical_def(&mut self) -> Result<CanonicalDef> {
        let opcode = self.read_u8()?;
        let out = match opcode {
            0 => {
                let second = self.read_u8()?;

                ensure!(
                    second == 0,
                    Error::Parse(format!("expected 0x00 after canonical lift, got {second}"))
                );

                CanonicalDef::Lift {
                    core_func_idx: self.read_u32()?,
                    opts: self.parse_canon_opts()?,
                    type_idx: self.read_u32()?,
                }
            }
            1 => {
                let second = self.read_u8()?;

                ensure!(
                    second == 0,
                    Error::Parse(format!(
                        "expected 0x00 after canonical lower, got {second:#x}"
                    ))
                );

                CanonicalDef::Lower {
                    func_idx: self.read_u32()?,
                    opts: self.parse_canon_opts()?,
                }
            }
            2 => CanonicalDef::ResourceNew(self.read_u32()?),
            3 => CanonicalDef::ResourceDrop(self.read_u32()?),
            4 => CanonicalDef::ResourceRep(self.read_u32()?),
            5 => CanonicalDef::TaskCancel,
            6 => CanonicalDef::SubtaskCancel {
                async_: self.read_u8()? != 0,
            },
            8 => CanonicalDef::BackpressureSet,
            9 => CanonicalDef::TaskReturn {
                result_type: self.parse_result_list()?,
                opts: self.parse_canon_opts()?,
            },
            10 => {
                let _valtype = self.read_u8()?;
                CanonicalDef::ContextGet {
                    slot: self.read_u32()?,
                }
            }
            11 => {
                let _valtype = self.read_u8()?;
                CanonicalDef::ContextSet {
                    slot: self.read_u32()?,
                }
            }
            12 => CanonicalDef::ThreadYield {
                cancel: self.read_u8()? != 0,
            },
            13 => CanonicalDef::SubtaskDrop,
            14 => CanonicalDef::StreamNew(self.read_u32()?),
            15 => CanonicalDef::StreamRead {
                type_idx: self.read_u32()?,
                opts: self.parse_canon_opts()?,
            },
            16 => CanonicalDef::StreamWrite {
                type_idx: self.read_u32()?,
                opts: self.parse_canon_opts()?,
            },
            17 => CanonicalDef::StreamCancelRead {
                type_idx: self.read_u32()?,
                async_: self.read_u8()? != 0,
            },
            18 => CanonicalDef::StreamCancelWrite {
                type_idx: self.read_u32()?,
                async_: self.read_u8()? != 0,
            },
            19 => CanonicalDef::StreamDropReadable(self.read_u32()?),
            20 => CanonicalDef::StreamDropWritable(self.read_u32()?),
            21 => CanonicalDef::FutureNew(self.read_u32()?),
            22 => CanonicalDef::FutureRead {
                type_idx: self.read_u32()?,
                opts: self.parse_canon_opts()?,
            },
            23 => CanonicalDef::FutureWrite {
                type_idx: self.read_u32()?,
                opts: self.parse_canon_opts()?,
            },
            24 => CanonicalDef::FutureCancelRead {
                type_idx: self.read_u32()?,
                async_: self.read_u8()? != 0,
            },
            25 => CanonicalDef::FutureCancelWrite {
                type_idx: self.read_u32()?,
                async_: self.read_u8()? != 0,
            },
            26 => CanonicalDef::FutureDropReadable(self.read_u32()?),
            27 => CanonicalDef::FutureDropWritable(self.read_u32()?),
            28 => CanonicalDef::ErrorContextNew(self.parse_canon_opts()?),
            29 => CanonicalDef::ErrorContextDebugMessage(self.parse_canon_opts()?),
            30 => CanonicalDef::ErrorContextDrop,
            31 => CanonicalDef::WaitableSetNew,
            32 => CanonicalDef::WaitableSetWait {
                cancel: self.read_u8()? != 0,
                memory: self.read_u32()?,
            },
            33 => CanonicalDef::WaitableSetPoll {
                cancel: self.read_u8()? != 0,
                memory: self.read_u32()?,
            },
            34 => CanonicalDef::WaitableSetDrop,
            35 => CanonicalDef::WaitableJoin,
            36 => CanonicalDef::BackpressureInc,
            37 => CanonicalDef::BackpressureDec,
            38 => CanonicalDef::ThreadIndex,
            39 => CanonicalDef::ThreadNewIndirect {
                type_idx: self.read_u32()?,
                table: self.read_u32()?,
            },
            40 => CanonicalDef::ThreadSuspendToSuspended {
                cancel: self.read_u8()? != 0,
            },
            41 => CanonicalDef::ThreadSuspend {
                cancel: self.read_u8()? != 0,
            },
            42 => CanonicalDef::ThreadUnsuspend,
            43 => CanonicalDef::ThreadYieldToSuspended {
                cancel: self.read_u8()? != 0,
            },
            44 => CanonicalDef::ThreadSuspendTo {
                cancel: self.read_u8()? != 0,
            },
            64 => CanonicalDef::ThreadSpawnRef {
                shared: self.read_u8()? != 0,
                type_idx: self.read_u32()?,
            },
            65 => CanonicalDef::ThreadSpawnIndirect {
                shared: self.read_u8()? != 0,
                type_idx: self.read_u32()?,
                table: self.read_u32()?,
            },
            66 => CanonicalDef::ThreadAvailableParallelism {
                shared: self.read_u8()? != 0,
            },
            b => parse_err!("unknown canonical opcode: {b:#x}"),
        };

        Ok(out)
    }

    fn parse_canon_opts(&mut self) -> Result<CanonOpts> {
        let count = self.read_u32()?;
        let mut opts = CanonOpts::default();
        for _ in 0..count {
            match self.read_u8()? {
                0 => opts.string_encoding = StringEncoding::Utf8,
                1 => opts.string_encoding = StringEncoding::Utf16,
                2 => opts.string_encoding = StringEncoding::Latin1Utf16,
                3 => opts.memory = Some(self.read_u32()?),
                4 => opts.realloc = Some(self.read_u32()?),
                5 => opts.post_return = Some(self.read_u32()?),
                6 => opts.async_ = true,
                7 => opts.callback = Some(self.read_u32()?),
                b => parse_err!("unknown canon opt: {b:#x}"),
            }
        }
        Ok(opts)
    }

    fn parse_result_list(&mut self) -> Result<Option<ComponentValType>> {
        let out = match self.read_u8()? {
            0x00 => Some(self.parse_component_val_type()?),
            0x01 => {
                let _ = self.read_u8()?;
                None
            }
            b => parse_err!("unknown resultlist discriminant: {b:#x}"),
        };

        Ok(out)
    }

    fn parse_core_type(&mut self) -> Result<CoreType> {
        let byte = self.peek_u8()?;
        let out = if byte == 0x50 {
            self.cursor += 1;

            CoreType::Module(self.parse_core_module_type()?)
        } else if byte == 0x00 {
            let saved = self.cursor;
            self.cursor += 1;

            if self.peek_u8()? == 0x50 {
                self.cursor += 1;
                CoreType::SubType(SubType {
                    is_final: false,
                    supertypes: self.parse_vec(Self::read_u32)?,
                    composite_type: self.parse_composite_type()?,
                })
            } else {
                self.cursor = saved;
                CoreType::SubType(self.parse_sub_type()?)
            }
        } else {
            CoreType::SubType(self.parse_sub_type()?)
        };

        Ok(out)
    }

    fn parse_core_module_type(&mut self) -> Result<CoreModuleType> {
        Ok(CoreModuleType {
            declarations: self.parse_vec(Self::parse_core_module_decl)?,
        })
    }

    fn parse_core_module_decl(&mut self) -> Result<CoreModuleDecl> {
        let out = match self.read_u8()? {
            0 => CoreModuleDecl::Import(self.parse_import()?),
            1 => CoreModuleDecl::Type(self.parse_sub_type()?),
            2 => CoreModuleDecl::Alias(self.parse_alias()?),
            3 => CoreModuleDecl::Export(CoreExportDecl {
                name: self.parse_name()?,
                description: match self.read_u8()? {
                    0x00 => ImportDescription::Func(self.read_u32()?),
                    0x01 => ImportDescription::Table(self.parse_table_type()?),
                    0x02 => ImportDescription::Mem(self.parse_memory_type()?),
                    0x03 => ImportDescription::Global(self.parse_global_type()?),
                    0x04 => {
                        let _attribute = self.read_u8()?;
                        ImportDescription::Tag(self.read_u32()?)
                    }
                    b => parse_err!("unknown core export desc: {b:#x}"),
                },
            }),
            b => parse_err!("unknown core module decl: {b:#x}"),
        };

        Ok(out)
    }

    fn parse_component_type_def(&mut self) -> Result<ComponentTypeDef> {
        let out = match self.peek_u8()? {
            0x40 => {
                self.cursor += 1;
                ComponentTypeDef::Func(self.parse_component_func_type()?)
            }
            0x41 => {
                self.cursor += 1;
                ComponentTypeDef::Component(self.parse_vec(Self::parse_component_type_decl)?)
            }
            0x42 => {
                self.cursor += 1;
                ComponentTypeDef::Instance(self.parse_vec(Self::parse_instance_type_decl)?)
            }
            0x3f => {
                self.cursor += 1;
                let _rep = self.read_u8()?;
                ComponentTypeDef::Resource {
                    dtor: match self.read_u8()? {
                        0 => None,
                        1 => Some(self.read_u32()?),
                        b => parse_err!("expected 0x00 or 0x01 for resource dtor flag, got {b:#x}"),
                    },
                }
            }
            _ => ComponentTypeDef::Defined(self.parse_component_defined_type()?),
        };

        Ok(out)
    }

    fn parse_component_func_type(&mut self) -> Result<ComponentFuncType> {
        Ok(ComponentFuncType {
            params: self.parse_vec(Self::parse_labeled_val_type)?,
            results: {
                match self.read_u8()? {
                    0x00 => ComponentFuncResult::Unnamed(self.parse_component_val_type()?),
                    0x01 => {
                        ComponentFuncResult::Named(self.parse_vec(Self::parse_labeled_val_type)?)
                    }
                    b => parse_err!("expected 0x00 or 0x01 for func result tag, got {b:#x}"),
                }
            },
        })
    }

    fn parse_labeled_val_type(&mut self) -> Result<(String, ComponentValType)> {
        Ok((self.parse_name()?, self.parse_component_val_type()?))
    }

    fn parse_component_val_type(&mut self) -> Result<ComponentValType> {
        let b = self.peek_u8()?;
        let out = match b {
            0x68..=0x7f => {
                self.cursor += 1;

                ComponentValType::Primitive(PrimitiveValType::from_byte(b)?)
            }
            _ => ComponentValType::Type(self.read_u32()?),
        };

        Ok(out)
    }

    fn parse_component_defined_type(&mut self) -> Result<ComponentDefinedType> {
        let out = match self.read_u8()? {
            0x72 => ComponentDefinedType::Record(self.parse_vec(Self::parse_labeled_val_type)?),
            0x71 => ComponentDefinedType::Variant(self.parse_vec(Self::parse_variant_case)?),
            0x70 => ComponentDefinedType::List(self.parse_component_val_type()?),
            0x6f => ComponentDefinedType::Tuple(self.parse_vec(Self::parse_component_val_type)?),
            0x6e => ComponentDefinedType::Flags(self.parse_vec(Self::parse_name)?),
            0x6d => ComponentDefinedType::Enum(self.parse_vec(Self::parse_name)?),
            0x6b => ComponentDefinedType::Option(self.parse_component_val_type()?),
            0x6a => ComponentDefinedType::Result {
                ok: self.parse_optional_val_type()?,
                err: self.parse_optional_val_type()?,
            },
            0x69 => ComponentDefinedType::Own(self.read_u32()?),
            0x68 => ComponentDefinedType::Borrow(self.read_u32()?),
            b @ 0x73..=0x7f => ComponentDefinedType::Primitive(PrimitiveValType::from_byte(b)?),
            b => parse_err!("unknown component defined type: {b:#x}"),
        };

        Ok(out)
    }

    fn parse_optional_val_type(&mut self) -> Result<Option<ComponentValType>> {
        let out = match self.read_u8()? {
            0 => None,
            1 => Some(self.parse_component_val_type()?),
            b => parse_err!("expected 0x00 or 0x01 for optional valtype, got {b:#x}"),
        };

        Ok(out)
    }

    fn parse_variant_case(&mut self) -> Result<VariantCase> {
        let name = self.parse_name()?;
        let ty = self.parse_optional_val_type()?;
        let refines = match self.read_u8()? {
            0 => None,
            1 => Some(self.read_u32()?),
            b => parse_err!("expected 0x00 or 0x01 for variant refines, got {b:#x}"),
        };
        Ok(VariantCase { name, ty, refines })
    }

    fn parse_instance_type_decl(&mut self) -> Result<InstanceTypeDecl> {
        let tag = self.read_u8()?;
        let out = match tag {
            0 => InstanceTypeDecl::CoreType(self.parse_core_type()?),
            1 => InstanceTypeDecl::Type(self.parse_component_type_def()?),
            2 => InstanceTypeDecl::Alias(self.parse_alias()?),
            4 => InstanceTypeDecl::Export(ComponentExportDecl {
                name: self.parse_component_name()?,
                desc: self.parse_extern_desc()?,
            }),
            b => parse_err!("unknown instance type decl: {b:#x}"),
        };

        Ok(out)
    }

    fn parse_component_type_decl(&mut self) -> Result<ComponentTypeDecl> {
        let byte = self.peek_u8()?;
        let out = if byte == 3 {
            self.cursor += 1;
            ComponentTypeDecl::Import(self.parse_component_import()?)
        } else {
            ComponentTypeDecl::Instance(self.parse_instance_type_decl()?)
        };

        Ok(out)
    }

    fn parse_module(&mut self) -> Result<ParsedModule> {
        let mut module = ParsedModule::default();
        let mut data_count: Option<u32> = None;
        let mut last_non_custom_id: u8 = 0;
        let mut has_function_section = false;
        let mut has_code_section = false;
        let mut function_count = 0;
        let mut code_count = 0;

        while self.cursor < self.buffer.len() {
            let id = self.read_u8()?;

            if id != 0 {
                let order = section_logical_order(id);
                ensure!(
                    order > 0 && order > last_non_custom_id,
                    Error::Parse(format!(
                        "unexpected content after last section: section {} after previous",
                        id
                    ))
                );
                last_non_custom_id = order;
            }

            match self.parse_module_section(id)? {
                ModuleSection::Custom(custom) => module.customs.push(custom),
                ModuleSection::Type(TypeSection { mut types }) => module.types.append(&mut types),
                ModuleSection::Import(ImportSection {
                    mut import_declarations,
                }) => module.import_declarations.append(&mut import_declarations),
                ModuleSection::Function(FunctionSection { indices }) => {
                    has_function_section = true;
                    function_count = indices.len() as u32;
                    self.function_types.extend(indices)
                }
                ModuleSection::Table(TableSection { mut tables }) => {
                    module.tables.append(&mut tables)
                }
                ModuleSection::Memory(MemorySection { mut memories }) => {
                    module.mems.append(&mut memories)
                }
                ModuleSection::Global(GlobalSection { mut globals }) => {
                    module.globals.append(&mut globals)
                }
                ModuleSection::Export(ExportSection { mut exports }) => {
                    module.exports.append(&mut exports)
                }
                ModuleSection::Start(idx) => module.start = Some(idx),
                ModuleSection::Element(ElementSection { mut elements }) => {
                    module.element_segments.append(&mut elements)
                }
                ModuleSection::Code(CodeSection { mut codes }) => {
                    has_code_section = true;
                    code_count = codes.len() as u32;
                    module.functions.append(&mut codes);
                }
                ModuleSection::Data(DataSection { mut data_segments }) => {
                    module.data_segments.append(&mut data_segments)
                }
                ModuleSection::DataCount(n) => data_count = Some(n),
                ModuleSection::Tag(TagSection { mut tags }) => module.tags.append(&mut tags),
            }
        }

        if has_function_section || has_code_section {
            ensure!(
                has_function_section && has_code_section && function_count == code_count,
                Error::Parse(format!(
                    "function and code section have inconsistent lengths: {} functions vs {} code entries",
                    if has_function_section { function_count } else { 0 },
                    if has_code_section { code_count } else { 0 },
                ))
            );
        }

        if let Some(count) = data_count {
            ensure!(
                count as usize == module.data_segments.len(),
                Error::Parse(format!(
                    "Data count {} does not match number of data segments {}",
                    count,
                    module.data_segments.len()
                ))
            );
        }

        if self.uses_data_count_instructions {
            ensure!(
                data_count.is_some(),
                Error::Parse("data count section required".into())
            );
        }

        Ok(module)
    }

    fn parse_preamble(&mut self) -> Result<(u8, u8)> {
        ensure!(
            self.read_slice(4)? == b"\0asm",
            Error::Parse("Expected magic number in preamble.".into())
        );

        let preamble = self.read_slice(4)?;
        Ok((preamble[0], preamble[2]))
    }

    fn peek_u8(&self) -> Result<u8> {
        self.buffer
            .get(self.cursor)
            .copied()
            .ok_or_else(|| Error::Parse("oob".into()))
    }

    fn read_u8(&mut self) -> Result<u8> {
        let b = self.peek_u8()?;
        self.cursor += 1;

        Ok(b)
    }

    fn read_u32(&mut self) -> Result<u32> {
        let buf = self.peek_leb_slice::<MAX_LEB128_LEN_32>()?;

        let (out, seen) = leb128::read_u32(buf)?;
        self.cursor += seen;

        Ok(out)
    }

    fn read_i32(&mut self) -> Result<i32> {
        let buf = self.peek_leb_slice::<MAX_LEB128_LEN_32>()?;

        let (out, seen) = leb128::read_i32(buf)?;
        self.cursor += seen;

        Ok(out)
    }

    fn read_u64(&mut self) -> Result<u64> {
        let buf = self.peek_leb_slice::<MAX_LEB128_LEN_64>()?;

        let (out, seen) = leb128::read_u64(buf)?;
        self.cursor += seen;

        Ok(out)
    }

    fn read_i64(&mut self) -> Result<i64> {
        let buf = self.peek_leb_slice::<MAX_LEB128_LEN_64>()?;

        let (out, seen) = leb128::read_i64(buf)?;
        self.cursor += seen;

        Ok(out)
    }

    fn read_f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.read_slice(4)?.try_into()?))
    }

    fn read_f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.read_slice(8)?.try_into()?))
    }

    fn peek_leb_slice<const MAX_LEB128_LEN: usize>(&self) -> Result<&'a [u8]> {
        let max_len = min(self.buffer.len() - self.cursor, MAX_LEB128_LEN);

        self.peek_slice(max_len)
    }

    fn peek_slice(&self, len: usize) -> Result<&'a [u8]> {
        self.buffer
            .get(self.cursor..self.cursor + len)
            .ok_or_else(|| Error::Parse("oob".into()))
    }

    fn read_slice(&mut self, len: usize) -> Result<&'a [u8]> {
        let buf = self.peek_slice(len)?;
        self.cursor += len;

        Ok(buf)
    }

    fn parse_vec<T>(&mut self, parse: impl Fn(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        let len = self.read_u32()?;

        let mut items = Vec::with_capacity(len as usize);

        for _ in 0..len {
            items.push(parse(self)?);
        }

        Ok(items)
    }

    // 2.5.6: Global
    fn parse_global(&mut self) -> Result<Global> {
        Ok(Global {
            global_type: self.parse_global_type()?,
            initial_expression: self.parse_expression()?,
        })
    }

    // 5.2: Values
    fn parse_name(&mut self) -> Result<String> {
        let n = self.read_u32()?;
        let slice = self.read_slice(n as usize)?;

        Ok(std::str::from_utf8(slice)?.to_owned())
    }

    // 5.3: Types
    fn parse_abs_heap_type(&mut self) -> Result<HeapType> {
        let ht = match self.read_u8()? {
            0x70 => HeapType::Func,
            0x6F => HeapType::Extern,
            0x6E => HeapType::Any,
            0x6D => HeapType::Eq,
            0x6C => HeapType::I31,
            0x6B => HeapType::Struct,
            0x6A => HeapType::Array,
            0x69 => HeapType::Exn,
            0x71 => HeapType::None,
            0x72 => HeapType::NoExtern,
            0x73 => HeapType::NoFunc,
            0x74 => HeapType::NoExn,
            foreign => parse_err!("Unrecognized abstract heap type byte: {}", foreign),
        };
        Ok(ht)
    }

    fn parse_heap_type(&mut self) -> Result<HeapType> {
        let byte = self.peek_u8()?;
        match byte {
            0x69..=0x74 => self.parse_abs_heap_type(),
            _ => {
                // Type index encoded as s33 (positive signed integer)
                let idx = self.read_i64()?;
                ensure!(
                    idx >= 0,
                    Error::Parse(format!("heap type index must be non-negative, got {}", idx))
                );
                Ok(HeapType::TypeIndex(idx as u32))
            }
        }
    }

    fn parse_reference_type(&mut self) -> Result<RefType> {
        let byte = self.peek_u8()?;
        let r = match byte {
            0x70 => {
                self.cursor += 1;
                RefType::FuncRef
            }
            0x6F => {
                self.cursor += 1;
                RefType::ExternRef
            }
            0x63 => {
                self.cursor += 1;
                let ht = self.parse_heap_type()?;
                RefType::Ref {
                    nullable: true,
                    heap_type: ht,
                }
            }
            0x64 => {
                self.cursor += 1;
                let ht = self.parse_heap_type()?;
                RefType::Ref {
                    nullable: false,
                    heap_type: ht,
                }
            }
            // Abstract heap type shorthands → ref null ht
            0x69..=0x6E | 0x71..=0x74 => {
                let ht = self.parse_abs_heap_type()?;
                RefType::Ref {
                    nullable: true,
                    heap_type: ht,
                }
            }
            foreign => parse_err!("Unrecognized reference byte. Got: {}", foreign),
        };
        Ok(r)
    }

    fn parse_value_type(&mut self) -> Result<ValueType> {
        let byte = self.peek_u8()?;
        let value_type = match byte {
            0x7F => {
                self.cursor += 1;
                ValueType::I32
            }
            0x7E => {
                self.cursor += 1;
                ValueType::I64
            }
            0x7D => {
                self.cursor += 1;
                ValueType::F32
            }
            0x7C => {
                self.cursor += 1;
                ValueType::F64
            }
            0x7B => {
                self.cursor += 1;
                ValueType::V128
            }
            // Reference types (includes 0x70, 0x6F, 0x63, 0x64, 0x69-0x6E, 0x71-0x74)
            0x63 | 0x64 | 0x69..=0x74 => ValueType::Ref(self.parse_reference_type()?),
            foreign => parse_err!("Unrecognized type. Got: {}", foreign),
        };
        Ok(value_type)
    }

    fn parse_result_type(&mut self) -> Result<ResultType> {
        Ok(ResultType(self.parse_vec(Self::parse_value_type)?))
    }

    fn parse_mutability(&mut self) -> Result<Mutability> {
        match self.read_u8()? {
            0x00 => Ok(Mutability::Const),
            0x01 => Ok(Mutability::Var),
            foreign => parse_err!(
                "Unrecognized mutability byte. Expected 0x00 or 0x01, Got: {}",
                foreign
            ),
        }
    }

    fn parse_storage_type(&mut self) -> Result<StorageType> {
        let byte = self.peek_u8()?;
        match byte {
            0x78 => {
                self.cursor += 1;
                Ok(StorageType::I8)
            }
            0x77 => {
                self.cursor += 1;
                Ok(StorageType::I16)
            }
            _ => Ok(StorageType::Val(self.parse_value_type()?)),
        }
    }

    fn parse_field_type(&mut self) -> Result<FieldType> {
        let storage_type = self.parse_storage_type()?;
        let mutability = self.parse_mutability()?;
        Ok(FieldType {
            storage_type,
            mutability,
        })
    }

    fn parse_composite_type(&mut self) -> Result<CompositeType> {
        let b = self.read_u8()?;
        match b {
            0x60 => {
                let arg_type = self.parse_result_type()?;
                let return_type = self.parse_result_type()?;
                Ok(CompositeType::Func(FunctionType(arg_type, return_type)))
            }
            0x5E => {
                let field_type = self.parse_field_type()?;
                Ok(CompositeType::Array(ArrayType { field_type }))
            }
            0x5F => {
                let fields = self.parse_vec(Self::parse_field_type)?;
                Ok(CompositeType::Struct(StructType { fields }))
            }
            _ => parse_err!(
                "Expected composite type (0x5E/0x5F/0x60), got: 0x{:02X} at pos {}",
                b,
                self.cursor - 1
            ),
        }
    }

    fn parse_sub_type(&mut self) -> Result<SubType> {
        let byte = self.peek_u8()?;
        match byte {
            0x4F => {
                self.cursor += 1;
                let supertypes = self.parse_vec(Self::read_u32)?;
                let composite_type = self.parse_composite_type()?;
                Ok(SubType {
                    is_final: true,
                    supertypes,
                    composite_type,
                })
            }
            0x50 => {
                self.cursor += 1;
                let supertypes = self.parse_vec(Self::read_u32)?;
                let composite_type = self.parse_composite_type()?;
                Ok(SubType {
                    is_final: false,
                    supertypes,
                    composite_type,
                })
            }
            _ => {
                let composite_type = self.parse_composite_type()?;
                Ok(SubType {
                    is_final: true,
                    supertypes: vec![],
                    composite_type,
                })
            }
        }
    }

    fn parse_rec_type(&mut self) -> Result<Vec<SubType>> {
        let byte = self.peek_u8()?;
        if byte == 0x4E {
            self.cursor += 1;
            Ok(self.parse_vec(Self::parse_sub_type)?)
        } else {
            Ok(vec![self.parse_sub_type()?])
        }
    }

    fn parse_limit(&mut self) -> Result<(AddrType, Limit)> {
        let flag = self.read_u8()?;
        let has_max = flag & 0x01 != 0;
        let is_64 = flag & 0x04 != 0;
        let has_page_size = flag & 0x08 != 0;

        if flag & !0x0D != 0 {
            parse_err!("Expected valid limit flag. Got: 0x{:02X}", flag);
        }

        let addr_type = if is_64 { AddrType::I64 } else { AddrType::I32 };
        let limit = if is_64 {
            Limit {
                min: self.read_u64()?,
                max: if has_max { self.read_u64()? } else { u64::MAX },
            }
        } else {
            Limit {
                min: self.read_u32()? as u64,
                max: if has_max {
                    self.read_u32()? as u64
                } else {
                    u64::MAX
                },
            }
        };

        if has_page_size {
            let _page_size = self.read_u32()?;
        }

        Ok((addr_type, limit))
    }

    fn parse_memory_type(&mut self) -> Result<MemoryType> {
        let (addr_type, limit) = self.parse_limit()?;
        Ok(MemoryType { addr_type, limit })
    }

    fn parse_table_type(&mut self) -> Result<TableType> {
        let element_reference_type = self.parse_reference_type()?;
        let (addr_type, limit) = self.parse_limit()?;
        Ok(TableType {
            element_reference_type,
            addr_type,
            limit,
        })
    }

    fn parse_global_type(&mut self) -> Result<GlobalType> {
        Ok(GlobalType {
            value_type: self.parse_value_type()?,
            mutability: match self.read_u8()? {
                0x00 => Mutability::Const,
                0x01 => Mutability::Var,
                foreign => parse_err!(
                    "Unrecognized mutability byte. Expected 0x00 or 0x01, Got: {}",
                    foreign
                ),
            },
        })
    }

    // 5.4: Instructions

    fn parse_block_type(&mut self) -> Result<BlockType> {
        let byte = self.peek_u8()?;
        if byte == 0x40 {
            self.cursor += 1;
            Ok(BlockType::Empty)
        } else if matches!(byte,
            0x7B..=0x7F |                    // numtype + vectype
            0x70 | 0x6F |                    // funcref, externref
            0x63 | 0x64 |                    // ref null ht, ref ht
            0x69..=0x6E | 0x71..=0x74        // abstract heap type shorthands
        ) {
            Ok(BlockType::SingleValue(self.parse_value_type()?))
        } else {
            Ok(BlockType::TypeIndex(self.read_i32()?))
        }
    }

    fn parse_memarg(&mut self) -> Result<MemArg> {
        let align_raw = self.read_u32()?;
        let (align, memory) = if align_raw & (1 << 6) != 0 {
            let align = align_raw & !(1 << 6);
            let mem_idx = self.read_u32()?;
            (align, mem_idx)
        } else {
            (align_raw, 0)
        };

        ensure!(align <= 4, Error::Parse("malformed memop flags".into()));

        Ok(MemArg {
            align,
            offset: self.read_u64()?,
            memory,
        })
    }

    fn parse_catch_clause(&mut self) -> Result<CatchClause> {
        let kind = self.read_u8()?;
        match kind {
            0x00 => Ok(CatchClause::Catch {
                tag: self.read_u32()?,
                label: self.read_u32()?,
            }),
            0x01 => Ok(CatchClause::CatchRef {
                tag: self.read_u32()?,
                label: self.read_u32()?,
            }),
            0x02 => Ok(CatchClause::CatchAll {
                label: self.read_u32()?,
            }),
            0x03 => Ok(CatchClause::CatchAllRef {
                label: self.read_u32()?,
            }),
            _ => parse_err!("Unknown catch clause kind: {}", kind),
        }
    }

    fn parse_expression(&mut self) -> Result<Vec<Instruction>> {
        let mut instructions = vec![];

        loop {
            let opcode = self.read_u8()?;

            if opcode == TERM_END_BYTE {
                break;
            }

            let instruction = self.parse_instruction(opcode)?;

            instructions.push(instruction);
        }

        Ok(instructions)
    }

    fn parse_if_else(&mut self) -> Result<(Vec<Instruction>, Vec<Instruction>)> {
        let mut if_else = (vec![], vec![]);

        let mut else_flag = false;

        loop {
            let opcode = self.read_u8()?;

            if opcode == TERM_ELSE_BYTE {
                else_flag = true;
                continue;
            }

            if opcode == TERM_END_BYTE {
                break;
            }

            let instruction = self.parse_instruction(opcode)?;

            if else_flag {
                if_else.1.push(instruction);
            } else {
                if_else.0.push(instruction);
            }
        }

        Ok(if_else)
    }

    fn parse_instruction(&mut self, opcode: u8) -> Result<Instruction> {
        let instr = match opcode {
            0x00 => Instruction::Unreachable,
            0x01 => Instruction::Nop,
            0x02 => Instruction::Block(self.parse_block_type()?, self.parse_expression()?),
            0x03 => Instruction::Loop(self.parse_block_type()?, self.parse_expression()?),
            0x04 => {
                let bt = self.parse_block_type()?;
                let (if_exprs, else_exprs) = self.parse_if_else()?;
                Instruction::IfElse(bt, if_exprs, else_exprs)
            }
            0x08 => Instruction::Throw(self.read_u32()?),
            0x0A => Instruction::ThrowRef,
            0x0C => Instruction::Br(self.read_u32()?),
            0x0D => Instruction::BrIf(self.read_u32()?),
            0x0E => Instruction::BrTable(self.parse_vec(Self::read_u32)?, self.read_u32()?),
            0x0F => Instruction::Return,
            0x10 => Instruction::Call(self.read_u32()?),
            0x11 => Instruction::CallIndirect(self.read_u32()?, self.read_u32()?),
            0x12 => Instruction::ReturnCall(self.read_u32()?),
            0x13 => Instruction::ReturnCallIndirect(self.read_u32()?, self.read_u32()?),
            0x14 => Instruction::CallRef(self.read_u32()?),
            0x15 => Instruction::ReturnCallRef(self.read_u32()?),
            0x1F => {
                let bt = self.parse_block_type()?;
                let catches = self.parse_vec(Self::parse_catch_clause)?;
                let body = self.parse_expression()?;
                Instruction::TryTable(bt, catches, body)
            }
            0xD0 => Instruction::RefNull(self.parse_heap_type()?),
            0xD1 => Instruction::RefIsNull,
            0xD2 => Instruction::RefFunc(self.read_u32()?),
            0xD3 => Instruction::RefEq,
            0xD4 => Instruction::RefAsNonNull,
            0xD5 => Instruction::BrOnNull(self.read_u32()?),
            0xD6 => Instruction::BrOnNonNull(self.read_u32()?),
            0x1A => Instruction::Drop,
            0x1B => Instruction::Select(vec![]),
            0x1C => Instruction::Select(self.parse_vec(Self::parse_value_type)?),
            0x20 => Instruction::LocalGet(self.read_u32()?),
            0x21 => Instruction::LocalSet(self.read_u32()?),
            0x22 => Instruction::LocalTee(self.read_u32()?),
            0x23 => Instruction::GlobalGet(self.read_u32()?),
            0x24 => Instruction::GlobalSet(self.read_u32()?),
            0x25 => Instruction::TableGet(self.read_u32()?),
            0x26 => Instruction::TableSet(self.read_u32()?),
            0xFB => match self.read_u32()? {
                0x00 => Instruction::StructNew(self.read_u32()?),
                0x01 => Instruction::StructNewDefault(self.read_u32()?),
                0x02 => {
                    let type_idx = self.read_u32()?;
                    let field_idx = self.read_u32()?;
                    Instruction::StructGet(type_idx, field_idx)
                }
                0x03 => {
                    let type_idx = self.read_u32()?;
                    let field_idx = self.read_u32()?;
                    Instruction::StructGetSigned(type_idx, field_idx)
                }
                0x04 => {
                    let type_idx = self.read_u32()?;
                    let field_idx = self.read_u32()?;
                    Instruction::StructGetUnsigned(type_idx, field_idx)
                }
                0x05 => {
                    let type_idx = self.read_u32()?;
                    let field_idx = self.read_u32()?;
                    Instruction::StructSet(type_idx, field_idx)
                }
                0x06 => Instruction::ArrayNew(self.read_u32()?),
                0x07 => Instruction::ArrayNewDefault(self.read_u32()?),
                0x08 => {
                    let type_idx = self.read_u32()?;
                    let size = self.read_u32()?;
                    Instruction::ArrayNewFixed(type_idx, size)
                }
                0x09 => {
                    let type_idx = self.read_u32()?;
                    let data_idx = self.read_u32()?;
                    Instruction::ArrayNewData(type_idx, data_idx)
                }
                0x0A => {
                    let type_idx = self.read_u32()?;
                    let elem_idx = self.read_u32()?;
                    Instruction::ArrayNewElem(type_idx, elem_idx)
                }
                0x0B => Instruction::ArrayGet(self.read_u32()?),
                0x0C => Instruction::ArrayGetSigned(self.read_u32()?),
                0x0D => Instruction::ArrayGetUnsigned(self.read_u32()?),
                0x0E => Instruction::ArraySet(self.read_u32()?),
                0x0F => Instruction::ArrayLen,
                0x10 => Instruction::ArrayFill(self.read_u32()?),
                0x11 => {
                    let dst_type_idx = self.read_u32()?;
                    let src_type_idx = self.read_u32()?;
                    Instruction::ArrayCopy(dst_type_idx, src_type_idx)
                }
                0x12 => {
                    let type_idx = self.read_u32()?;
                    let data_idx = self.read_u32()?;
                    Instruction::ArrayInitData(type_idx, data_idx)
                }
                0x13 => {
                    let type_idx = self.read_u32()?;
                    let elem_idx = self.read_u32()?;
                    Instruction::ArrayInitElem(type_idx, elem_idx)
                }
                0x14 => Instruction::RefTest(self.parse_heap_type()?),
                0x15 => Instruction::RefTestNull(self.parse_heap_type()?),
                0x16 => Instruction::RefCast(self.parse_heap_type()?),
                0x17 => Instruction::RefCastNull(self.parse_heap_type()?),
                0x18 => {
                    let flags = self.read_u8()?;
                    let label = self.read_u32()?;
                    let ht1 = self.parse_heap_type()?;
                    let ht2 = self.parse_heap_type()?;
                    Instruction::BrOnCast(flags, label, ht1, ht2)
                }
                0x19 => {
                    let flags = self.read_u8()?;
                    let label = self.read_u32()?;
                    let ht1 = self.parse_heap_type()?;
                    let ht2 = self.parse_heap_type()?;
                    Instruction::BrOnCastFail(flags, label, ht1, ht2)
                }
                0x1A => Instruction::AnyConvertExtern,
                0x1B => Instruction::ExternConvertAny,
                0x1C => Instruction::RefI31,
                0x1D => Instruction::I31GetSigned,
                0x1E => Instruction::I31GetUnsigned,
                foreign => parse_err!("Encountered unknown GC opcode: 0xFB 0x{:02X}", foreign),
            },
            0xFC => match self.read_u32()? {
                0 => Instruction::I32TruncSaturatedF32Signed,
                1 => Instruction::I32TruncSaturatedF32Unsigned,
                2 => Instruction::I32TruncSaturatedF64Signed,
                3 => Instruction::I32TruncSaturatedF64Unsigned,
                4 => Instruction::I64TruncSaturatedF32Signed,
                5 => Instruction::I64TruncSaturatedF32Unsigned,
                6 => Instruction::I64TruncSaturatedF64Signed,
                7 => Instruction::I64TruncSaturatedF64Unsigned,
                8 => {
                    self.uses_data_count_instructions = true;
                    Instruction::MemoryInit(self.read_u32()?, self.read_u32()?)
                }
                9 => {
                    self.uses_data_count_instructions = true;
                    Instruction::DataDrop(self.read_u32()?)
                }
                10 => {
                    let dst_mem = self.read_u32()?;
                    let src_mem = self.read_u32()?;
                    Instruction::MemoryCopy(dst_mem, src_mem)
                }
                11 => {
                    let mem_idx = self.read_u32()?;
                    Instruction::MemoryFill(mem_idx)
                }
                12 => {
                    let y = self.read_u32()?;
                    let x = self.read_u32()?;
                    Instruction::TableInit(x, y)
                }
                13 => Instruction::ElemDrop(self.read_u32()?),
                14 => Instruction::TableCopy(self.read_u32()?, self.read_u32()?),
                15 => Instruction::TableGrow(self.read_u32()?),
                16 => Instruction::TableSize(self.read_u32()?),
                17 => Instruction::TableFill(self.read_u32()?),
                foreign => parse_err!("Encountered foreign table opcode: {}", foreign),
            },
            0x28 => Instruction::I32Load(self.parse_memarg()?),
            0x29 => Instruction::I64Load(self.parse_memarg()?),
            0x2A => Instruction::F32Load(self.parse_memarg()?),
            0x2B => Instruction::F64Load(self.parse_memarg()?),
            0x2C => Instruction::I32Load8Signed(self.parse_memarg()?),
            0x2D => Instruction::I32Load8Unsigned(self.parse_memarg()?),
            0x2E => Instruction::I32Load16Signed(self.parse_memarg()?),
            0x2F => Instruction::I32Load16Unsigned(self.parse_memarg()?),
            0x30 => Instruction::I64Load8Signed(self.parse_memarg()?),
            0x31 => Instruction::I64Load8Unsigned(self.parse_memarg()?),
            0x32 => Instruction::I64Load16Signed(self.parse_memarg()?),
            0x33 => Instruction::I64Load16Unsigned(self.parse_memarg()?),
            0x34 => Instruction::I64Load32Signed(self.parse_memarg()?),
            0x35 => Instruction::I64Load32Unsigned(self.parse_memarg()?),
            0x36 => Instruction::I32Store(self.parse_memarg()?),
            0x37 => Instruction::I64Store(self.parse_memarg()?),
            0x38 => Instruction::F32Store(self.parse_memarg()?),
            0x39 => Instruction::F64Store(self.parse_memarg()?),
            0x3A => Instruction::I32Store8(self.parse_memarg()?),
            0x3B => Instruction::I32Store16(self.parse_memarg()?),
            0x3C => Instruction::I64Store8(self.parse_memarg()?),
            0x3D => Instruction::I64Store16(self.parse_memarg()?),
            0x3E => Instruction::I64Store32(self.parse_memarg()?),
            0x3F => {
                let mem_idx = self.read_u32()?;
                Instruction::MemorySize(mem_idx)
            }
            0x40 => {
                let mem_idx = self.read_u32()?;
                Instruction::MemoryGrow(mem_idx)
            }
            0x41 => Instruction::I32Const(self.read_i32()?),
            0x42 => Instruction::I64Const(self.read_i64()?),
            0x43 => Instruction::F32Const(self.read_f32()?),
            0x44 => Instruction::F64Const(self.read_f64()?),
            0x45 => Instruction::I32EqZero,
            0x46 => Instruction::I32Eq,
            0x47 => Instruction::I32Ne,
            0x48 => Instruction::I32LtSigned,
            0x49 => Instruction::I32LtUnsigned,
            0x4A => Instruction::I32GtSigned,
            0x4B => Instruction::I32GtUnsigned,
            0x4C => Instruction::I32LeSigned,
            0x4D => Instruction::I32LeUnsigned,
            0x4E => Instruction::I32GeSigned,
            0x4F => Instruction::I32GeUnsigned,
            0x50 => Instruction::I64EqZero,
            0x51 => Instruction::I64Eq,
            0x52 => Instruction::I64Ne,
            0x53 => Instruction::I64LtSigned,
            0x54 => Instruction::I64LtUnsigned,
            0x55 => Instruction::I64GtSigned,
            0x56 => Instruction::I64GtUnsigned,
            0x57 => Instruction::I64LeSigned,
            0x58 => Instruction::I64LeUnsigned,
            0x59 => Instruction::I64GeSigned,
            0x5A => Instruction::I64GeUnsigned,
            0x5B => Instruction::F32Eq,
            0x5C => Instruction::F32Ne,
            0x5D => Instruction::F32Lt,
            0x5E => Instruction::F32Gt,
            0x5F => Instruction::F32Le,
            0x60 => Instruction::F32Ge,
            0x61 => Instruction::F64Eq,
            0x62 => Instruction::F64Ne,
            0x63 => Instruction::F64Lt,
            0x64 => Instruction::F64Gt,
            0x65 => Instruction::F64Le,
            0x66 => Instruction::F64Ge,
            0x67 => Instruction::I32CountLeadingZeros,
            0x68 => Instruction::I32CountTrailingZeros,
            0x69 => Instruction::I32PopCount,
            0x6A => Instruction::I32Add,
            0x6B => Instruction::I32Sub,
            0x6C => Instruction::I32Mul,
            0x6D => Instruction::I32DivSigned,
            0x6E => Instruction::I32DivUnsigned,
            0x6F => Instruction::I32RemainderSigned,
            0x70 => Instruction::I32RemainderUnsigned,
            0x71 => Instruction::I32And,
            0x72 => Instruction::I32Or,
            0x73 => Instruction::I32Xor,
            0x74 => Instruction::I32Shl,
            0x75 => Instruction::I32ShrSigned,
            0x76 => Instruction::I32ShrUnsigned,
            0x77 => Instruction::I32RotateLeft,
            0x78 => Instruction::I32RotateRight,
            0x79 => Instruction::I64CountLeadingZeros,
            0x7A => Instruction::I64CountTrailingZeros,
            0x7B => Instruction::I64PopCount,
            0x7C => Instruction::I64Add,
            0x7D => Instruction::I64Sub,
            0x7E => Instruction::I64Mul,
            0x7F => Instruction::I64DivSigned,
            0x80 => Instruction::I64DivUnsigned,
            0x81 => Instruction::I64RemainderSigned,
            0x82 => Instruction::I64RemainderUnsigned,
            0x83 => Instruction::I64And,
            0x84 => Instruction::I64Or,
            0x85 => Instruction::I64Xor,
            0x86 => Instruction::I64Shl,
            0x87 => Instruction::I64ShrSigned,
            0x88 => Instruction::I64ShrUnsigned,
            0x89 => Instruction::I64RotateLeft,
            0x8A => Instruction::I64RotateRight,
            0x8B => Instruction::F32Abs,
            0x8C => Instruction::F32Neg,
            0x8D => Instruction::F32Ceil,
            0x8E => Instruction::F32Floor,
            0x8F => Instruction::F32Trunc,
            0x90 => Instruction::F32Nearest,
            0x91 => Instruction::F32Sqrt,
            0x92 => Instruction::F32Add,
            0x93 => Instruction::F32Sub,
            0x94 => Instruction::F32Mul,
            0x95 => Instruction::F32Div,
            0x96 => Instruction::F32Min,
            0x97 => Instruction::F32Max,
            0x98 => Instruction::F32CopySign,
            0x99 => Instruction::F64Abs,
            0x9A => Instruction::F64Neg,
            0x9B => Instruction::F64Ceil,
            0x9C => Instruction::F64Floor,
            0x9D => Instruction::F64Trunc,
            0x9E => Instruction::F64Nearest,
            0x9F => Instruction::F64Sqrt,
            0xA0 => Instruction::F64Add,
            0xA1 => Instruction::F64Sub,
            0xA2 => Instruction::F64Mul,
            0xA3 => Instruction::F64Div,
            0xA4 => Instruction::F64Min,
            0xA5 => Instruction::F64Max,
            0xA6 => Instruction::F64CopySign,
            0xA7 => Instruction::I32WrapI64,
            0xA8 => Instruction::I32TruncF32Signed,
            0xA9 => Instruction::I32TruncF32Unsigned,
            0xAA => Instruction::I32TruncF64Signed,
            0xAB => Instruction::I32TruncF64Unsigned,
            0xAC => Instruction::I64ExtendI32Signed,
            0xAD => Instruction::I64ExtendI32Unsigned,
            0xAE => Instruction::I64TruncF32Signed,
            0xAF => Instruction::I64TruncF32Unsigned,
            0xB0 => Instruction::I64TruncF64Signed,
            0xB1 => Instruction::I64TruncF64Unsigned,
            0xB2 => Instruction::F32ConvertI32Signed,
            0xB3 => Instruction::F32ConvertI32Unsigned,
            0xB4 => Instruction::F32ConvertI64Signed,
            0xB5 => Instruction::F32ConvertI64Unsigned,
            0xB6 => Instruction::F32DemoteF64,
            0xB7 => Instruction::F64ConvertI32Signed,
            0xB8 => Instruction::F64ConvertI32Unsigned,
            0xB9 => Instruction::F64ConvertI64Signed,
            0xBA => Instruction::F64ConvertI64Unsigned,
            0xBB => Instruction::F64PromoteF32,
            0xBC => Instruction::I32ReinterpretF32,
            0xBD => Instruction::I64ReinterpretF64,
            0xBE => Instruction::F32ReinterpretI32,
            0xBF => Instruction::F64ReinterpretI64,
            0xC0 => Instruction::I32Extend8Signed,
            0xC1 => Instruction::I32Extend16Signed,
            0xC2 => Instruction::I64Extend8Signed,
            0xC3 => Instruction::I64Extend16Signed,
            0xC4 => Instruction::I64Extend32Signed,

            0xFD => match self.read_u32()? {
                0x00 => Instruction::V128Load(self.parse_memarg()?),
                0x01 => Instruction::V128Load8x8Signed(self.parse_memarg()?),
                0x02 => Instruction::V128Load8x8Unsigned(self.parse_memarg()?),
                0x03 => Instruction::V128Load16x4Signed(self.parse_memarg()?),
                0x04 => Instruction::V128Load16x4Unsigned(self.parse_memarg()?),
                0x05 => Instruction::V128Load32x2Signed(self.parse_memarg()?),
                0x06 => Instruction::V128Load32x2Unsigned(self.parse_memarg()?),
                0x07 => Instruction::V128Load8Splat(self.parse_memarg()?),
                0x08 => Instruction::V128Load16Splat(self.parse_memarg()?),
                0x09 => Instruction::V128Load32Splat(self.parse_memarg()?),
                0x0A => Instruction::V128Load64Splat(self.parse_memarg()?),
                0x0B => Instruction::V128Store(self.parse_memarg()?),
                0x0C => {
                    let bytes = self.read_slice(16)?;
                    Instruction::V128Const(i128::from_le_bytes(bytes.try_into()?))
                }
                0x0D => {
                    let mut lanes = [0u8; 16];
                    lanes.copy_from_slice(self.read_slice(16)?);
                    Instruction::I8x16Shuffle(lanes)
                }
                0x0E => Instruction::I8x16Swizzle,
                0x0F => Instruction::I8x16Splat,
                0x10 => Instruction::I16x8Splat,
                0x11 => Instruction::I32x4Splat,
                0x12 => Instruction::I64x2Splat,
                0x13 => Instruction::F32x4Splat,
                0x14 => Instruction::F64x2Splat,
                0x15 => Instruction::I8x16ExtractLaneSigned(self.read_u8()?),
                0x16 => Instruction::I8x16ExtractLaneUnsigned(self.read_u8()?),
                0x17 => Instruction::I8x16ReplaceLane(self.read_u8()?),
                0x18 => Instruction::I16x8ExtractLaneSigned(self.read_u8()?),
                0x19 => Instruction::I16x8ExtractLaneUnsigned(self.read_u8()?),
                0x1A => Instruction::I16x8ReplaceLane(self.read_u8()?),
                0x1B => Instruction::I32x4ExtractLane(self.read_u8()?),
                0x1C => Instruction::I32x4ReplaceLane(self.read_u8()?),
                0x1D => Instruction::I64x2ExtractLane(self.read_u8()?),
                0x1E => Instruction::I64x2ReplaceLane(self.read_u8()?),
                0x1F => Instruction::F32x4ExtractLane(self.read_u8()?),
                0x20 => Instruction::F32x4ReplaceLane(self.read_u8()?),
                0x21 => Instruction::F64x2ExtractLane(self.read_u8()?),
                0x22 => Instruction::F64x2ReplaceLane(self.read_u8()?),
                0x23 => Instruction::I8x16Eq,
                0x24 => Instruction::I8x16Ne,
                0x25 => Instruction::I8x16LtSigned,
                0x26 => Instruction::I8x16LtUnsigned,
                0x27 => Instruction::I8x16GtSigned,
                0x28 => Instruction::I8x16GtUnsigned,
                0x29 => Instruction::I8x16LeSigned,
                0x2A => Instruction::I8x16LeUnsigned,
                0x2B => Instruction::I8x16GeSigned,
                0x2C => Instruction::I8x16GeUnsigned,
                0x2D => Instruction::I16x8Eq,
                0x2E => Instruction::I16x8Ne,
                0x2F => Instruction::I16x8LtSigned,
                0x30 => Instruction::I16x8LtUnsigned,
                0x31 => Instruction::I16x8GtSigned,
                0x32 => Instruction::I16x8GtUnsigned,
                0x33 => Instruction::I16x8LeSigned,
                0x34 => Instruction::I16x8LeUnsigned,
                0x35 => Instruction::I16x8GeSigned,
                0x36 => Instruction::I16x8GeUnsigned,
                0x37 => Instruction::I32x4Eq,
                0x38 => Instruction::I32x4Ne,
                0x39 => Instruction::I32x4LtSigned,
                0x3A => Instruction::I32x4LtUnsigned,
                0x3B => Instruction::I32x4GtSigned,
                0x3C => Instruction::I32x4GtUnsigned,
                0x3D => Instruction::I32x4LeSigned,
                0x3E => Instruction::I32x4LeUnsigned,
                0x3F => Instruction::I32x4GeSigned,
                0x40 => Instruction::I32x4GeUnsigned,
                0x41 => Instruction::F32X4Eq,
                0x42 => Instruction::F32x4Ne,
                0x43 => Instruction::F32x4Lt,
                0x44 => Instruction::F32x4Gt,
                0x45 => Instruction::F32x4Le,
                0x46 => Instruction::F32x4Ge,
                0x47 => Instruction::F64x2Eq,
                0x48 => Instruction::F64x2Ne,
                0x49 => Instruction::F64x2Lt,
                0x4A => Instruction::F64x2Gt,
                0x4B => Instruction::F64x2Le,
                0x4C => Instruction::F64x2Ge,
                0x4D => Instruction::V128Not,
                0x4E => Instruction::V128And,
                0x4F => Instruction::V128AndNot,
                0x50 => Instruction::V128Or,
                0x51 => Instruction::V128Xor,
                0x52 => Instruction::V128BitSelect,
                0x53 => Instruction::V128AnyTrue,
                0x54 => {
                    let memarg = self.parse_memarg()?;
                    let lane = self.read_u8()?;
                    Instruction::V128Load8Lane(memarg, lane)
                }
                0x55 => {
                    let memarg = self.parse_memarg()?;
                    let lane = self.read_u8()?;
                    Instruction::V128Load16Lane(memarg, lane)
                }
                0x56 => {
                    let memarg = self.parse_memarg()?;
                    let lane = self.read_u8()?;
                    Instruction::V128Load32Lane(memarg, lane)
                }
                0x57 => {
                    let memarg = self.parse_memarg()?;
                    let lane = self.read_u8()?;
                    Instruction::V128Load64Lane(memarg, lane)
                }
                0x58 => {
                    let memarg = self.parse_memarg()?;
                    let lane = self.read_u8()?;
                    Instruction::V128Store8Lane(memarg, lane)
                }
                0x59 => {
                    let memarg = self.parse_memarg()?;
                    let lane = self.read_u8()?;
                    Instruction::V128Store16Lane(memarg, lane)
                }
                0x5A => {
                    let memarg = self.parse_memarg()?;
                    let lane = self.read_u8()?;
                    Instruction::V128Store32Lane(memarg, lane)
                }
                0x5B => {
                    let memarg = self.parse_memarg()?;
                    let lane = self.read_u8()?;
                    Instruction::V128Store64Lane(memarg, lane)
                }
                0x5C => Instruction::V128Load32Zero(self.parse_memarg()?),
                0x5D => Instruction::V128Load64Zero(self.parse_memarg()?),
                0x5E => Instruction::F32x4DemoteF64x2Zero,
                0x5F => Instruction::F64xPromoteLowF32x4,
                0x60 => Instruction::I8x16Abs,
                0x61 => Instruction::I8x16Neg,
                0x62 => Instruction::I8x16PopCount,
                0x63 => Instruction::I8x16AllTrue,
                0x64 => Instruction::I8x16BitMask,
                0x65 => Instruction::I8x16NarrowI16x8Signed,
                0x66 => Instruction::I8x16NarrowI16x8Unsigned,
                0x67 => Instruction::F32x4Ceil,
                0x68 => Instruction::F32x4Floor,
                0x69 => Instruction::F32x4Trunc,
                0x6A => Instruction::F32x4Nearest,
                0x6B => Instruction::I8x16Shl,
                0x6C => Instruction::I8x16ShrSigned,
                0x6D => Instruction::I8x16ShrUnsigned,
                0x6E => Instruction::I8x16Add,
                0x6F => Instruction::I8x16AddSaturatedSigned,
                0x70 => Instruction::I8x16AddSaturatedUnsigned,
                0x71 => Instruction::I8x16Sub,
                0x72 => Instruction::I8x16SubSaturatedSigned,
                0x73 => Instruction::I8x16SubSaturatedUnsigned,
                0x74 => Instruction::F64x2Ceil,
                0x75 => Instruction::F64x2Floor,
                0x76 => Instruction::I8x16MinSigned,
                0x77 => Instruction::I8x16MinUnsigned,
                0x78 => Instruction::I8x16MaxSigned,
                0x79 => Instruction::I8x16MaxUnsigned,
                0x7A => Instruction::F64x2Trunc,
                0x7B => Instruction::I8x16AvgRangeUnsigned,
                0x7C => Instruction::I16x8ExtAddPairWiseI8x16Signed,
                0x7D => Instruction::I16x8ExtAddPairWiseI8x16Unsigned,
                0x7E => Instruction::I32x4ExtAddPairWiseI16x8Signed,
                0x7F => Instruction::I32x4ExtAddPairWiseI16x8Unsigned,
                128 => Instruction::I16x8Abs,
                129 => Instruction::I16x8Neg,
                130 => Instruction::I16xQ15MulRangeSaturatedSigned,
                131 => Instruction::I16x8AllTrue,
                132 => Instruction::I16x8BitMask,
                133 => Instruction::I16x8NarrowI32x4Signed,
                134 => Instruction::I16x8NarrowI32x4Unsigned,
                135 => Instruction::I16x8ExtendLowI8x16Signed,
                136 => Instruction::I16x8ExtendHighI8x16Signed,
                137 => Instruction::I16x8ExtendLowI8x16Unsigned,
                138 => Instruction::I16x8ExtendHighI8x16Unsigned,
                139 => Instruction::I16x8Shl,
                140 => Instruction::I16x8ShrSigned,
                141 => Instruction::I16x8ShrUnsigned,
                142 => Instruction::I16x8Add,
                143 => Instruction::I16x8AddSaturatedSigned,
                144 => Instruction::I16x8AddSaturatedUnsigned,
                145 => Instruction::I16x8Sub,
                146 => Instruction::I16x8SubSaturatedSigned,
                147 => Instruction::I16x8SubSaturatedUnsigned,
                148 => Instruction::F64x2Nearest,
                149 => Instruction::I16x8Mul,
                150 => Instruction::I16x8MinSigned,
                151 => Instruction::I16x8MinUnsigned,
                152 => Instruction::I16x8MaxSigned,
                153 => Instruction::I16x8MaxUnsigned,
                155 => Instruction::I16x8AvgRangeUnsigned,
                156 => Instruction::I16x8ExtMulLowI8x16Signed,
                157 => Instruction::I16x8ExtMulHighI8x16Signed,
                158 => Instruction::I16x8ExtMulLowI8x16Unsigned,
                159 => Instruction::I16x8ExtMulHighI8x16Unsigned,
                160 => Instruction::I32x4Abs,
                161 => Instruction::I32x4Neg,
                163 => Instruction::I32x4AllTrue,
                164 => Instruction::I32x4BitMask,
                167 => Instruction::I32x4ExtendLowI16x8Signed,
                168 => Instruction::I32x4ExtendHighI16x8Signed,
                169 => Instruction::I32x4ExtendLowI16x8Unsigned,
                170 => Instruction::I32x4ExtendHighI16x8Unsigned,
                171 => Instruction::I32x4Shl,
                172 => Instruction::I32x4ShrSigned,
                173 => Instruction::I32x4ShrUnsigned,
                174 => Instruction::I32x4Add,
                177 => Instruction::I32x4Sub,
                181 => Instruction::I32x4Mul,
                182 => Instruction::I32x4MinSigned,
                183 => Instruction::I32x4MinUnsigned,
                184 => Instruction::I32x4MaxSigned,
                185 => Instruction::I32x4MaxUnsigned,
                186 => Instruction::I32x4DotI16x8Signed,
                188 => Instruction::I32x4ExtMulLowI16x8Signed,
                189 => Instruction::I32x4ExtMulHighI16x8Signed,
                190 => Instruction::I32x4ExtMulLowI16x8Unsigned,
                191 => Instruction::I32x4ExtMulHighI16x8Unsigned,
                192 => Instruction::I64x2Abs,
                193 => Instruction::I64x2Neg,
                195 => Instruction::I64x2AllTrue,
                196 => Instruction::I64x2BitMask,
                199 => Instruction::I64x2ExtendLowI32x4Signed,
                200 => Instruction::I64x2ExtendHighI32x4Signed,
                201 => Instruction::I64x2ExtendLowI32x4Unsigned,
                202 => Instruction::I64x2ExtendHighI32x4Unsigned,
                203 => Instruction::I64x2Shl,
                204 => Instruction::I64x2ShrSigned,
                205 => Instruction::I64x2ShrUnsigned,
                206 => Instruction::I64x2Add,
                209 => Instruction::I64x2Sub,
                213 => Instruction::I64x2Mul,
                220 => Instruction::I64x2ExtMulLowI32x4Signed,
                221 => Instruction::I64x2ExtMulHighI32x4Signed,
                222 => Instruction::I64x2ExtMulLowI32x4Unsigned,
                223 => Instruction::I64x2ExtMulHighI32x4Unsigned,
                224 => Instruction::F32x4Abs,
                225 => Instruction::F32x4Neg,
                227 => Instruction::F32x4Sqrt,
                228 => Instruction::F32x4Add,
                229 => Instruction::F32x4Sub,
                230 => Instruction::F32x4Mul,
                231 => Instruction::F32x4Div,
                232 => Instruction::F32x4Min,
                233 => Instruction::F32x4Max,
                234 => Instruction::F32x4PMin,
                235 => Instruction::F32x4PMax,
                236 => Instruction::F64x2Abs,
                237 => Instruction::F64x2Neg,
                239 => Instruction::F64x2Sqrt,
                240 => Instruction::F64x2Add,
                241 => Instruction::F64x2Sub,
                242 => Instruction::F64x2Mul,
                243 => Instruction::F64x2Div,
                244 => Instruction::F64x2Min,
                245 => Instruction::F64x2Max,
                246 => Instruction::F64x2PMin,
                247 => Instruction::F64x2PMax,
                248 => Instruction::I32x4TruncSaturatedF32x4Signed,
                249 => Instruction::I32x4TruncSaturatedF32x4Unsigned,
                250 => Instruction::F32x4ConvertI32x4Signed,
                251 => Instruction::F32x4ConvertI32x4Unsigned,
                252 => Instruction::I32x4TruncSaturatedF64x2SignedZero,
                253 => Instruction::I32x4TruncSaturatedF64x2UnsignedZero,
                254 => Instruction::F64x2ConvertLowI32x4Signed,
                255 => Instruction::F64x2ConvertLowI32x4Unsigned,
                // i64x2 comparisons
                214 => Instruction::I64x2Eq,
                215 => Instruction::I64x2Ne,
                216 => Instruction::I64x2LtSigned,
                217 => Instruction::I64x2GtSigned,
                218 => Instruction::I64x2LeSigned,
                219 => Instruction::I64x2GeSigned,
                // Relaxed SIMD (0x100+)
                0x100 => Instruction::I8x16RelaxedSwizzle,
                0x101 => Instruction::I32x4RelaxedTruncF32x4Signed,
                0x102 => Instruction::I32x4RelaxedTruncF32x4Unsigned,
                0x103 => Instruction::I32x4RelaxedTruncF64x2SignedZero,
                0x104 => Instruction::I32x4RelaxedTruncF64x2UnsignedZero,
                0x105 => Instruction::F32x4RelaxedMadd,
                0x106 => Instruction::F32x4RelaxedNmadd,
                0x107 => Instruction::F64x2RelaxedMadd,
                0x108 => Instruction::F64x2RelaxedNmadd,
                0x109 => Instruction::I8x16RelaxedLaneselect,
                0x10A => Instruction::I16x8RelaxedLaneselect,
                0x10B => Instruction::I32x4RelaxedLaneselect,
                0x10C => Instruction::I64x2RelaxedLaneselect,
                0x10D => Instruction::F32x4RelaxedMin,
                0x10E => Instruction::F32x4RelaxedMax,
                0x10F => Instruction::F64x2RelaxedMin,
                0x110 => Instruction::F64x2RelaxedMax,
                0x111 => Instruction::I16x8RelaxedQ15mulrSigned,
                0x112 => Instruction::I16x8RelaxedDotI8x16I7x16Signed,
                0x113 => Instruction::I32x4RelaxedDotI8x16I7x16AddSigned,
                foreign => parse_err!("Encountered unknown SIMD opcode: 0xFD 0x{:X}", foreign),
            },
            foreign => parse_err!("Encountered unknown opcode: {}", foreign),
        };

        Ok(instr)
    }

    // 5.5: Modules

    fn parse_custom_section(&mut self, size: u32) -> Result<CustomSection> {
        let current_pos = self.cursor;

        let name = self.parse_name()?;
        ensure!(
            self.cursor - current_pos <= size as usize,
            Error::Parse("custom section name exceeds section size".into())
        );
        let slice_len = size as usize - (self.cursor - current_pos);

        let bytes = self.read_slice(slice_len)?.to_vec();

        Ok(CustomSection { name, bytes })
    }

    fn parse_type_section(&mut self) -> Result<TypeSection> {
        let rec_types = self.parse_vec(Self::parse_rec_type)?;
        Ok(TypeSection {
            types: rec_types.into_iter().flatten().collect(),
        })
    }

    fn parse_import(&mut self) -> Result<ImportDeclaration> {
        Ok(ImportDeclaration {
            module: self.parse_name()?,
            name: self.parse_name()?,
            description: match self.read_u8()? {
                0x00 => ImportDescription::Func(self.read_u32()?),
                0x01 => ImportDescription::Table(self.parse_table_type()?),
                0x02 => ImportDescription::Mem(self.parse_memory_type()?),
                0x03 => ImportDescription::Global(self.parse_global_type()?),
                0x04 => {
                    let _attribute = self.read_u8()?; // tag attribute (0x00)
                    ImportDescription::Tag(self.read_u32()?)
                }
                foreign => parse_err!(
                    "Unrecognized import description. Got: {}, at: {}",
                    foreign,
                    self.cursor
                ),
            },
        })
    }

    fn parse_import_section(&mut self) -> Result<ImportSection> {
        let imports = self.parse_vec(Self::parse_import)?;

        Ok(ImportSection {
            import_declarations: imports,
        })
    }

    fn parse_function_section(&mut self) -> Result<FunctionSection> {
        Ok(FunctionSection {
            indices: self.parse_vec(Self::read_u32)?,
        })
    }

    fn parse_table_def(&mut self) -> Result<TableDef> {
        let byte = self.peek_u8()?;
        if byte == 0x40 {
            // table with init expression: 0x40 0x00 reftype limit expr
            self.cursor += 1;
            ensure!(
                self.read_u8()? == 0x00,
                Error::Parse("Expected 0x00 after 0x40 in table definition".into())
            );
            let table_type = self.parse_table_type()?;
            let init = self.parse_expression()?;
            Ok(TableDef { table_type, init })
        } else {
            let table_type = self.parse_table_type()?;
            let ht = match table_type.element_reference_type {
                RefType::FuncRef => HeapType::Func,
                RefType::ExternRef => HeapType::Extern,
                RefType::Ref { heap_type, .. } => heap_type,
            };
            Ok(TableDef {
                table_type,
                init: vec![Instruction::RefNull(ht)],
            })
        }
    }

    fn parse_table_section(&mut self) -> Result<TableSection> {
        Ok(TableSection {
            tables: self.parse_vec(Self::parse_table_def)?,
        })
    }

    fn parse_memory_section(&mut self) -> Result<MemorySection> {
        Ok(MemorySection {
            memories: self.parse_vec(Self::parse_memory_type)?,
        })
    }

    fn parse_global_section(&mut self) -> Result<GlobalSection> {
        Ok(GlobalSection {
            globals: self.parse_vec(Self::parse_global)?,
        })
    }

    fn parse_tag(&mut self) -> Result<Tag> {
        ensure!(
            self.read_u8()? == 0x00,
            Error::Parse("Expected 0x00 attribute byte for tag.".into())
        );
        Ok(Tag {
            type_index: self.read_u32()?,
        })
    }

    fn parse_tag_section(&mut self) -> Result<TagSection> {
        Ok(TagSection {
            tags: self.parse_vec(Self::parse_tag)?,
        })
    }

    fn parse_export(&mut self) -> Result<Export> {
        Ok(Export {
            name: self.parse_name()?,
            description: match self.read_u8()? {
                0x00 => ExportDescription::Func(self.read_u32()?),
                0x01 => ExportDescription::Table(self.read_u32()?),
                0x02 => ExportDescription::Mem(self.read_u32()?),
                0x03 => ExportDescription::Global(self.read_u32()?),
                0x04 => ExportDescription::Tag(self.read_u32()?),
                foreign => parse_err!(
                    "Encountered foreign byte when parsing export description. Got: {}",
                    foreign
                ),
            },
        })
    }

    fn parse_export_section(&mut self) -> Result<ExportSection> {
        Ok(ExportSection {
            exports: self.parse_vec(Self::parse_export)?,
        })
    }

    fn parse_element_segement(&mut self) -> Result<ElementSegment> {
        let segment = match self.read_u32()? {
            0 => {
                let offset = self.parse_expression()?;
                let expression = self
                    .parse_vec(Self::read_u32)?
                    .into_iter()
                    .map(|idx| vec![Instruction::RefFunc(idx)])
                    .collect::<Vec<_>>();

                ElementSegment {
                    ref_type: RefType::FuncRef,
                    expression,
                    mode: ElementMode::Active {
                        table_index: 0,
                        offset,
                    },
                }
            }
            1 => {
                ensure!(
                    self.read_u8()? == 0x00,
                    Error::Parse("Expected elemkind 0x00.".into())
                );

                let expression = self
                    .parse_vec(Self::read_u32)?
                    .into_iter()
                    .map(|idx| vec![Instruction::RefFunc(idx)])
                    .collect::<Vec<_>>();

                ElementSegment {
                    ref_type: RefType::FuncRef,
                    expression,
                    mode: ElementMode::Passive,
                }
            }
            2 => {
                let table_index = self.read_u32()?;
                let offset = self.parse_expression()?;
                ensure!(
                    self.read_u8()? == 0x00,
                    Error::Parse("Expected elemkind 0x00.".into())
                );

                let expression = self
                    .parse_vec(Self::read_u32)?
                    .into_iter()
                    .map(|idx| vec![Instruction::RefFunc(idx)])
                    .collect::<Vec<_>>();

                ElementSegment {
                    ref_type: RefType::FuncRef,
                    expression,
                    mode: ElementMode::Active {
                        table_index,
                        offset,
                    },
                }
            }
            3 => {
                ensure!(
                    self.read_u8()? == 0x00,
                    Error::Parse("Expected elemkind 0x00.".into())
                );

                let expression = self
                    .parse_vec(Self::read_u32)?
                    .into_iter()
                    .map(|idx| vec![Instruction::RefFunc(idx)])
                    .collect::<Vec<_>>();

                ElementSegment {
                    ref_type: RefType::FuncRef,
                    expression,
                    mode: ElementMode::Declarative,
                }
            }
            4 => {
                let offset = self.parse_expression()?;
                let expression = self.parse_vec(Self::parse_expression)?;

                ElementSegment {
                    ref_type: RefType::FuncRef,
                    expression,
                    mode: ElementMode::Active {
                        table_index: 0,
                        offset,
                    },
                }
            }
            5 => ElementSegment {
                ref_type: self.parse_reference_type()?,
                expression: self.parse_vec(Self::parse_expression)?,
                mode: ElementMode::Passive,
            },
            6 => {
                let table_index = self.read_u32()?;
                let offset = self.parse_expression()?;
                let ref_type = self.parse_reference_type()?;
                let expression = self.parse_vec(Self::parse_expression)?;

                ElementSegment {
                    ref_type,
                    expression,
                    mode: ElementMode::Active {
                        table_index,
                        offset,
                    },
                }
            }
            7 => ElementSegment {
                ref_type: self.parse_reference_type()?,
                expression: self.parse_vec(Self::parse_expression)?,
                mode: ElementMode::Declarative,
            },
            foreign => parse_err!("Encountered foreign element segement kind: {}", foreign),
        };

        Ok(segment)
    }

    fn parse_element_section(&mut self) -> Result<ElementSection> {
        Ok(ElementSection {
            elements: self.parse_vec(Self::parse_element_segement)?,
        })
    }

    fn parse_local(&mut self) -> Result<Local> {
        Ok(Local {
            count: self.read_u32()?,
            value_type: self.parse_value_type()?,
        })
    }

    fn parse_code(&mut self) -> Result<Function> {
        let size = self.read_u32()?;
        let start = self.cursor;

        let type_index = self
            .function_types
            .pop_front()
            .ok_or_else(|| Error::Parse("Function type list empty".into()))?;

        let locals = self.parse_vec(Self::parse_local)?;

        let total_locals = locals
            .iter()
            .map(|Local { count, .. }| *count as usize)
            .sum::<usize>();

        ensure!(
            total_locals <= u32::MAX as usize,
            Error::Parse("too many locals".into())
        );

        let func = Function {
            type_index,
            locals,
            body: self.parse_expression()?,
        };

        let consumed = self.cursor - start;
        if consumed != size as usize {
            parse_err!(
                "parse_code: expected {} bytes but consumed {} (type_index={}, start=0x{:x})",
                size,
                consumed,
                type_index,
                start
            );
        }

        Ok(func)
    }

    fn parse_code_section(&mut self) -> Result<CodeSection> {
        Ok(CodeSection {
            codes: self.parse_vec(Self::parse_code)?,
        })
    }

    fn parse_data_segment(&mut self) -> Result<DataSegment> {
        let segment = match self.read_u32()? {
            0 => {
                let offset = self.parse_expression()?;

                let len = self.read_u32()? as usize;
                let bytes = self.read_slice(len)?.to_vec();

                DataSegment {
                    bytes,
                    mode: DataMode::Active { memory: 0, offset },
                }
            }
            1 => DataSegment {
                bytes: {
                    let len = self.read_u32()? as usize;
                    self.read_slice(len)?.to_vec()
                },
                mode: DataMode::Passive,
            },
            2 => {
                let memory = self.read_u32()?;
                let offset = self.parse_expression()?;
                let len = self.read_u32()? as usize;
                let bytes = self.read_slice(len)?.to_vec();

                DataSegment {
                    bytes,
                    mode: DataMode::Active { memory, offset },
                }
            }
            foreign => parse_err!("Encountered foreign data kind. Got: {}", foreign),
        };

        Ok(segment)
    }

    fn parse_data_section(&mut self) -> Result<DataSection> {
        Ok(DataSection {
            data_segments: self.parse_vec(Self::parse_data_segment)?,
        })
    }

    fn parse_module_section(&mut self, id: u8) -> Result<ModuleSection> {
        let size = self.read_u32()?;
        let section_start = self.cursor;

        ensure!(
            section_start + size as usize <= self.buffer.len(),
            Error::Parse("section size exceeds remaining bytes".into())
        );

        let section = match id {
            0 => ModuleSection::Custom(self.parse_custom_section(size)?),
            1 => ModuleSection::Type(self.parse_type_section()?),
            2 => ModuleSection::Import(self.parse_import_section()?),
            3 => ModuleSection::Function(self.parse_function_section()?),
            4 => ModuleSection::Table(self.parse_table_section()?),
            5 => ModuleSection::Memory(self.parse_memory_section()?),
            6 => ModuleSection::Global(self.parse_global_section()?),
            7 => ModuleSection::Export(self.parse_export_section()?),
            8 => ModuleSection::Start(self.read_u32()?),
            9 => ModuleSection::Element(self.parse_element_section()?),
            10 => ModuleSection::Code(self.parse_code_section()?),
            11 => ModuleSection::Data(self.parse_data_section()?),
            12 => ModuleSection::DataCount(self.read_u32()?),
            13 => ModuleSection::Tag(self.parse_tag_section()?),
            foreign_id => parse_err!("Encountered foreign section id: {}", foreign_id),
        };

        if id != 0 {
            let seen = self.cursor - section_start;
            ensure!(
                seen == size as usize,
                Error::Parse(format!(
                    "section {} size mismatch, expected {}, got {}",
                    id, size, seen
                ))
            );
        }

        Ok(section)
    }
}

// maps section ids to their logical ordering position in the binary format
const fn section_logical_order(id: u8) -> u8 {
    match id {
        // type
        1 => 1,
        // import
        2 => 2,
        // function
        3 => 3,
        // table
        4 => 4,
        // memory
        5 => 5,
        // tag
        13 => 6,
        // global
        6 => 7,
        // export
        7 => 8,
        // start
        8 => 9,
        // element
        9 => 10,
        // data count
        12 => 11,
        // code
        10 => 12,
        // data
        11 => 13,
        _ => 0,
    }
}
