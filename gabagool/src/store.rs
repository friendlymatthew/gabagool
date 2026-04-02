use std::collections::HashMap;
use std::fmt::Debug;
use std::ops::Neg;
use std::rc::Rc;
use std::sync::Arc;

use crate::compiler::ModuleCode;
use crate::error::{Error, Result};
#[cfg(feature = "jit")]
use crate::jit::assembler::JitFunction;
use crate::{
    compiler, ensure, instantiation_err, trap, AddrType, Alias, CanonicalDef, Component,
    ComponentSection, ComponentSort, ComponentValue, CoreInstance, CoreSort, DataMode, ElementMode,
    ImportDescription, InstantiatedComponent, Instruction, Module, Mutability, Trap,
};

use crate::binary_grammar::{
    CompositeType, DataSegment, ElementSegment, ExportDescription, Function, FunctionType, Global,
    GlobalType, Limit, MemoryType, ParsedModule, RefType, SubType, TableType, ValueType,
};
use crate::error::Exception;
use crate::execution_grammar::{
    AddressMap, DataInstance, ElementInstance, ExportInstance, ExternalValue, FunctionInstance,
    GlobalInstance, MemoryInstance, Ref, TableInstance, TagInstance,
};
use crate::ir::{CatchKind, CompiledFunction, Op};
use crate::snapshot::{decode_bulk, encode_bulk, Snapshot, SNAPSHOT_MAGIC, SNAPSHOT_VERSION};
use crate::value_stack::ValueStack;
use crate::RawValue;

pub const PAGE_SIZE: usize = 65536;
pub const MAX_CALL_DEPTH: usize = 1024;

#[derive(Debug, Clone)]
pub enum ExecutionState<V = RawValue> {
    Completed(Vec<V>),
    FuelExhausted,
    Suspended {
        module_name: String,
        func_name: String,
        args: Vec<V>,
    },
}

impl<V> ExecutionState<V> {
    pub fn into_completed(self) -> Result<Vec<V>> {
        match self {
            Self::Completed(v) => Ok(v),
            Self::FuelExhausted => instantiation_err!("execution paused: fuel exhausted"),
            Self::Suspended { func_name, .. } => {
                instantiation_err!("execution suspended on host function: {}", func_name)
            }
        }
    }
}

macro_rules! pop_val {
    ($self:expr, I32) => {
        $self.stack.pop().as_i32()
    };
    ($self:expr, I64) => {
        $self.stack.pop().as_i64()
    };
    ($self:expr, F32) => {
        $self.stack.pop().as_f32()
    };
    ($self:expr, F64) => {
        $self.stack.pop().as_f64()
    };
    ($self:expr, Ref) => {
        $self.stack.pop().as_ref()
    };
}

macro_rules! cmp_branch_zero {
    ($self:expr, $depth:expr, $ty:ident, $target:expr, $keep:expr, $drop:expr, $op:tt) => {{
        let a = pop_val!($self, $ty);
        if a $op 0 {
            $self.stack.keep_top($keep as usize, $drop as usize);
            $self.call_stack[$depth].pc = $target as usize;
        }
    }};
}

macro_rules! cmp_branch {
    ($self:expr, $depth:expr, $ty:ident, $target:expr, $keep:expr, $drop:expr, $op:tt) => {{
        let a = pop_val!($self, $ty);
        let b = pop_val!($self, $ty);
        if b $op a {
            $self.stack.keep_top($keep as usize, $drop as usize);
            $self.call_stack[$depth].pc = $target as usize;
        }
    }};
    ($self:expr, $depth:expr, $ty:ident, $target:expr, $keep:expr, $drop:expr, $cast:ty, $op:tt) => {{
        let a = pop_val!($self, $ty);
        let b = pop_val!($self, $ty);
        if (b as $cast) $op (a as $cast) {
            $self.stack.keep_top($keep as usize, $drop as usize);
            $self.call_stack[$depth].pc = $target as usize;
        }
    }};
}

macro_rules! local_get_load {
    ($self:expr, $depth:expr, $mi:expr, $local_idx:expr, $offset:expr, $memory:expr, $width:literal, |$bytes:ident| $convert:expr) => {{
        let locals = &$self.call_stack[$depth].locals;
        let mem_addr = $self.instances[$mi].mem_addrs[$memory as usize];
        let mem = &$self.memories[mem_addr];
        let base = match mem.memory_type.addr_type {
            AddrType::I32 => locals[$local_idx as usize].as_i32() as u64,
            AddrType::I64 => locals[$local_idx as usize].as_i64() as u64,
        };
        let ea = base
            .checked_add($offset as u64)
            .and_then(|v| usize::try_from(v).ok());
        let Some(ea) = ea.filter(|&ea| ea.saturating_add($width) <= mem.data.len()) else {
            trap!(Trap::OutOfBoundsMemoryAccess);
        };
        let $bytes: [u8; $width] = mem.data[ea..ea + $width].try_into().unwrap();
        $self.stack.push($convert);
    }};
}

macro_rules! local_get_store {
    ($self:expr, $depth:expr, $mi:expr, $local_idx:expr, $offset:expr, $memory:expr, $width:literal, $to_bytes:expr) => {{
        let locals = &$self.call_stack[$depth].locals;
        let val = locals[$local_idx as usize];
        let mem_addr = $self.instances[$mi].mem_addrs[$memory as usize];
        let addr_type = $self.memories[mem_addr].memory_type.addr_type;
        let base = $self.stack.pop_address(addr_type) as u64;
        let ea = base
            .checked_add($offset as u64)
            .and_then(|v| usize::try_from(v).ok());
        let mem = &mut $self.memories[mem_addr];
        let Some(ea) = ea.filter(|&ea| ea.saturating_add($width) <= mem.data.len()) else {
            trap!(Trap::OutOfBoundsMemoryAccess);
        };
        let bytes: [u8; $width] = $to_bytes(val);
        mem.data[ea..ea + $width].copy_from_slice(&bytes);
    }};
}

macro_rules! binop {
    ($self:expr, $variant:ident, |$b:ident, $a:ident| $expr:expr) => {{
        let $a = pop_val!($self, $variant);
        let $b = pop_val!($self, $variant);
        $self.stack.push($expr);
    }};
}

macro_rules! cmpop {
    ($self:expr, $variant:ident, |$b:ident, $a:ident| $expr:expr) => {{
        let $a = pop_val!($self, $variant);
        let $b = pop_val!($self, $variant);
        $self.stack.push($expr as i32);
    }};
}

macro_rules! mem_load_c {
    ($self:expr, $mi:expr, $offset:expr, $memory:expr, $width:literal, |$bytes:ident| $convert:expr) => {{
        let mem_addr = $self.instances[$mi].mem_addrs[$memory as usize];
        let mem = &$self.memories[mem_addr];
        let base = $self.stack.pop_address(mem.memory_type.addr_type) as u64;

        let ea = base
            .checked_add($offset as u64)
            .and_then(|v| usize::try_from(v).ok());
        let Some(ea) = ea.filter(|&ea| ea.saturating_add($width) <= mem.data.len()) else {
            trap!(Trap::OutOfBoundsMemoryAccess);
        };
        let $bytes: [u8; $width] = mem.data[ea..ea + $width].try_into().unwrap();

        $self.stack.push($convert);
    }};
}

macro_rules! mem_store_c {
    ($self:expr, $mi:expr, $offset:expr, $memory:expr, $width:literal, |$val:ident| $to_bytes:expr) => {{
        let $val = $self.stack.pop();
        let mem_addr = $self.instances[$mi].mem_addrs[$memory as usize];
        let addr_type = $self.memories[mem_addr].memory_type.addr_type;
        let base = $self.stack.pop_address(addr_type) as u64;
        let ea = base
            .checked_add($offset as u64)
            .and_then(|v| usize::try_from(v).ok());
        let mem = &mut $self.memories[mem_addr];
        let Some(ea) = ea.filter(|&ea| ea.saturating_add($width) <= mem.data.len()) else {
            trap!(Trap::OutOfBoundsMemoryAccess);
        };
        let bytes: [u8; $width] = $to_bytes;
        mem.data[ea..ea + $width].copy_from_slice(&bytes);
    }};
}

enum RunOutcome {
    Completed,
    FuelExhausted,
    Suspended,
}

/// A handle to an instantiated WASM module in the store
///
/// Only created by [`Store::instantiate`], so the handle is always valid
#[derive(Debug, Copy, Clone)]
pub struct Instance(pub(crate) usize);

/// A handle to an instantiated WASM module in the store
///
/// Only created by [`Store::instantiate`], so the handle is always valid
#[derive(Debug, Copy, Clone)]
pub struct ComponentInstance(pub(crate) usize);

pub struct CallFrame {
    pub module_idx: u16,
    pub compiled_func_idx: u32,
    pub pc: usize,
    pub locals: Vec<RawValue>,
    pub stack_base: usize,
    pub arity: usize,
}

struct CatchFrame {
    call_depth: usize,
    stack_restore: usize,
    module_idx: u16,
    handler_idx: u32,
}

/// Runtime state of an instantiated [`crate::Module`]
pub struct InstantiatedModule {
    pub code: Arc<ModuleCode>,
    pub function_addrs: Vec<usize>,
    pub table_addrs: Vec<usize>,
    pub mem_addrs: Vec<usize>,
    pub global_addrs: Vec<usize>,
    pub tag_addrs: Vec<usize>,
    pub elem_addrs: Vec<usize>,
    pub data_addrs: Vec<usize>,
    pub exports: Vec<ExportInstance>,
    #[cfg(feature = "jit")]
    pub jit_functions: Vec<Option<JitFunction>>,
}

/// The runtime state for all instantiated WASM modules
///
/// It also includes shared linear memories, tables, globals, and the execution
/// stacks
pub struct Store {
    // wasm address spaces indexed by module instances
    pub functions: Vec<FunctionInstance>,
    pub tables: Vec<TableInstance>,
    pub memories: Vec<MemoryInstance>,
    pub globals: Vec<GlobalInstance>,
    pub tags: Vec<TagInstance>,
    pub element_segments: Vec<ElementInstance>,
    pub data_segments: Vec<DataInstance>,

    pub(crate) instances: Vec<InstantiatedModule>,
    /// maps func addr → (instance_idx, compiled_func_idx)
    func_addr_to_module: Vec<Option<(u16, u32)>>,

    // execution state
    stack: ValueStack,
    call_stack: Vec<CallFrame>,
    catch_stack: Vec<CatchFrame>,
    exceptions: Vec<Exception>,
    fuel: Option<u64>,
    pending_arity: Option<usize>,
    pending_suspension: Option<(String, String, Vec<RawValue>)>,

    component_instances: Vec<InstantiatedComponent>,
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("functions", &self.functions.len())
            .field("tables", &self.tables.len())
            .field("memories", &self.memories.len())
            .field("globals", &self.globals.len())
            .field("instances", &self.instances.len())
            .finish()
    }
}

impl Store {
    pub fn value_stack(&self) -> &[RawValue] {
        self.stack.snapshot_data().0
    }

    pub fn value_stack_from(&self, stack_base: usize) -> &[RawValue] {
        self.stack.slice_from(stack_base)
    }

    pub fn new() -> Self {
        Self {
            functions: vec![],
            tables: vec![],
            memories: vec![],
            globals: vec![],
            tags: vec![],
            element_segments: vec![],
            data_segments: vec![],
            stack: ValueStack::with_capacity(1024),
            call_stack: Vec::new(),
            catch_stack: Vec::new(),
            exceptions: Vec::new(),
            fuel: None,
            pending_arity: None,
            pending_suspension: None,
            instances: vec![],
            func_addr_to_module: vec![],
            component_instances: vec![],
        }
    }

    pub fn instance(&self, index: usize) -> Instance {
        assert!(index < self.instances.len(), "instance index out of bounds");
        Instance(index)
    }

    pub fn exports(&self, instance: Instance) -> &[ExportInstance] {
        &self.instances[instance.0].exports
    }

    pub fn get_func(&self, instance: Instance, name: &str) -> Result<usize> {
        for export in self.exports(instance) {
            if export.name == name {
                if let ExternalValue::Function { addr } = export.value {
                    return Ok(addr);
                }
                instantiation_err!("export '{}' is not a function", name);
            }
        }
        instantiation_err!("export '{}' not found", name)
    }

    pub fn get_param_types(&self, instance: Instance, name: &str) -> Result<Vec<ValueType>> {
        let addr = self.get_func(instance, name)?;
        let fi = self
            .functions
            .get(addr)
            .ok_or_else(|| Error::Instantiation(format!("function addr {} oob", addr)))?;
        match fi {
            FunctionInstance::Local { function_type, .. }
            | FunctionInstance::Host { function_type, .. } => Ok(function_type.0 .0.clone()),
        }
    }

    pub const fn is_paused(&self) -> bool {
        self.pending_arity.is_some()
    }

    pub const fn set_fuel(&mut self, fuel: u64) {
        self.fuel = Some(fuel);
    }

    pub const fn fuel(&self) -> Option<u64> {
        self.fuel
    }

    pub fn call_stack(&self) -> &[CallFrame] {
        &self.call_stack
    }

    pub fn top_frame(&self) -> Option<&CallFrame> {
        self.call_stack.last()
    }

    fn extract_function_type(types: &[SubType], type_index: u32) -> Result<FunctionType> {
        let sub_type = types.get(type_index as usize).ok_or_else(|| {
            Error::Instantiation(format!(
                "Type index {} too large to index into types. Len: {}",
                type_index,
                types.len()
            ))
        })?;

        match &sub_type.composite_type {
            CompositeType::Func(ft) => Ok(ft.clone()),
            _ => instantiation_err!("Type index {} is not a function type", type_index),
        }
    }

    fn allocate_function(
        &mut self,
        f: Function,
        address_map: &Rc<AddressMap>,
        types: &[SubType],
    ) -> Result<usize> {
        let f_address = self.functions.len();

        let function_type = Self::extract_function_type(types, f.type_index)?;

        self.functions.push(FunctionInstance::Local {
            function_type,
            address_map: Rc::clone(address_map),
            code: f,
        });

        Ok(f_address)
    }

    fn allocate_table(&mut self, table_type: TableType, initial_ref: Ref) -> usize {
        let n = table_type.limit.min;

        let table_address = self.tables.len();

        self.tables.push(TableInstance {
            table_type,
            elem: vec![initial_ref; n as usize],
        });

        table_address
    }

    fn allocate_memory(&mut self, memory_type: MemoryType) -> usize {
        let memory_address = self.memories.len();
        let n = memory_type.limit.min as usize * PAGE_SIZE;

        self.memories.push(MemoryInstance {
            memory_type,
            data: vec![0u8; n],
        });

        memory_address
    }

    fn allocate_global(&mut self, global: Global, initializer_value: RawValue) -> usize {
        let global_address = self.globals.len();

        self.globals.push(GlobalInstance {
            global_type: global.global_type,
            value: initializer_value,
        });

        global_address
    }

    fn allocate_element_segment(
        &mut self,
        element_segment: ElementSegment,
        element_segment_ref: Vec<Ref>,
    ) -> usize {
        let element_segment_address = self.element_segments.len();

        self.element_segments.push(ElementInstance {
            ref_type: element_segment.ref_type,
            elem: element_segment_ref,
        });

        element_segment_address
    }

    fn allocate_data_instance(&mut self, data_segment: DataSegment) -> usize {
        let data_address = self.data_segments.len();

        self.data_segments.push(DataInstance {
            data: data_segment.bytes,
        });

        data_address
    }

    fn allocate_tag(&mut self, tag_type: FunctionType) -> usize {
        let addr = self.tags.len();
        self.tags.push(TagInstance { tag_type });

        addr
    }

    pub fn allocate_module(
        &mut self,
        module: ParsedModule,
        extern_addrs: Vec<ExternalValue>,
        initial_global_values: Vec<RawValue>,
        initial_table_refs: Vec<Ref>,
        element_segment_refs: Vec<Vec<Ref>>,
    ) -> Result<Rc<AddressMap>> {
        // step 1
        let types = module.types;
        let mut address_map = AddressMap::default();

        // step 2-6
        for addr in extern_addrs {
            match addr {
                ExternalValue::Function { addr } => address_map.function_addrs.push(addr),
                ExternalValue::Table { addr } => address_map.table_addrs.push(addr),
                ExternalValue::Memory { addr } => address_map.mem_addrs.push(addr),
                ExternalValue::Global { addr } => address_map.global_addrs.push(addr),
                ExternalValue::Tag { addr } => address_map.tag_addrs.push(addr),
            }
        }

        // step 7
        let _function_addresses = (0..module.functions.len()).map(|i| self.functions.len() + i);

        // step 25-26
        for tag in &module.tags {
            let tag_type = Self::extract_function_type(&types, tag.type_index)?;
            let addr = self.allocate_tag(tag_type);
            address_map.tag_addrs.push(addr);
        }

        // step 27-28
        address_map.global_addrs.extend(
            module
                .globals
                .into_iter()
                .zip(initial_global_values)
                .map(|(global, init_val)| self.allocate_global(global, init_val)),
        );

        // step 29-30
        address_map
            .mem_addrs
            .extend(module.mems.into_iter().map(|m| self.allocate_memory(m)));

        // step 31-32
        address_map.table_addrs.extend(
            module
                .tables
                .into_iter()
                .zip(initial_table_refs)
                .map(|(td, ref_t)| self.allocate_table(td.table_type, ref_t)),
        );

        // step 35-36
        address_map.data_addrs.extend(
            module
                .data_segments
                .into_iter()
                .map(|ds| self.allocate_data_instance(ds)),
        );

        // step 37-38
        for (elem, refs) in module
            .element_segments
            .into_iter()
            .zip(element_segment_refs)
        {
            let addr = self.allocate_element_segment(elem, refs);
            address_map.elem_addrs.push(addr);
        }

        // step 40-42
        let first_func_addr = self.functions.len();
        let num_funcs = module.functions.len();
        for i in 0..num_funcs {
            address_map.function_addrs.push(first_func_addr + i);
        }

        // step 33-34
        for export in &module.exports {
            let extern_value = match export.description {
                ExportDescription::Func(x) => ExternalValue::Function {
                    addr: *address_map
                        .function_addrs
                        .get(x as usize)
                        .ok_or_else(|| Error::Instantiation("oob".into()))?,
                },
                ExportDescription::Table(x) => ExternalValue::Table {
                    addr: *address_map
                        .table_addrs
                        .get(x as usize)
                        .ok_or_else(|| Error::Instantiation("oob".into()))?,
                },
                ExportDescription::Mem(x) => ExternalValue::Memory {
                    addr: *address_map
                        .mem_addrs
                        .get(x as usize)
                        .ok_or_else(|| Error::Instantiation("oob".into()))?,
                },
                ExportDescription::Global(x) => ExternalValue::Global {
                    addr: *address_map
                        .global_addrs
                        .get(x as usize)
                        .ok_or_else(|| Error::Instantiation("oob".into()))?,
                },
                ExportDescription::Tag(x) => ExternalValue::Tag {
                    addr: *address_map
                        .tag_addrs
                        .get(x as usize)
                        .ok_or_else(|| Error::Instantiation("oob".into()))?,
                },
            };

            address_map.exports.push(ExportInstance {
                name: export.name.clone(),
                value: extern_value,
            });
        }

        let module_instance = Rc::new(address_map);
        for func in module.functions {
            self.allocate_function(func, &module_instance, &types)?;
        }

        Ok(module_instance)
    }

    fn validate_imports(&self, module: &Module, imports: &[ExternalValue]) -> Result<()> {
        for (extern_val, import_decl) in imports.iter().zip(module.import_declarations.iter()) {
            let err = |msg: &str| {
                Error::Instantiation(format!(
                    "incompatible import type for {}.{}: {}",
                    import_decl.module, import_decl.name, msg
                ))
            };

            match (extern_val, &import_decl.description) {
                (ExternalValue::Function { addr }, ImportDescription::Func(type_idx)) => {
                    let expected = match &module.types()[*type_idx as usize].composite_type {
                        CompositeType::Func(ft) => ft,
                        _ => return Err(err("type index is not a function type")),
                    };

                    let actual = match &self.functions[*addr] {
                        FunctionInstance::Local { function_type, .. }
                        | FunctionInstance::Host { function_type, .. } => function_type,
                    };

                    ensure!(
                        *expected == *actual,
                        err(&format!("expected {:?}, got {:?}", expected, actual))
                    );
                }
                (ExternalValue::Table { addr }, ImportDescription::Table(expected_tt)) => {
                    let actual_tt = &self.tables[*addr].table_type;

                    ensure!(
                        actual_tt.element_reference_type == expected_tt.element_reference_type
                            && actual_tt.addr_type == expected_tt.addr_type
                            && limits_match(&actual_tt.limit, &expected_tt.limit),
                        err("table type mismatch")
                    );
                }
                (ExternalValue::Memory { addr }, ImportDescription::Mem(expected_mt)) => {
                    let actual_mt = &self.memories[*addr].memory_type;

                    ensure!(
                        actual_mt.addr_type == expected_mt.addr_type
                            && limits_match(&actual_mt.limit, &expected_mt.limit),
                        err("memory type mismatch")
                    );
                }
                (ExternalValue::Global { addr }, ImportDescription::Global(expected_gt)) => {
                    let actual_gt = &self.globals[*addr].global_type;

                    ensure!(
                        actual_gt.value_type == expected_gt.value_type
                            && actual_gt.mutability == expected_gt.mutability,
                        err("global type mismatch")
                    );
                }
                (ExternalValue::Tag { addr }, ImportDescription::Tag(type_idx)) => {
                    let expected = match &module.types()[*type_idx as usize].composite_type {
                        CompositeType::Func(ft) => ft,
                        _ => return Err(err("type index is not a function type")),
                    };

                    if *addr < self.tags.len() {
                        let actual = &self.tags[*addr].tag_type;
                        ensure!(*expected == *actual, err("tag type mismatch"));
                    }
                }
                _ => return Err(Error::Instantiation("import kind mismatch".into())),
            }
        }
        Ok(())
    }

    pub fn instantiate(
        &mut self,
        module: &Module,
        external_addresses: Vec<ExternalValue>,
    ) -> Result<Instance> {
        // step 4
        ensure!(
            module.import_declarations.len() == external_addresses.len(),
            Error::Instantiation(format!(
                "Expected {} imports, got {}",
                module.import_declarations.len(),
                external_addresses.len()
            ))
        );

        // step 5
        self.validate_imports(module, &external_addresses)?;

        // step 6
        let data_instructions = module
            .data_segments
            .iter()
            .enumerate()
            .flat_map(|(i, ds)| run_data(i as u32, ds))
            .collect::<Vec<_>>();

        // step 7
        let element_instructions = module
            .element_segments
            .iter()
            .enumerate()
            .flat_map(|(i, es)| run_elem(i as u32, es))
            .collect::<Vec<_>>();

        // step 8
        let mut address_map = AddressMap {
            global_addrs: external_addresses
                .iter()
                .filter_map(|addr| match addr {
                    ExternalValue::Global { addr } => Some(*addr),
                    _ => None,
                })
                .collect(),
            ..Default::default()
        };

        address_map.function_addrs = external_addresses
            .iter()
            .filter_map(|addr| match addr {
                ExternalValue::Function { addr } => Some(*addr),
                _ => None,
            })
            .collect();

        let func_base = self.functions.len();
        address_map
            .function_addrs
            .extend((0..module.functions.len()).map(|i| func_base + i));

        // step 19: evaluate global init expressions sequentially so that each
        // newly created global is visible to subsequent global.get in const exprs
        let num_imported_globals = self.globals.len();
        let mut initial_global_values = Vec::new();
        for g in &module.globals {
            let value = eval_const_expr_with_module(&g.initial_expression, self, &address_map)?;
            let addr = self.globals.len();
            self.globals.push(GlobalInstance {
                global_type: g.global_type.clone(),
                value,
            });
            address_map.global_addrs.push(addr);
            initial_global_values.push(value);
        }
        // step 20: evaluate table init expressions
        let initial_table_refs = module
            .tables
            .iter()
            .map(|td| {
                let val = eval_const_expr_with_module(&td.init, self, &address_map)?;
                Ok(val.as_ref())
            })
            .collect::<Result<Vec<_>>>()?;

        // step 21 - evaluate element segment exprs
        let element_segment_refs = module
            .element_segments
            .iter()
            .map(|es| {
                es.expression
                    .iter()
                    .map(|expr| {
                        let val = eval_const_expr_with_module(expr, self, &address_map)?;
                        Ok(val.as_ref())
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;

        // remove temp globals — allocate_module will add them properly
        self.globals.truncate(num_imported_globals);

        let start_func_idx = module.start;
        let num_local_funcs = module.functions.len();

        // step 24: allocate_module needs ownership, so clone declaration data
        let parsed_clone = crate::binary_grammar::ParsedModule {
            types: module.code.types.clone(),
            functions: module.functions.clone(),
            tables: module.tables.clone(),
            mems: module.mems.clone(),
            element_segments: module.element_segments.clone(),
            globals: module.globals.clone(),
            data_segments: module.data_segments.clone(),
            start: module.start,
            import_declarations: module.import_declarations.clone(),
            exports: module.exports.clone(),
            tags: module.tags.clone(),
            customs: vec![],
        };
        let module_instance = self.allocate_module(
            parsed_clone,
            external_addresses,
            initial_global_values,
            initial_table_refs,
            element_segment_refs,
        )?;

        // build the InstanceEntity with shared code + address mappings
        let instance_idx = self.instances.len() as u16;
        let mut entity = InstantiatedModule {
            code: Arc::clone(&module.code),
            function_addrs: module_instance.function_addrs.clone(),
            table_addrs: module_instance.table_addrs.clone(),
            mem_addrs: module_instance.mem_addrs.clone(),
            global_addrs: module_instance.global_addrs.clone(),
            tag_addrs: module_instance.tag_addrs.clone(),
            elem_addrs: module_instance.elem_addrs.clone(),
            data_addrs: module_instance.data_addrs.clone(),
            exports: module_instance.exports.clone(),

            #[cfg(feature = "jit")]
            jit_functions: Vec::new(),
        };

        // build a mapping from func_addr to (instance_idx, compiled func index)
        if self.func_addr_to_module.len() < self.functions.len() {
            self.func_addr_to_module.resize(self.functions.len(), None);
        }
        let first_compiled_idx = entity.code.compiled_funcs.len() - num_local_funcs;
        for (i, &addr) in module_instance
            .function_addrs
            .iter()
            .rev()
            .take(num_local_funcs)
            .rev()
            .enumerate()
        {
            self.func_addr_to_module[addr] = Some((instance_idx, (first_compiled_idx + i) as u32));
        }

        // compile any imported local functions not yet in the compiled set
        let types_for_compile = entity.code.types.clone();
        for &addr in &module_instance.function_addrs {
            if self
                .func_addr_to_module
                .get(addr)
                .is_some_and(|v| v.is_some())
            {
                continue;
            }
            if let FunctionInstance::Local { code, .. } = &self.functions[addr] {
                let code_mut = Arc::make_mut(&mut entity.code);
                let cf = compiler::compile_function_into_code(&types_for_compile, code, code_mut);
                let idx = code_mut.compiled_funcs.len();
                code_mut.compiled_funcs.push(cf);
                if addr < self.func_addr_to_module.len() {
                    self.func_addr_to_module[addr] = Some((instance_idx, idx as u32));
                }
            }
        }

        #[cfg(feature = "jit")]
        {
            use crate::jit::assembler::assemble;

            entity.jit_functions = entity
                .code
                .compiled_funcs
                .iter()
                .map(|cf| assemble(&cf.ops))
                .collect();
        }

        self.instances.push(entity);
        self.ensure_stack_capacity();
        let instance = Instance(instance_idx as usize);

        // step 27 - execute element segment initialization
        // step 28 - execute data segment initialization
        let init_instructions = [element_instructions, data_instructions].concat();
        if !init_instructions.is_empty() {
            self.run_init_instructions(&init_instructions, instance_idx)?;
        }

        // step 29: invoke start function if present
        if let Some(start_idx) = start_func_idx {
            let func_addr = *module_instance
                .function_addrs
                .get(start_idx as usize)
                .ok_or_else(|| {
                    Error::Instantiation(format!("start function index {} oob", start_idx))
                })?;
            if self.push_function_call(func_addr)? {
                instantiation_err!("start function cannot be a host import");
            }
            self.run()?;
        }

        // step 31
        Ok(instance)
    }

    fn ensure_stack_capacity(&mut self) {
        let max_func_stack = self
            .instances
            .iter()
            .flat_map(|inst| inst.code.compiled_funcs.iter())
            .map(|f| f.max_stack_height as usize)
            .max()
            .unwrap_or(1_024);

        let needed = max_func_stack.saturating_mul(MAX_CALL_DEPTH).max(1024);
        if needed > self.stack.capacity() {
            self.stack = ValueStack::with_capacity(needed);
        }
    }

    pub fn instantiate_component(&mut self, component: &Component) -> Result<ComponentInstance> {
        let mut core_modules = vec![];
        let mut core_instances = vec![];
        let mut core_funcs = vec![];
        let mut component_funcs = vec![];
        let mut component_exports = HashMap::new();

        for section in &component.parsed.sections {
            match section {
                ComponentSection::CoreModule(parsed_module) => {
                    let module = Module::from_parsed((**parsed_module).clone())?;
                    core_modules.push(module);
                }
                ComponentSection::CoreInstance(instances) => {
                    for instance in instances {
                        match instance {
                            CoreInstance::Instantiate {
                                module_idx,
                                args: _args,
                            } => {
                                let module = &core_modules[*module_idx as usize];
                                // todo: resolve args into imports
                                let inst = self.instantiate(module, vec![])?;
                                core_instances.push(inst);
                            }
                            CoreInstance::FromExports(_exports) => {
                                todo!("core instance from exports")
                            }
                        }
                    }
                }
                ComponentSection::Alias(aliases) => {
                    for alias in aliases {
                        match alias {
                            Alias::CoreExport {
                                sort,
                                instance_idx,
                                name,
                            } => {
                                let instance = core_instances[*instance_idx as usize];

                                match sort {
                                    ComponentSort::Core(CoreSort::Func) => {
                                        let func_addr = self.get_func(instance, name)?;
                                        core_funcs.push(func_addr);
                                    }
                                    _ => todo!("alias core export sort {:?}", sort),
                                }
                            }
                            _ => todo!("alias kind {:?}", alias),
                        }
                    }
                }
                ComponentSection::Canonical(defs) => {
                    for def in defs {
                        match def {
                            CanonicalDef::Lift {
                                core_func_idx,
                                opts: _opts,
                                type_idx: _type_idx,
                            } => {
                                // For primitives (s32 → i32), lift is a passthrough
                                let func_addr = core_funcs[*core_func_idx as usize];

                                component_funcs.push(func_addr);
                            }
                            CanonicalDef::Lower {
                                func_idx: _func_idx,
                                opts: _opts,
                            } => {
                                todo!("canon lower")
                            }
                            _ => todo!("canonical def {:?}", def),
                        }
                    }
                }
                ComponentSection::Export(exports) => {
                    for export in exports {
                        match export.sort {
                            ComponentSort::Func => {
                                let func_addr = component_funcs[export.idx as usize];

                                component_exports.insert(export.name.clone(), func_addr);
                            }
                            _ => todo!("export sort {:?}", export.sort),
                        }
                    }
                }
                ComponentSection::CoreType(_) | ComponentSection::ComponentType(_) => {}
                other => todo!("what do {other:?} section look like"),
            }
        }

        self.component_instances.push(InstantiatedComponent {
            exports: component_exports,
            may_leave: true,
        });

        Ok(ComponentInstance(self.component_instances.len() - 1))
    }

    pub fn invoke<I>(&mut self, instance: Instance, name: &str, args: I) -> Result<ExecutionState>
    where
        I: IntoIterator<IntoIter: ExactSizeIterator>,
        I::Item: Into<RawValue>,
    {
        let addr = self.get_func(instance, name)?;
        self.invoke_by_addr(addr, args)
    }

    pub fn invoke_component<I>(
        &mut self,
        instance: ComponentInstance,
        name: &str,
        args: I,
    ) -> Result<ExecutionState<ComponentValue>>
    where
        I: IntoIterator<IntoIter: ExactSizeIterator>,
        I::Item: Into<ComponentValue>,
    {
        let inst = &self.component_instances[instance.0];
        let &addr = inst
            .exports
            .get(name)
            .ok_or_else(|| Error::Instantiation(format!("export '{name}' not found")))?;

        let raw_args = args.into_iter().map(|v| match v.into() {
            ComponentValue::S32(v) => RawValue::from(v),
            _ => todo!("lower"),
        });

        let result = self.invoke_by_addr(addr, raw_args)?;

        Ok(match result {
            ExecutionState::Completed(raw_values) => ExecutionState::Completed(
                raw_values
                    .iter()
                    .map(|r| ComponentValue::S32(r.as_i32()))
                    .collect(),
            ),
            ExecutionState::FuelExhausted => ExecutionState::FuelExhausted,
            ExecutionState::Suspended {
                module_name,
                func_name,
                args,
            } => ExecutionState::Suspended {
                module_name,
                func_name,
                args: args
                    .iter()
                    .map(|r| ComponentValue::S32(r.as_i32()))
                    .collect(),
            },
        })
    }

    pub fn resume(&mut self) -> Result<ExecutionState> {
        let arity = self
            .pending_arity
            .ok_or_else(|| Error::Instantiation("no pending execution to resume".into()))?;

        self.finish_run(arity)
    }

    pub fn resume_with(&mut self, return_values: &[RawValue]) -> Result<ExecutionState> {
        let arity = self
            .pending_arity
            .ok_or_else(|| Error::Instantiation("no pending execution to resume".into()))?;

        for val in return_values {
            self.stack.push(*val);
        }

        self.finish_run(arity)
    }

    fn finish_run(&mut self, num_results: usize) -> Result<ExecutionState> {
        match self.run() {
            Ok(RunOutcome::Completed) => {
                let results = self.stack.pop_n(num_results);
                self.pending_arity = None;
                Ok(ExecutionState::Completed(results.to_vec()))
            }
            Ok(RunOutcome::FuelExhausted) => {
                self.pending_arity = Some(num_results);
                Ok(ExecutionState::FuelExhausted)
            }
            Ok(RunOutcome::Suspended) => {
                self.pending_arity = Some(num_results);
                let (module_name, func_name, args) = self.pending_suspension.take().unwrap();
                Ok(ExecutionState::Suspended {
                    module_name,
                    func_name,
                    args,
                })
            }
            Err(e) => {
                self.stack.clear();
                self.call_stack.clear();
                self.catch_stack.clear();
                self.pending_arity = None;
                Err(e)
            }
        }
    }

    fn invoke_by_addr<I>(
        &mut self,
        function_addr: usize,
        args: I,
    ) -> Result<ExecutionState<RawValue>>
    where
        I: IntoIterator<IntoIter: ExactSizeIterator>,
        I::Item: Into<RawValue>,
    {
        let args = args.into_iter().map(Into::into);

        if self.pending_arity.is_some() {
            instantiation_err!("cannot invoke while execution is paused; call resume() first");
        }

        let fi = self
            .functions
            .get(function_addr)
            .ok_or_else(|| Error::Instantiation(format!("function addr {} oob", function_addr)))?;

        let (num_args, num_results) = match fi {
            FunctionInstance::Local { function_type, .. }
            | FunctionInstance::Host { function_type, .. } => {
                (function_type.0 .0.len(), function_type.1 .0.len())
            }
        };

        let args_len = args.size_hint().0;

        ensure!(
            num_args == args_len,
            Error::Instantiation(format!("expected {} args, got {}", num_args, args_len))
        );

        self.stack.extend_exact(args);
        let suspended = self.push_function_call(function_addr)?;

        if suspended {
            let (module_name, func_name, host_args) = self.pending_suspension.take().unwrap();
            return Ok(ExecutionState::Suspended {
                module_name,
                func_name,
                args: host_args,
            });
        }

        self.finish_run(num_results)
    }

    fn compiled_func_index(&self, func_addr: usize) -> Option<(u16, u32)> {
        self.func_addr_to_module.get(func_addr).copied().flatten()
    }

    fn push_function_call(&mut self, func_addr: usize) -> Result<bool> {
        ensure!(
            self.call_stack.len() < MAX_CALL_DEPTH,
            Error::Trap(Trap::CallStackExhausted)
        );

        let fi = &self.functions[func_addr];
        let (num_args, num_results) = match fi {
            FunctionInstance::Local { function_type, .. }
            | FunctionInstance::Host { function_type, .. } => {
                (function_type.0 .0.len(), function_type.1 .0.len())
            }
        };

        let Some((module_idx, compiled_idx)) = self.compiled_func_index(func_addr) else {
            match &self.functions[func_addr] {
                FunctionInstance::Host {
                    module_name,
                    function_name,
                    ..
                } => {
                    ensure!(
                        self.stack.len() >= num_args,
                        Error::Instantiation("not enough args on stack".into())
                    );

                    let args = self.stack.pop_n(num_args).to_vec();
                    self.pending_suspension =
                        Some((module_name.clone(), function_name.clone(), args));

                    return Ok(true);
                }
                _ => instantiation_err!("expected host function at addr {}", func_addr),
            }
        };

        ensure!(
            self.stack.len() >= num_args,
            Error::Instantiation("not enough args on stack".into())
        );
        let args_start = self.stack.len() - num_args;
        let mut locals: Vec<RawValue> = self.stack.slice_from(args_start).to_vec();
        self.stack.truncate(args_start);

        let cf = &self.instances[module_idx as usize].code.compiled_funcs[compiled_idx as usize];
        for _ in &cf.local_types[num_args..] {
            locals.push(RawValue::default());
        }

        let stack_base = self.stack.len();

        self.call_stack.push(CallFrame {
            module_idx,
            compiled_func_idx: compiled_idx,
            pc: 0,
            locals,
            stack_base,
            arity: num_results,
        });

        Ok(false)
    }

    #[cfg(feature = "jit")]
    fn run_jit(&mut self) -> Result<RunOutcome> {
        use crate::jit::{Exit, StencilContext};

        loop {
            let depth = match self.call_stack.len() {
                0 => return Ok(RunOutcome::Completed),
                n => n - 1,
            };

            let mi = self.call_stack[depth].module_idx as usize;
            let func_idx = self.call_stack[depth].compiled_func_idx;
            let pc = self.call_stack[depth].pc;

            let Some(jit_function) = &self.instances[mi].jit_functions[func_idx as usize] else {
                return self.run();
            };

            let mut ctx = StencilContext {
                stack: self.stack.as_mut_ptr() as *mut u64,
                stack_pointer: self.stack.len() as u64,
                locals: self.call_stack[depth].locals.as_mut_ptr() as *mut u64,
                mem_base: std::ptr::null_mut(),
                mem_len: 0,
                globals: std::ptr::null_mut(),
                imm_table: jit_function.imm_table.as_ptr(),
                fn_table: jit_function.fn_table.as_ptr(),
                pc: pc as u32,
                snapshot_flag: 0,
                exit_reason: 0,
                exit_value: 0,
            };

            unsafe { (jit_function.fn_table[pc])(&mut ctx) };

            self.stack.truncate(ctx.stack_pointer as usize);
            self.call_stack[depth].pc = ctx.pc as usize;

            match ctx.exit_reason.into() {
                Exit::Return => {
                    self.do_return(depth);
                }
                Exit::Snapshot => {
                    return Ok(RunOutcome::FuelExhausted);
                }
            }
        }
    }

    fn run(&mut self) -> Result<RunOutcome> {
        loop {
            let depth = match self.call_stack.len() {
                0 => return Ok(RunOutcome::Completed),
                n => n - 1,
            };
            assert!(depth < self.call_stack.len());

            let mi = self.call_stack[depth].module_idx as usize;
            let func_idx = self.call_stack[depth].compiled_func_idx;
            let pc = self.call_stack[depth].pc;
            assert!(
                (func_idx as usize) < self.instances[mi].code.compiled_funcs.len(),
                "compiler error: compiled function index {func_idx} oob"
            );

            let func_ops = &self.instances[mi].code.compiled_funcs[func_idx as usize].ops;
            assert!(
                pc < func_ops.len(),
                "compiler error: pc {pc} past end of function {func_idx} (len {})",
                func_ops.len()
            );

            let op = func_ops[pc];
            self.call_stack[depth].pc += 1;

            if let Some(ref mut fuel) = self.fuel {
                if *fuel == 0 {
                    self.call_stack[depth].pc -= 1;
                    return Ok(RunOutcome::FuelExhausted);
                }
                *fuel -= 1;
            }

            match op {
                Op::Nop => {}
                Op::Unreachable => trap!(Trap::Unreachable),
                Op::Return => self.do_return(depth),
                Op::Jump { target, keep, drop } => {
                    self.stack.keep_top(keep as usize, drop as usize);
                    self.call_stack[depth].pc = target as usize;
                }
                Op::JumpIf { target, keep, drop } => {
                    let cond = pop_val!(self, I32);
                    if cond != 0 {
                        self.stack.keep_top(keep as usize, drop as usize);
                        self.call_stack[depth].pc = target as usize;
                    }
                }
                Op::JumpIfNot { target, keep, drop } => {
                    let cond = pop_val!(self, I32);
                    if cond == 0 {
                        self.stack.keep_top(keep as usize, drop as usize);
                        self.call_stack[depth].pc = target as usize;
                    }
                }
                Op::JumpTable { index, keep } => {
                    let i = pop_val!(self, I32) as usize;
                    let table = &self.instances[mi].code.jump_tables[index as usize];
                    let entry_target = if i < table.len() - 1 {
                        table[i]
                    } else {
                        *table.last().unwrap()
                    };
                    self.stack
                        .keep_top(keep as usize, entry_target.drop as usize);
                    self.call_stack[depth].pc = entry_target.target as usize;
                }
                Op::BrOnNull { target, keep, drop } => {
                    let val = self.stack.pop();
                    if matches!(val.as_ref(), Ref::Null) {
                        self.stack.keep_top(keep as usize, drop as usize);
                        self.call_stack[depth].pc = target as usize;
                    } else {
                        self.stack.push(val);
                    }
                }
                Op::BrOnNonNull { target, keep, drop } => {
                    let val = self.stack.pop();
                    if !matches!(val.as_ref(), Ref::Null) {
                        self.stack.push(val);
                        self.stack.keep_top(keep as usize, drop as usize);
                        self.call_stack[depth].pc = target as usize;
                    }
                }
                Op::Call { func_idx } => {
                    let func_addr = self.instances[mi].function_addrs[func_idx as usize];
                    if self.push_function_call(func_addr)? {
                        return Ok(RunOutcome::Suspended);
                    }
                }
                Op::CallIndirect {
                    type_idx,
                    table_idx,
                } => {
                    let table_addr = self.instances[mi].table_addrs[table_idx as usize];
                    let addr_type = self.tables[table_addr].table_type.addr_type;

                    let i = self.stack.pop_address(addr_type);

                    let elem = self.tables[table_addr]
                        .elem
                        .get(i)
                        .ok_or(Error::Trap(Trap::UndefinedElement))?;

                    let Ref::FunctionAddr(func_addr) = elem else {
                        trap!(Trap::UndefinedElement);
                    };

                    let expected =
                        match &self.instances[mi].code.types[type_idx as usize].composite_type {
                            CompositeType::Func(ft) => ft,
                            _ => instantiation_err!("type index {} not a func type", type_idx),
                        };

                    let actual = match &self.functions[*func_addr] {
                        FunctionInstance::Local { function_type, .. }
                        | FunctionInstance::Host { function_type, .. } => function_type,
                    };

                    ensure!(
                        expected == actual,
                        Error::Trap(Trap::IndirectCallTypeMismatch)
                    );

                    if self.push_function_call(*func_addr)? {
                        return Ok(RunOutcome::Suspended);
                    }
                }
                Op::ReturnCall { func_idx } => {
                    let func_addr = self.instances[mi].function_addrs[func_idx as usize];
                    let num_args = self.func_num_params(func_addr);

                    let old_base = self.call_stack[depth].stack_base;
                    let len = self.stack.len();

                    self.pop_catch_frames(depth);

                    self.stack.copy_within(len - num_args..len, old_base);
                    self.stack.truncate(old_base + num_args);
                    self.call_stack.pop();

                    if self.push_function_call(func_addr)? {
                        return Ok(RunOutcome::Suspended);
                    }
                }
                Op::ReturnCallIndirect {
                    type_idx,
                    table_idx,
                } => {
                    let table_addr = self.instances[mi].table_addrs[table_idx as usize];
                    let addr_type = self.tables[table_addr].table_type.addr_type;

                    let i = self.stack.pop_address(addr_type);
                    let elem = self.tables[table_addr]
                        .elem
                        .get(i)
                        .ok_or(Error::Trap(Trap::UndefinedElement))?;

                    let Ref::FunctionAddr(func_addr) = elem else {
                        trap!(Trap::UndefinedElement);
                    };

                    let expected =
                        match &self.instances[mi].code.types[type_idx as usize].composite_type {
                            CompositeType::Func(ft) => ft,
                            _ => instantiation_err!("type index {} not a func type", type_idx),
                        };

                    let actual = match &self.functions[*func_addr] {
                        FunctionInstance::Local { function_type, .. }
                        | FunctionInstance::Host { function_type, .. } => function_type,
                    };

                    ensure!(
                        expected == actual,
                        Error::Trap(Trap::IndirectCallTypeMismatch)
                    );

                    let func_addr = *func_addr;
                    let num_args = expected.0 .0.len();
                    let old_base = self.call_stack[depth].stack_base;
                    let len = self.stack.len();

                    self.pop_catch_frames(depth);

                    self.stack.copy_within(len - num_args..len, old_base);
                    self.stack.truncate(old_base + num_args);
                    self.call_stack.pop();
                    if self.push_function_call(func_addr)? {
                        return Ok(RunOutcome::Suspended);
                    }
                }
                Op::CallRef { .. } => {
                    let func_addr = match self.stack.pop().as_ref() {
                        Ref::Null => trap!(Trap::NullReference),
                        Ref::FunctionAddr(f) => f,
                        _ => instantiation_err!("expected function or null ref"),
                    };
                    if self.push_function_call(func_addr)? {
                        return Ok(RunOutcome::Suspended);
                    }
                }
                Op::ReturnCallRef { .. } => {
                    let func_addr = match self.stack.pop().as_ref() {
                        Ref::Null => trap!(Trap::NullReference),
                        Ref::FunctionAddr(f) => f,
                        _ => instantiation_err!("expected function or null ref"),
                    };

                    let num_args = self.func_num_params(func_addr);
                    let old_base = self.call_stack[depth].stack_base;
                    let len = self.stack.len();

                    self.pop_catch_frames(depth);

                    self.stack.copy_within(len - num_args..len, old_base);
                    self.stack.truncate(old_base + num_args);
                    self.call_stack.pop();
                    if self.push_function_call(func_addr)? {
                        return Ok(RunOutcome::Suspended);
                    }
                }
                Op::I32Const { value } => self.stack.push(value),
                Op::I64Const { value } => self.stack.push(value),
                Op::F32Const { value } => self.stack.push(value),
                Op::F64Const { value } => self.stack.push(value),
                Op::V128Const { table_idx } => {
                    let v = self.instances[mi].code.v128_constants[table_idx as usize];
                    self.stack.push_v128(v);
                }
                Op::LocalGet { local_idx } => self.do_local_get(local_idx as usize, depth),
                Op::LocalSet { local_idx } => {
                    let val = self.stack.pop();
                    let locals = &mut self.call_stack[depth].locals;
                    assert!(
                        (local_idx as usize) < locals.len(),
                        "compiler error: local index {local_idx} oob (func has {} locals)",
                        locals.len()
                    );
                    locals[local_idx as usize] = val;
                }
                Op::LocalTee { local_idx } => {
                    let val = *self.stack.last();
                    let locals = &mut self.call_stack[depth].locals;
                    assert!(
                        (local_idx as usize) < locals.len(),
                        "compiler error: local index {local_idx} oob (func has {} locals)",
                        locals.len()
                    );
                    locals[local_idx as usize] = val;
                }
                Op::GlobalGet { global_idx } => {
                    let addr = self.instances[mi].global_addrs[global_idx as usize];
                    self.stack.push(self.globals[addr].value);
                }
                Op::GlobalSet { global_idx } => {
                    let addr = self.instances[mi].global_addrs[global_idx as usize];
                    ensure!(
                        matches!(self.globals[addr].global_type.mutability, Mutability::Var),
                        Error::Instantiation("cannot set immutable global".into())
                    );
                    self.globals[addr].value = self.stack.pop();
                }
                Op::Drop => {
                    self.stack.pop();
                }
                Op::Select => {
                    let cond = pop_val!(self, I32);
                    let val2 = self.stack.pop();
                    let val1 = self.stack.pop();
                    self.stack.push(if cond != 0 { val1 } else { val2 });
                }
                Op::RefNull(_) => self.stack.push(RawValue::from_ref(Ref::Null)),
                Op::RefIsNull => {
                    let val = self.stack.pop();
                    let is_null = matches!(val.as_ref(), Ref::Null);
                    self.stack.push(is_null as i32);
                }
                Op::RefFunc { func_idx } => {
                    let addr = self.instances[mi].function_addrs[func_idx as usize];
                    self.stack.push(RawValue::from_ref(Ref::FunctionAddr(addr)));
                }
                Op::RefEq => todo!(),
                Op::RefAsNonNull => {
                    let val = self.stack.pop();
                    ensure!(
                        !matches!(val.as_ref(), Ref::Null),
                        Error::Trap(Trap::NullReference)
                    );
                    self.stack.push(val);
                }
                Op::TryCatchPush { handler_idx } => {
                    self.catch_stack.push(CatchFrame {
                        call_depth: self.call_stack.len(),
                        stack_restore: self.stack.len(),
                        module_idx: mi as u16,
                        handler_idx,
                    });
                }
                Op::TryCatchPop => {
                    self.catch_stack.pop();
                }
                Op::Throw { tag_idx } => {
                    let tag_addr = self.instances[mi].tag_addrs[tag_idx as usize];
                    let n_values = self.tags[tag_addr].tag_type.0 .0.len();
                    let values = self.stack.pop_n(n_values).to_vec();
                    self.handle_exception(tag_addr, values)?;
                }
                Op::ThrowRef => {
                    let exn_ref = self.stack.pop().as_ref();
                    match exn_ref {
                        Ref::Null => trap!(Trap::NullReference),
                        Ref::ExnRef(idx) => {
                            let exn = self.exceptions[idx].clone();
                            self.handle_exception(exn.tag_addr, exn.values)?;
                        }
                        _ => trap!(Trap::NullReference),
                    }
                }
                Op::TableGet { table_idx } => {
                    let ta = self.instances[mi].table_addrs[table_idx as usize];
                    let addr_type = self.tables[ta].table_type.addr_type;

                    let i = self.stack.pop_address(addr_type);

                    let elem = self.tables[ta]
                        .elem
                        .get(i)
                        .ok_or(Error::Trap(Trap::OutOfBoundsTableAccess))?;

                    self.stack.push(RawValue::from_ref(*elem));
                }
                Op::TableSet { table_idx } => {
                    let ta = self.instances[mi].table_addrs[table_idx as usize];
                    let r = self.stack.pop().as_ref();

                    let at = self.tables[ta].table_type.addr_type;
                    let i = self.stack.pop_address(at);

                    let elem = self.tables[ta]
                        .elem
                        .get_mut(i)
                        .ok_or(Error::Trap(Trap::OutOfBoundsTableAccess))?;

                    *elem = r;
                }
                Op::TableInit {
                    elem_idx,
                    table_idx,
                } => {
                    let ta = self.instances[mi].table_addrs[table_idx as usize];
                    let ea = self.instances[mi].elem_addrs[elem_idx as usize];

                    let n = pop_val!(self, I32);
                    let s = pop_val!(self, I32);

                    let (n, s) = (n as usize, s as usize);

                    let at = self.tables[ta].table_type.addr_type;
                    let d = self.stack.pop_address(at);

                    if s.saturating_add(n) > self.element_segments[ea].elem.len() {
                        trap!(Trap::OutOfBoundsTableAccess);
                    }

                    if d.saturating_add(n) > self.tables[ta].elem.len() {
                        trap!(Trap::OutOfBoundsTableAccess);
                    }

                    if n > 0 {
                        let src = self.element_segments[ea].elem[s..s + n].to_vec();
                        self.tables[ta].elem[d..d + n].copy_from_slice(&src);
                    }
                }
                Op::ElemDrop { elem_idx } => {
                    let ea = self.instances[mi].elem_addrs[elem_idx as usize];
                    self.element_segments[ea].elem.clear();
                }
                Op::TableCopy {
                    dst_table_idx,
                    src_table_idx,
                } => {
                    let dst_a = self.instances[mi].table_addrs[dst_table_idx as usize];
                    let src_a = self.instances[mi].table_addrs[src_table_idx as usize];

                    let dst_at = self.tables[dst_a].table_type.addr_type;
                    let src_at = self.tables[src_a].table_type.addr_type;
                    let n_at = match (dst_at, src_at) {
                        (AddrType::I64, AddrType::I64) => AddrType::I64,
                        _ => AddrType::I32,
                    };

                    let n = self.stack.pop_address(n_at);
                    let s = self.stack.pop_address(src_at);
                    let d = self.stack.pop_address(dst_at);

                    if s.saturating_add(n) > self.tables[src_a].elem.len() {
                        trap!(Trap::OutOfBoundsTableAccess);
                    }

                    if d.saturating_add(n) > self.tables[dst_a].elem.len() {
                        trap!(Trap::OutOfBoundsTableAccess);
                    }

                    if n > 0 {
                        if dst_a == src_a {
                            self.tables[dst_a].elem.copy_within(s..s + n, d);
                        } else {
                            let src = self.tables[src_a].elem[s..s + n].to_vec();
                            self.tables[dst_a].elem[d..d + n].copy_from_slice(&src);
                        }
                    }
                }
                Op::TableGrow { table_idx } => {
                    let ta = self.instances[mi].table_addrs[table_idx as usize];
                    let at = self.tables[ta].table_type.addr_type;
                    let n = self.stack.pop_address(at);

                    let r = self.stack.pop().as_ref();

                    let old_size = self.tables[ta].elem.len();
                    let new_size = (old_size as u64).checked_add(n as u64);

                    if new_size.is_none_or(|s| s > self.tables[ta].table_type.limit.max) {
                        match at {
                            AddrType::I32 => self.stack.push(-1i32),
                            AddrType::I64 => self.stack.push(-1i64),
                        }
                        continue;
                    }

                    let new_size = new_size.unwrap();
                    self.tables[ta].elem.resize(new_size as usize, r);
                    self.tables[ta].table_type.limit.min = new_size;
                    self.stack.push_address(old_size, at);
                }
                Op::TableSize { table_idx } => {
                    let ta = self.instances[mi].table_addrs[table_idx as usize];
                    let size = self.tables[ta].elem.len();
                    let at = self.tables[ta].table_type.addr_type;

                    self.stack.push_address(size, at);
                }
                Op::TableFill { table_idx } => {
                    let ta = self.instances[mi].table_addrs[table_idx as usize];
                    let at = self.tables[ta].table_type.addr_type;
                    let n = self.stack.pop_address(at);

                    let r = self.stack.pop().as_ref();

                    let i = self.stack.pop_address(at);
                    if i.saturating_add(n) > self.tables[ta].elem.len() {
                        trap!(Trap::OutOfBoundsTableAccess);
                    }

                    if n > 0 {
                        self.tables[ta].elem[i..i + n].fill(r);
                    }
                }
                Op::I32Load { offset, memory } => {
                    mem_load_c!(self, mi, offset, memory, 4, |b| i32::from_le_bytes(b))
                }
                Op::I64Load { offset, memory } => {
                    mem_load_c!(self, mi, offset, memory, 8, |b| i64::from_le_bytes(b))
                }
                Op::F32Load { offset, memory } => {
                    mem_load_c!(self, mi, offset, memory, 4, |b| f32::from_le_bytes(b))
                }
                Op::F64Load { offset, memory } => {
                    mem_load_c!(self, mi, offset, memory, 8, |b| f64::from_le_bytes(b))
                }
                Op::I32Load8Signed { offset, memory } => {
                    mem_load_c!(self, mi, offset, memory, 1, |b| b[0] as i8 as i32)
                }
                Op::I32Load8Unsigned { offset, memory } => {
                    mem_load_c!(self, mi, offset, memory, 1, |b| b[0] as i32)
                }
                Op::I32Load16Signed { offset, memory } => {
                    mem_load_c!(self, mi, offset, memory, 2, |b| i16::from_le_bytes(b)
                        as i32)
                }
                Op::I32Load16Unsigned { offset, memory } => {
                    mem_load_c!(self, mi, offset, memory, 2, |b| u16::from_le_bytes(b)
                        as i32)
                }
                Op::I64Load8Signed { offset, memory } => {
                    mem_load_c!(self, mi, offset, memory, 1, |b| b[0] as i8 as i64)
                }
                Op::I64Load8Unsigned { offset, memory } => {
                    mem_load_c!(self, mi, offset, memory, 1, |b| b[0] as i64)
                }
                Op::I64Load16Signed { offset, memory } => {
                    mem_load_c!(self, mi, offset, memory, 2, |b| i16::from_le_bytes(b)
                        as i64)
                }
                Op::I64Load16Unsigned { offset, memory } => {
                    mem_load_c!(self, mi, offset, memory, 2, |b| u16::from_le_bytes(b)
                        as i64)
                }
                Op::I64Load32Signed { offset, memory } => {
                    mem_load_c!(self, mi, offset, memory, 4, |b| i32::from_le_bytes(b)
                        as i64)
                }
                Op::I64Load32Unsigned { offset, memory } => {
                    mem_load_c!(self, mi, offset, memory, 4, |b| u32::from_le_bytes(b)
                        as i64)
                }
                Op::I32Store { offset, memory } => {
                    mem_store_c!(self, mi, offset, memory, 4, |v| v.as_i32().to_le_bytes())
                }
                Op::I64Store { offset, memory } => {
                    mem_store_c!(self, mi, offset, memory, 8, |v| v.as_i64().to_le_bytes())
                }
                Op::F32Store { offset, memory } => {
                    mem_store_c!(self, mi, offset, memory, 4, |v| v.as_f32().to_le_bytes())
                }
                Op::F64Store { offset, memory } => {
                    mem_store_c!(self, mi, offset, memory, 8, |v| v.as_f64().to_le_bytes())
                }
                Op::I32Store8 { offset, memory } => {
                    mem_store_c!(self, mi, offset, memory, 1, |v| (v.as_i32() as u8)
                        .to_le_bytes())
                }
                Op::I32Store16 { offset, memory } => {
                    mem_store_c!(self, mi, offset, memory, 2, |v| (v.as_i32() as u16)
                        .to_le_bytes())
                }
                Op::I64Store8 { offset, memory } => {
                    mem_store_c!(self, mi, offset, memory, 1, |v| (v.as_i64() as u8)
                        .to_le_bytes())
                }
                Op::I64Store16 { offset, memory } => {
                    mem_store_c!(self, mi, offset, memory, 2, |v| (v.as_i64() as u16)
                        .to_le_bytes())
                }
                Op::I64Store32 { offset, memory } => {
                    mem_store_c!(self, mi, offset, memory, 4, |v| (v.as_i64() as u32)
                        .to_le_bytes())
                }
                Op::MemorySize { memory_idx } => {
                    let ma = self.instances[mi].mem_addrs[memory_idx as usize];
                    let mem = &self.memories[ma];
                    let size = mem.data.len() / PAGE_SIZE;

                    self.stack.push_address(size, mem.memory_type.addr_type);
                }
                Op::MemoryGrow { memory_idx } => {
                    let ma = self.instances[mi].mem_addrs[memory_idx as usize];
                    let mem = &mut self.memories[ma];
                    let at = mem.memory_type.addr_type;
                    let page_count = self.stack.pop_address(at);

                    let old_size = mem.data.len() / PAGE_SIZE;
                    let new_size = old_size + page_count;

                    const MAX_PAGES: usize = 65536;

                    if new_size > MAX_PAGES || new_size as u64 > mem.memory_type.limit.max {
                        match at {
                            AddrType::I32 => self.stack.push(-1i32),
                            AddrType::I64 => self.stack.push(-1i64),
                        }
                        continue;
                    }
                    mem.data.resize(new_size * PAGE_SIZE, 0);
                    let at = mem.memory_type.addr_type;
                    mem.memory_type.limit.min = new_size as u64;

                    self.stack.push_address(old_size, at);
                }
                Op::MemoryInit {
                    data_idx,
                    memory_idx,
                } => {
                    let ma = self.instances[mi].mem_addrs[memory_idx as usize];
                    let da = self.instances[mi].data_addrs[data_idx as usize];

                    let at = self.memories[ma].memory_type.addr_type;

                    let n = pop_val!(self, I32) as usize;
                    let s = pop_val!(self, I32) as usize;
                    let d = self.stack.pop_address(at);

                    if s.saturating_add(n) > self.data_segments[da].data.len() {
                        trap!(Trap::OutOfBoundsMemoryAccess);
                    }

                    if d.saturating_add(n) > self.memories[ma].data.len() {
                        trap!(Trap::OutOfBoundsMemoryAccess);
                    }

                    if n > 0 {
                        let src = self.data_segments[da].data[s..s + n].to_vec();
                        self.memories[ma].data[d..d + n].copy_from_slice(&src);
                    }
                }
                Op::DataDrop { data_idx } => {
                    let da = self.instances[mi].data_addrs[data_idx as usize];
                    self.data_segments[da].data.clear();
                }
                Op::MemoryCopy {
                    dst_memory_idx,
                    src_memory_idx,
                } => {
                    let m1 = self.instances[mi].mem_addrs[dst_memory_idx as usize];
                    let m2 = self.instances[mi].mem_addrs[src_memory_idx as usize];
                    let at = self.memories[m1].memory_type.addr_type;

                    let n = self.stack.pop_address(at);
                    let i2 = self.stack.pop_address(at);
                    let i1 = self.stack.pop_address(at);

                    if i1.saturating_add(n) > self.memories[m1].data.len() {
                        trap!(Trap::OutOfBoundsMemoryAccess);
                    }

                    if i2.saturating_add(n) > self.memories[m2].data.len() {
                        trap!(Trap::OutOfBoundsMemoryAccess);
                    }

                    if n > 0 {
                        if m1 == m2 {
                            self.memories[m1].data.copy_within(i2..i2 + n, i1);
                        } else {
                            let src = self.memories[m2].data[i2..i2 + n].to_vec();
                            self.memories[m1].data[i1..i1 + n].copy_from_slice(&src);
                        }
                    }
                }
                Op::MemoryFill { memory_idx } => {
                    let ma = self.instances[mi].mem_addrs[memory_idx as usize];
                    let at = self.memories[ma].memory_type.addr_type;
                    let n = self.stack.pop_address(at);
                    let val = pop_val!(self, I32);
                    let i = self.stack.pop_address(at);

                    if i.saturating_add(n) > self.memories[ma].data.len() {
                        trap!(Trap::OutOfBoundsMemoryAccess);
                    }

                    if n > 0 {
                        self.memories[ma].data[i..i + n].fill(val as u8);
                    }
                }
                Op::I32EqZero => {
                    let a = pop_val!(self, I32);
                    self.stack.push((a == 0) as i32);
                }
                Op::I32Eq => cmpop!(self, I32, |b, a| b == a),
                Op::I32Ne => cmpop!(self, I32, |b, a| b != a),
                Op::I32LtSigned => cmpop!(self, I32, |b, a| b < a),
                Op::I32LtUnsigned => cmpop!(self, I32, |b, a| (b as u32) < (a as u32)),
                Op::I32GtSigned => cmpop!(self, I32, |b, a| b > a),
                Op::I32GtUnsigned => cmpop!(self, I32, |b, a| (b as u32) > (a as u32)),
                Op::I32LeSigned => cmpop!(self, I32, |b, a| b <= a),
                Op::I32LeUnsigned => cmpop!(self, I32, |b, a| (b as u32) <= (a as u32)),
                Op::I32GeSigned => cmpop!(self, I32, |b, a| b >= a),
                Op::I32GeUnsigned => cmpop!(self, I32, |b, a| (b as u32) >= (a as u32)),
                Op::I32CountLeadingZeros => {
                    let a = pop_val!(self, I32);
                    self.stack.push(a.leading_zeros() as i32);
                }
                Op::I32CountTrailingZeros => {
                    let a = pop_val!(self, I32);
                    self.stack.push(a.trailing_zeros() as i32);
                }
                Op::I32PopCount => {
                    let a = pop_val!(self, I32);
                    self.stack.push(a.count_ones() as i32);
                }
                Op::I32Add => binop!(self, I32, |b, a| b.wrapping_add(a)),
                Op::I32Sub => binop!(self, I32, |b, a| b.wrapping_sub(a)),
                Op::I32Mul => binop!(self, I32, |b, a| b.wrapping_mul(a)),
                Op::I32DivSigned => {
                    let a = pop_val!(self, I32);
                    let b = pop_val!(self, I32);

                    ensure!(a != 0, Error::Trap(Trap::IntegerDivideByZero));
                    ensure!(
                        !(b == i32::MIN && a == -1),
                        Error::Trap(Trap::IntegerOverflow)
                    );

                    self.stack.push(b.wrapping_div(a));
                }
                Op::I32DivUnsigned => {
                    let a = pop_val!(self, I32);
                    let b = pop_val!(self, I32);

                    ensure!(a != 0, Error::Trap(Trap::IntegerDivideByZero));
                    self.stack.push(((b as u32) / (a as u32)) as i32);
                }
                Op::I32RemainderSigned => {
                    let a = pop_val!(self, I32);
                    let b = pop_val!(self, I32);

                    ensure!(a != 0, Error::Trap(Trap::IntegerDivideByZero));
                    self.stack.push(b.wrapping_rem(a));
                }
                Op::I32RemainderUnsigned => {
                    let a = pop_val!(self, I32);
                    let b = pop_val!(self, I32);

                    ensure!(a != 0, Error::Trap(Trap::IntegerDivideByZero));
                    self.stack.push(((b as u32) % (a as u32)) as i32);
                }
                Op::I32And => binop!(self, I32, |b, a| b & a),
                Op::I32Or => binop!(self, I32, |b, a| b | a),
                Op::I32Xor => binop!(self, I32, |b, a| b ^ a),
                Op::I32Shl => binop!(self, I32, |b, a| b.wrapping_shl(a as u32 % 32)),
                Op::I32ShrSigned => binop!(self, I32, |b, a| b.wrapping_shr(a as u32 % 32)),
                Op::I32ShrUnsigned => {
                    binop!(self, I32, |b, a| ((b as u32).wrapping_shr(a as u32 % 32))
                        as i32)
                }
                Op::I32RotateLeft => binop!(self, I32, |b, a| b.rotate_left(a as u32 % 32)),
                Op::I32RotateRight => binop!(self, I32, |b, a| b.rotate_right(a as u32 % 32)),
                Op::I64EqZero => {
                    let a = pop_val!(self, I64);
                    self.stack.push((a == 0) as i32);
                }
                Op::I64Eq => cmpop!(self, I64, |b, a| b == a),
                Op::I64Ne => cmpop!(self, I64, |b, a| b != a),
                Op::I64LtSigned => cmpop!(self, I64, |b, a| b < a),
                Op::I64LtUnsigned => cmpop!(self, I64, |b, a| (b as u64) < (a as u64)),
                Op::I64GtSigned => cmpop!(self, I64, |b, a| b > a),
                Op::I64GtUnsigned => cmpop!(self, I64, |b, a| (b as u64) > (a as u64)),
                Op::I64LeSigned => cmpop!(self, I64, |b, a| b <= a),
                Op::I64LeUnsigned => cmpop!(self, I64, |b, a| (b as u64) <= (a as u64)),
                Op::I64GeSigned => cmpop!(self, I64, |b, a| b >= a),
                Op::I64GeUnsigned => cmpop!(self, I64, |b, a| (b as u64) >= (a as u64)),
                Op::I64CountLeadingZeros => {
                    let a = pop_val!(self, I64);
                    self.stack.push(a.leading_zeros() as i64);
                }
                Op::I64CountTrailingZeros => {
                    let a = pop_val!(self, I64);
                    self.stack.push(a.trailing_zeros() as i64);
                }
                Op::I64PopCount => {
                    let a = pop_val!(self, I64);
                    self.stack.push(a.count_ones() as i64);
                }
                Op::I64Add => binop!(self, I64, |b, a| b.wrapping_add(a)),
                Op::I64Sub => binop!(self, I64, |b, a| b.wrapping_sub(a)),
                Op::I64Mul => binop!(self, I64, |b, a| b.wrapping_mul(a)),
                Op::I64DivSigned => {
                    let a = pop_val!(self, I64);
                    let b = pop_val!(self, I64);

                    ensure!(a != 0, Error::Trap(Trap::IntegerDivideByZero));
                    ensure!(
                        !(b == i64::MIN && a == -1),
                        Error::Trap(Trap::IntegerOverflow)
                    );

                    self.stack.push(b.wrapping_div(a));
                }
                Op::I64DivUnsigned => {
                    let a = pop_val!(self, I64);
                    let b = pop_val!(self, I64);

                    ensure!(a != 0, Error::Trap(Trap::IntegerDivideByZero));

                    self.stack.push(((b as u64) / (a as u64)) as i64);
                }
                Op::I64RemainderSigned => {
                    let a = pop_val!(self, I64);
                    let b = pop_val!(self, I64);

                    ensure!(a != 0, Error::Trap(Trap::IntegerDivideByZero));

                    self.stack.push(b.wrapping_rem(a));
                }
                Op::I64RemainderUnsigned => {
                    let a = pop_val!(self, I64);
                    let b = pop_val!(self, I64);

                    ensure!(a != 0, Error::Trap(Trap::IntegerDivideByZero));
                    self.stack.push(((b as u64) % (a as u64)) as i64);
                }
                Op::I64And => binop!(self, I64, |b, a| b & a),
                Op::I64Or => binop!(self, I64, |b, a| b | a),
                Op::I64Xor => binop!(self, I64, |b, a| b ^ a),
                Op::I64Shl => binop!(self, I64, |b, a| b.wrapping_shl(a as u32 % 64)),
                Op::I64ShrSigned => binop!(self, I64, |b, a| b.wrapping_shr(a as u32 % 64)),
                Op::I64ShrUnsigned => {
                    binop!(self, I64, |b, a| ((b as u64).wrapping_shr(a as u32 % 64))
                        as i64)
                }
                Op::I64RotateLeft => binop!(self, I64, |b, a| b.rotate_left(a as u32 % 64)),
                Op::I64RotateRight => binop!(self, I64, |b, a| b.rotate_right(a as u32 % 64)),
                Op::F32Eq => cmpop!(self, F32, |b, a| b == a),
                Op::F32Ne => cmpop!(self, F32, |b, a| b != a),
                Op::F32Lt => cmpop!(self, F32, |b, a| b < a),
                Op::F32Gt => cmpop!(self, F32, |b, a| b > a),
                Op::F32Le => cmpop!(self, F32, |b, a| b <= a),
                Op::F32Ge => cmpop!(self, F32, |b, a| b >= a),
                Op::F32Abs => {
                    let a = pop_val!(self, F32);
                    self.stack.push(a.abs());
                }
                Op::F32Neg => {
                    let a = pop_val!(self, F32);
                    self.stack.push(a.neg());
                }
                Op::F32Ceil => {
                    let a = pop_val!(self, F32);
                    self.stack.push(a.ceil());
                }
                Op::F32Floor => {
                    let a = pop_val!(self, F32);
                    self.stack.push(a.floor());
                }
                Op::F32Trunc => {
                    let a = pop_val!(self, F32);
                    self.stack.push(a.trunc());
                }
                Op::F32Nearest => {
                    let a = pop_val!(self, F32);
                    self.stack.push(a.round_ties_even());
                }
                Op::F32Sqrt => {
                    let a = pop_val!(self, F32);
                    self.stack.push(a.sqrt());
                }
                Op::F32Add => binop!(self, F32, |b, a| b + a),
                Op::F32Sub => binop!(self, F32, |b, a| b - a),
                Op::F32Mul => binop!(self, F32, |b, a| b * a),
                Op::F32Div => binop!(self, F32, |b, a| b / a),
                Op::F32Min => {
                    let a = pop_val!(self, F32);
                    let b = pop_val!(self, F32);
                    let r = if a.is_nan() || b.is_nan() {
                        f32::from_bits(0x7FC0_0000)
                    } else if a == b {
                        f32::from_bits(a.to_bits() | b.to_bits())
                    } else {
                        a.min(b)
                    };
                    self.stack.push(r);
                }
                Op::F32Max => {
                    let a = pop_val!(self, F32);
                    let b = pop_val!(self, F32);
                    let r = if a.is_nan() || b.is_nan() {
                        f32::from_bits(0x7FC0_0000)
                    } else if a == b {
                        f32::from_bits(a.to_bits() & b.to_bits())
                    } else {
                        a.max(b)
                    };
                    self.stack.push(r);
                }
                Op::F32CopySign => binop!(self, F32, |b, a| b.copysign(a)),
                Op::F64Eq => cmpop!(self, F64, |b, a| b == a),
                Op::F64Ne => cmpop!(self, F64, |b, a| b != a),
                Op::F64Lt => cmpop!(self, F64, |b, a| b < a),
                Op::F64Gt => cmpop!(self, F64, |b, a| b > a),
                Op::F64Le => cmpop!(self, F64, |b, a| b <= a),
                Op::F64Ge => cmpop!(self, F64, |b, a| b >= a),
                Op::F64Abs => {
                    let a = pop_val!(self, F64);
                    self.stack.push(a.abs());
                }
                Op::F64Neg => {
                    let a = pop_val!(self, F64);
                    self.stack.push(a.neg());
                }
                Op::F64Ceil => {
                    let a = pop_val!(self, F64);
                    self.stack.push(a.ceil());
                }
                Op::F64Floor => {
                    let a = pop_val!(self, F64);
                    self.stack.push(a.floor());
                }
                Op::F64Trunc => {
                    let a = pop_val!(self, F64);
                    self.stack.push(a.trunc());
                }
                Op::F64Nearest => {
                    let a = pop_val!(self, F64);
                    self.stack.push(a.round_ties_even());
                }
                Op::F64Sqrt => {
                    let a = pop_val!(self, F64);
                    self.stack.push(a.sqrt());
                }
                Op::F64Add => binop!(self, F64, |b, a| b + a),
                Op::F64Sub => binop!(self, F64, |b, a| b - a),
                Op::F64Mul => binop!(self, F64, |b, a| b * a),
                Op::F64Div => binop!(self, F64, |b, a| b / a),
                Op::F64Min => {
                    let a = pop_val!(self, F64);
                    let b = pop_val!(self, F64);
                    let r = if a.is_nan() || b.is_nan() {
                        f64::from_bits(0x7FF8_0000_0000_0000)
                    } else if a == b {
                        f64::from_bits(a.to_bits() | b.to_bits())
                    } else {
                        a.min(b)
                    };
                    self.stack.push(r);
                }
                Op::F64Max => {
                    let a = pop_val!(self, F64);
                    let b = pop_val!(self, F64);
                    let r = if a.is_nan() || b.is_nan() {
                        f64::from_bits(0x7FF8_0000_0000_0000)
                    } else if a == b {
                        f64::from_bits(a.to_bits() & b.to_bits())
                    } else {
                        a.max(b)
                    };
                    self.stack.push(r);
                }
                Op::F64CopySign => binop!(self, F64, |b, a| b.copysign(a)),
                Op::I32WrapI64 => {
                    let a = pop_val!(self, I64);
                    self.stack.push(a as i32);
                }
                Op::I32TruncF32Signed => {
                    let a = pop_val!(self, F32);
                    ensure!(!a.is_nan(), Error::Trap(Trap::InvalidConversionToInteger));
                    let t = a.trunc();
                    ensure!(
                        t >= i32::MIN as f32 && t < i32::MAX as f32,
                        Error::Trap(Trap::IntegerOverflow)
                    );
                    self.stack.push(t as i32);
                }
                Op::I32TruncF32Unsigned => {
                    let a = pop_val!(self, F32);
                    ensure!(!a.is_nan(), Error::Trap(Trap::InvalidConversionToInteger));
                    let t = a.trunc();
                    ensure!(
                        t >= 0.0 && t < u32::MAX as f32,
                        Error::Trap(Trap::IntegerOverflow)
                    );
                    self.stack.push(t as u32 as i32);
                }
                Op::I32TruncF64Signed => {
                    let a = pop_val!(self, F64);
                    ensure!(!a.is_nan(), Error::Trap(Trap::InvalidConversionToInteger));
                    let t = a.trunc();
                    ensure!(
                        t >= i32::MIN as f64 && t <= i32::MAX as f64,
                        Error::Trap(Trap::IntegerOverflow)
                    );
                    self.stack.push(t as i32);
                }
                Op::I32TruncF64Unsigned => {
                    let a = pop_val!(self, F64);
                    ensure!(!a.is_nan(), Error::Trap(Trap::InvalidConversionToInteger));
                    let t = a.trunc();
                    ensure!(
                        t >= 0.0 && t <= u32::MAX as f64,
                        Error::Trap(Trap::IntegerOverflow)
                    );
                    self.stack.push(t as u32 as i32);
                }
                Op::I64ExtendI32Signed => {
                    let a = pop_val!(self, I32);
                    self.stack.push(a as i64);
                }
                Op::I64ExtendI32Unsigned => {
                    let a = pop_val!(self, I32);
                    self.stack.push(a as u32 as i64);
                }
                Op::I64TruncF32Signed => {
                    let a = pop_val!(self, F32);
                    ensure!(!a.is_nan(), Error::Trap(Trap::InvalidConversionToInteger));
                    let t = a.trunc();
                    ensure!(
                        t >= i64::MIN as f32 && t < i64::MAX as f32,
                        Error::Trap(Trap::IntegerOverflow)
                    );
                    self.stack.push(t as i64);
                }
                Op::I64TruncF32Unsigned => {
                    let a = pop_val!(self, F32);
                    ensure!(!a.is_nan(), Error::Trap(Trap::InvalidConversionToInteger));
                    let t = a.trunc();
                    ensure!(
                        t >= 0.0 && t < u64::MAX as f32,
                        Error::Trap(Trap::IntegerOverflow)
                    );
                    self.stack.push(t as u64 as i64);
                }
                Op::I64TruncF64Signed => {
                    let a = pop_val!(self, F64);
                    ensure!(!a.is_nan(), Error::Trap(Trap::InvalidConversionToInteger));
                    let t = a.trunc();
                    ensure!(
                        t >= i64::MIN as f64 && t < i64::MAX as f64,
                        Error::Trap(Trap::IntegerOverflow)
                    );
                    self.stack.push(t as i64);
                }
                Op::I64TruncF64Unsigned => {
                    let a = pop_val!(self, F64);
                    ensure!(!a.is_nan(), Error::Trap(Trap::InvalidConversionToInteger));
                    let t = a.trunc();
                    ensure!(
                        t >= 0.0 && t < u64::MAX as f64,
                        Error::Trap(Trap::IntegerOverflow)
                    );
                    self.stack.push(t as u64 as i64);
                }
                Op::F32ConvertI32Signed => {
                    let a = pop_val!(self, I32);
                    self.stack.push(a as f32);
                }
                Op::F32ConvertI32Unsigned => {
                    let a = pop_val!(self, I32);
                    self.stack.push((a as u32) as f32);
                }
                Op::F32ConvertI64Signed => {
                    let a = pop_val!(self, I64);
                    self.stack.push(a as f32);
                }
                Op::F32ConvertI64Unsigned => {
                    let a = pop_val!(self, I64);
                    self.stack.push((a as u64) as f32);
                }
                Op::F32DemoteF64 => {
                    let a = pop_val!(self, F64);
                    self.stack.push(a as f32);
                }
                Op::F64ConvertI32Signed => {
                    let a = pop_val!(self, I32);
                    self.stack.push(a as f64);
                }
                Op::F64ConvertI32Unsigned => {
                    let a = pop_val!(self, I32);
                    self.stack.push((a as u32) as f64);
                }
                Op::F64ConvertI64Signed => {
                    let a = pop_val!(self, I64);
                    self.stack.push(a as f64);
                }
                Op::F64ConvertI64Unsigned => {
                    let a = pop_val!(self, I64);
                    self.stack.push((a as u64) as f64);
                }
                Op::F64PromoteF32 => {
                    let a = pop_val!(self, F32);
                    self.stack.push(a as f64);
                }
                Op::I32ReinterpretF32 => {
                    let a = pop_val!(self, F32);
                    self.stack.push(a.to_bits() as i32);
                }
                Op::I64ReinterpretF64 => {
                    let a = pop_val!(self, F64);
                    self.stack.push(a.to_bits() as i64);
                }
                Op::F32ReinterpretI32 => {
                    let a = pop_val!(self, I32);
                    self.stack.push(f32::from_bits(a as u32));
                }
                Op::F64ReinterpretI64 => {
                    let a = pop_val!(self, I64);
                    self.stack.push(f64::from_bits(a as u64));
                }
                Op::I32Extend8Signed => {
                    let a = pop_val!(self, I32);
                    self.stack.push((a as i8) as i32);
                }
                Op::I32Extend16Signed => {
                    let a = pop_val!(self, I32);
                    self.stack.push((a as i16) as i32);
                }
                Op::I64Extend8Signed => {
                    let a = pop_val!(self, I64);
                    self.stack.push((a as i8) as i64);
                }
                Op::I64Extend16Signed => {
                    let a = pop_val!(self, I64);
                    self.stack.push((a as i16) as i64);
                }
                Op::I64Extend32Signed => {
                    let a = pop_val!(self, I64);
                    self.stack.push((a as i32) as i64);
                }
                Op::I32TruncSaturatedF32Signed => {
                    let a = pop_val!(self, F32);
                    self.stack.push(if a.is_nan() { 0 } else { a as i32 });
                }
                Op::I32TruncSaturatedF32Unsigned => {
                    let a = pop_val!(self, F32);
                    let r = if a.is_nan() || a < 0.0 {
                        0u32
                    } else {
                        a as u32
                    };
                    self.stack.push(r as i32);
                }
                Op::I32TruncSaturatedF64Signed => {
                    let a = pop_val!(self, F64);
                    let r = if a.is_nan() {
                        0
                    } else if a < i32::MIN as f64 {
                        i32::MIN
                    } else if a >= i32::MAX as f64 + 1.0 {
                        i32::MAX
                    } else {
                        a as i32
                    };
                    self.stack.push(r);
                }
                Op::I32TruncSaturatedF64Unsigned => {
                    let a = pop_val!(self, F64);
                    let r = if a.is_nan() || a < 0.0 {
                        0u32
                    } else if a >= u32::MAX as f64 + 1.0 {
                        u32::MAX
                    } else {
                        a as u32
                    };
                    self.stack.push(r as i32);
                }
                Op::I64TruncSaturatedF32Signed => {
                    let a = pop_val!(self, F32);
                    let r = if a.is_nan() {
                        0i64
                    } else if a < i64::MIN as f32 {
                        i64::MIN
                    } else if a >= i64::MAX as f32 {
                        i64::MAX
                    } else {
                        a as i64
                    };
                    self.stack.push(r);
                }
                Op::I64TruncSaturatedF32Unsigned => {
                    let a = pop_val!(self, F32);
                    let r = if a.is_nan() || a < 0.0 {
                        0u64
                    } else if a >= u64::MAX as f32 {
                        u64::MAX
                    } else {
                        a as u64
                    };
                    self.stack.push(r as i64);
                }
                Op::I64TruncSaturatedF64Signed => {
                    let a = pop_val!(self, F64);
                    let r = if a.is_nan() {
                        0i64
                    } else if a < i64::MIN as f64 {
                        i64::MIN
                    } else if a >= i64::MAX as f64 {
                        i64::MAX
                    } else {
                        a as i64
                    };
                    self.stack.push(r);
                }
                Op::I64TruncSaturatedF64Unsigned => {
                    let a = pop_val!(self, F64);
                    let r = if a.is_nan() || a < 0.0 {
                        0u64
                    } else if a >= u64::MAX as f64 {
                        u64::MAX
                    } else {
                        a as u64
                    };
                    self.stack.push(r as i64);
                }
                Op::I32EqZeroJumpIf { target, keep, drop } => {
                    cmp_branch_zero!(self, depth, I32, target, keep, drop, ==)
                }
                Op::I32EqZeroJumpIfNot { target, keep, drop } => {
                    cmp_branch_zero!(self, depth, I32, target, keep, drop, !=)
                }
                Op::I32EqJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I32, target, keep, drop, ==)
                }
                Op::I32NeJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I32, target, keep, drop, !=)
                }
                Op::I32LtSignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I32, target, keep, drop, <)
                }
                Op::I32LtUnsignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I32, target, keep, drop, u32, <)
                }
                Op::I32GtSignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I32, target, keep, drop, >)
                }
                Op::I32GtUnsignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I32, target, keep, drop, u32, >)
                }
                Op::I32LeSignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I32, target, keep, drop, <=)
                }
                Op::I32LeUnsignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I32, target, keep, drop, u32, <=)
                }
                Op::I32GeSignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I32, target, keep, drop, >=)
                }
                Op::I32GeUnsignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I32, target, keep, drop, u32, >=)
                }
                Op::I64EqZeroJumpIf { target, keep, drop } => {
                    cmp_branch_zero!(self, depth, I64, target, keep, drop, ==)
                }
                Op::I64EqJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I64, target, keep, drop, ==)
                }
                Op::I64NeJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I64, target, keep, drop, !=)
                }
                Op::I64LtSignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I64, target, keep, drop, <)
                }
                Op::I64LtUnsignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I64, target, keep, drop, u64, <)
                }
                Op::I64GtSignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I64, target, keep, drop, >)
                }
                Op::I64GtUnsignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I64, target, keep, drop, u64, >)
                }
                Op::I64LeSignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I64, target, keep, drop, <=)
                }
                Op::I64LeUnsignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I64, target, keep, drop, u64, <=)
                }
                Op::I64GeSignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I64, target, keep, drop, >=)
                }
                Op::I64GeUnsignedJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, I64, target, keep, drop, u64, >=)
                }
                Op::F32EqJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, F32, target, keep, drop, ==)
                }
                Op::F32NeJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, F32, target, keep, drop, !=)
                }
                Op::F32LtJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, F32, target, keep, drop, <)
                }
                Op::F32GtJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, F32, target, keep, drop, >)
                }
                Op::F32LeJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, F32, target, keep, drop, <=)
                }
                Op::F32GeJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, F32, target, keep, drop, >=)
                }
                Op::F64EqJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, F64, target, keep, drop, ==)
                }
                Op::F64NeJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, F64, target, keep, drop, !=)
                }
                Op::F64LtJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, F64, target, keep, drop, <)
                }
                Op::F64GtJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, F64, target, keep, drop, >)
                }
                Op::F64LeJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, F64, target, keep, drop, <=)
                }
                Op::F64GeJumpIf { target, keep, drop } => {
                    cmp_branch!(self, depth, F64, target, keep, drop, >=)
                }
                Op::LocalGet2 {
                    local_idx_a,
                    local_idx_b,
                } => {
                    let locals = &self.call_stack[depth].locals;

                    self.stack.extend_from_slice(&[
                        locals[local_idx_a as usize],
                        locals[local_idx_b as usize],
                    ]);
                }
                Op::LocalGetReturn { local_idx } => {
                    self.do_local_get(local_idx as usize, depth);
                    self.do_return(depth);
                }
                Op::LocalGetI32Load {
                    local_idx,
                    offset,
                    memory,
                } => local_get_load!(self, depth, mi, local_idx, offset, memory, 4, |b| {
                    i32::from_le_bytes(b)
                }),
                Op::LocalGetI64Load {
                    local_idx,
                    offset,
                    memory,
                } => local_get_load!(self, depth, mi, local_idx, offset, memory, 8, |b| {
                    i64::from_le_bytes(b)
                }),
                Op::LocalGetF32Load {
                    local_idx,
                    offset,
                    memory,
                } => local_get_load!(self, depth, mi, local_idx, offset, memory, 4, |b| {
                    f32::from_le_bytes(b)
                }),
                Op::LocalGetF64Load {
                    local_idx,
                    offset,
                    memory,
                } => local_get_load!(self, depth, mi, local_idx, offset, memory, 8, |b| {
                    f64::from_le_bytes(b)
                }),
                Op::LocalGetI32Store {
                    local_idx,
                    offset,
                    memory,
                } => local_get_store!(
                    self,
                    depth,
                    mi,
                    local_idx,
                    offset,
                    memory,
                    4,
                    |v: RawValue| v.as_i32().to_le_bytes()
                ),
                Op::LocalGetI64Store {
                    local_idx,
                    offset,
                    memory,
                } => local_get_store!(
                    self,
                    depth,
                    mi,
                    local_idx,
                    offset,
                    memory,
                    8,
                    |v: RawValue| v.as_i64().to_le_bytes()
                ),
                Op::LocalGetF32Store {
                    local_idx,
                    offset,
                    memory,
                } => local_get_store!(
                    self,
                    depth,
                    mi,
                    local_idx,
                    offset,
                    memory,
                    4,
                    |v: RawValue| v.as_f32().to_le_bytes()
                ),
                Op::LocalGetF64Store {
                    local_idx,
                    offset,
                    memory,
                } => local_get_store!(
                    self,
                    depth,
                    mi,
                    local_idx,
                    offset,
                    memory,
                    8,
                    |v: RawValue| v.as_f64().to_le_bytes()
                ),
                Op::LocalGetLocalSet {
                    local_get_idx,
                    local_set_idx,
                } => {
                    let locals = &mut self.call_stack[depth].locals;
                    let out = locals[local_get_idx as usize];

                    locals[local_set_idx as usize] = out;
                }
                _ => todo!(),
            }
        }
    }

    fn do_local_get(&mut self, local_idx: usize, depth: usize) {
        let locals = &self.call_stack[depth].locals;
        debug_assert!(
            local_idx < locals.len(),
            "compiler error: local index {local_idx} oob (func has {} locals)",
            locals.len()
        );

        self.stack.push(locals[local_idx]);
    }

    fn handle_exception(&mut self, tag_addr: usize, values: Vec<RawValue>) -> Result<()> {
        while let Some(frame) = self.catch_stack.last() {
            let mi = frame.module_idx as usize;
            let handler_idx = frame.handler_idx as usize;
            let call_depth = frame.call_depth;
            let stack_restore = frame.stack_restore;

            let clauses = self.instances[mi].code.catch_handlers[handler_idx].clone();

            for clause in &clauses {
                let matches = match clause.kind {
                    CatchKind::Catch | CatchKind::CatchRef => {
                        let clause_tag_addr = self.instances[mi].tag_addrs[clause.tag_idx as usize];
                        clause_tag_addr == tag_addr
                    }
                    CatchKind::CatchAll | CatchKind::CatchAllRef => true,
                };

                if matches {
                    self.catch_stack.pop();

                    while self.call_stack.len() > call_depth {
                        self.call_stack.pop();
                    }

                    self.stack.truncate(stack_restore);

                    match clause.kind {
                        CatchKind::Catch | CatchKind::CatchRef => {
                            for v in &values {
                                self.stack.push(*v);
                            }
                        }
                        _ => {}
                    }

                    match clause.kind {
                        CatchKind::CatchRef | CatchKind::CatchAllRef => {
                            let exn_idx = self.exceptions.len();
                            self.exceptions.push(Exception { tag_addr, values });
                            self.stack.push(RawValue::from_ref(Ref::ExnRef(exn_idx)));
                        }
                        _ => {}
                    }

                    let n_values = clause.n_values as usize;
                    let drop = clause.drop as usize;
                    self.stack.keep_top(n_values, drop);

                    let depth = self.call_stack.len() - 1;
                    self.call_stack[depth].pc = clause.target as usize;

                    return Ok(());
                }
            }

            self.catch_stack.pop();
        }

        Err(Error::Exception(Exception { tag_addr, values }))
    }

    fn pop_catch_frames(&mut self, depth: usize) {
        while self
            .catch_stack
            .last()
            .is_some_and(|f| f.call_depth > depth)
        {
            self.catch_stack.pop();
        }
    }

    fn do_return(&mut self, depth: usize) {
        let arity = self.call_stack[depth].arity;
        let base = self.call_stack[depth].stack_base;
        let len = self.stack.len();
        debug_assert!(
            len >= arity,
            "do_return: stack len {} < arity {}, base={}, func={}, mi={}",
            len,
            arity,
            base,
            self.call_stack[depth].compiled_func_idx,
            self.call_stack[depth].module_idx,
        );
        if arity > 0 {
            self.stack.copy_within(len - arity..len, base);
        }
        self.stack.truncate(base + arity);
        self.call_stack.pop();
    }

    fn func_num_params(&self, func_addr: usize) -> usize {
        match &self.functions[func_addr] {
            FunctionInstance::Local { function_type, .. }
            | FunctionInstance::Host { function_type, .. } => function_type.0 .0.len(),
        }
    }

    fn run_init_instructions(
        &mut self,
        instructions: &[Instruction],
        module_idx: u16,
    ) -> Result<()> {
        let mut ops = Vec::with_capacity(instructions.len() + 1);
        for instr in instructions {
            match instr {
                Instruction::I32Const(v) => ops.push(Op::I32Const { value: *v }),
                Instruction::I64Const(v) => ops.push(Op::I64Const { value: *v }),
                Instruction::MemoryInit(data_idx, mem) => ops.push(Op::MemoryInit {
                    data_idx: *data_idx,
                    memory_idx: *mem,
                }),
                Instruction::DataDrop(idx) => ops.push(Op::DataDrop { data_idx: *idx }),
                Instruction::TableInit(table_idx, elem_idx) => ops.push(Op::TableInit {
                    table_idx: *table_idx,
                    elem_idx: *elem_idx,
                }),
                Instruction::ElemDrop(idx) => ops.push(Op::ElemDrop { elem_idx: *idx }),
                Instruction::RefNull(ht) => ops.push(Op::RefNull(*ht)),
                Instruction::RefFunc(idx) => ops.push(Op::RefFunc { func_idx: *idx }),
                Instruction::GlobalGet(idx) => ops.push(Op::GlobalGet { global_idx: *idx }),
                Instruction::I32Add => ops.push(Op::I32Add),
                Instruction::I32Sub => ops.push(Op::I32Sub),
                Instruction::I32Mul => ops.push(Op::I32Mul),
                Instruction::I64Add => ops.push(Op::I64Add),
                Instruction::I64Sub => ops.push(Op::I64Sub),
                Instruction::I64Mul => ops.push(Op::I64Mul),
                Instruction::F32Const(v) => ops.push(Op::F32Const { value: *v }),
                Instruction::F64Const(v) => ops.push(Op::F64Const { value: *v }),
                other => instantiation_err!("unexpected instruction in init sequence: {:?}", other),
            }
        }
        ops.push(Op::Return);

        let max_stack_height = ops.len() as u32;

        let cf = CompiledFunction {
            #[cfg(feature = "debugger")]
            source_positions: vec![0; ops.len()],
            ops,
            type_index: 0,
            num_args: 0,
            local_types: vec![],
            max_stack_height,
        };

        let code = Arc::make_mut(&mut self.instances[module_idx as usize].code);
        let compiled_func_idx = code.compiled_funcs.len();
        code.compiled_funcs.push(cf);

        self.call_stack.push(CallFrame {
            module_idx,
            compiled_func_idx: compiled_func_idx as u32,
            pc: 0,
            locals: vec![],
            stack_base: self.stack.len(),
            arity: 0,
        });
        self.run()?;
        Ok(())
    }
}

const fn limits_match(actual: &Limit, expected: &Limit) -> bool {
    actual.min >= expected.min && (expected.max == u64::MAX || actual.max <= expected.max)
}

fn run_data(index: u32, data_segment: &DataSegment) -> Vec<Instruction> {
    match &data_segment.mode {
        DataMode::Passive => vec![],
        DataMode::Active { memory, offset } => {
            let n = data_segment.bytes.len();
            let mut instrs = offset.clone();
            instrs.extend([
                Instruction::I32Const(0),
                Instruction::I32Const(n as i32),
                Instruction::MemoryInit(index, *memory),
                Instruction::DataDrop(index),
            ]);
            instrs
        }
    }
}

fn run_elem(index: u32, element_segment: &ElementSegment) -> Vec<Instruction> {
    match &element_segment.mode {
        ElementMode::Passive => vec![],
        ElementMode::Declarative => vec![Instruction::ElemDrop(index)],
        ElementMode::Active {
            table_index,
            offset,
        } => {
            let n = element_segment.expression.len();
            let mut instrs = offset.clone();
            instrs.extend([
                Instruction::I32Const(0),
                Instruction::I32Const(n as i32),
                Instruction::TableInit(*table_index, index),
                Instruction::ElemDrop(index),
            ]);
            instrs
        }
    }
}

fn eval_const_expr_with_module(
    expr: &[Instruction],
    store: &Store,
    address_map: &AddressMap,
) -> Result<RawValue> {
    let mut stack = Vec::with_capacity(expr.len());
    for instr in expr {
        match instr {
            Instruction::I32Const(v) => stack.push(RawValue::from(*v)),
            Instruction::I64Const(v) => stack.push(RawValue::from(*v)),
            Instruction::F32Const(v) => stack.push(RawValue::from(*v)),
            Instruction::F64Const(v) => stack.push(RawValue::from(*v)),
            Instruction::V128Const(v) => {
                let (hi, lo) = RawValue::from_v128(*v);
                stack.extend(<[RawValue; 2]>::from((hi, lo)));
            }
            Instruction::RefNull(_) => stack.push(RawValue::from_ref(Ref::Null)),
            Instruction::RefFunc(idx) => {
                let addr = *address_map
                    .function_addrs
                    .get(*idx as usize)
                    .ok_or_else(|| Error::Instantiation(format!("ref.func index {} oob", idx)))?;
                stack.push(RawValue::from_ref(Ref::FunctionAddr(addr)));
            }
            Instruction::GlobalGet(idx) => {
                let store_idx = *address_map.global_addrs.get(*idx as usize).ok_or_else(|| {
                    Error::Instantiation(format!("global index {} oob in const expr", idx))
                })?;
                let global = store.globals.get(store_idx).ok_or_else(|| {
                    Error::Instantiation(format!(
                        "global store index {} oob in const expr",
                        store_idx
                    ))
                })?;
                stack.push(global.value);
            }
            Instruction::RefI31 => {
                let v = const_pop_i32(&mut stack)?;
                stack.push(RawValue::from_ref(Ref::I31(v & 0x7FFF_FFFF)));
            }
            Instruction::I32Add => {
                let (b, a) = (const_pop_i32(&mut stack)?, const_pop_i32(&mut stack)?);
                stack.push(RawValue::from(a.wrapping_add(b)));
            }
            Instruction::I32Sub => {
                let (b, a) = (const_pop_i32(&mut stack)?, const_pop_i32(&mut stack)?);
                stack.push(RawValue::from(a.wrapping_sub(b)));
            }
            Instruction::I32Mul => {
                let (b, a) = (const_pop_i32(&mut stack)?, const_pop_i32(&mut stack)?);
                stack.push(RawValue::from(a.wrapping_mul(b)));
            }
            Instruction::I64Add => {
                let (b, a) = (const_pop_i64(&mut stack)?, const_pop_i64(&mut stack)?);
                stack.push(RawValue::from(a.wrapping_add(b)));
            }
            Instruction::I64Sub => {
                let (b, a) = (const_pop_i64(&mut stack)?, const_pop_i64(&mut stack)?);
                stack.push(RawValue::from(a.wrapping_sub(b)));
            }
            Instruction::I64Mul => {
                let (b, a) = (const_pop_i64(&mut stack)?, const_pop_i64(&mut stack)?);
                stack.push(RawValue::from(a.wrapping_mul(b)));
            }
            other => instantiation_err!("unexpected instruction in const expr: {:?}", other),
        }
    }
    stack
        .pop()
        .ok_or_else(|| Error::Instantiation("const expr produced no value".into()))
}

fn const_pop_i32(stack: &mut Vec<RawValue>) -> Result<i32> {
    stack
        .pop()
        .map(|v| v.as_i32())
        .ok_or_else(|| Error::Instantiation("stack underflow in const expr".into()))
}

fn const_pop_i64(stack: &mut Vec<RawValue>) -> Result<i64> {
    stack
        .pop()
        .map(|v| v.as_i64())
        .ok_or_else(|| Error::Instantiation("stack underflow in const expr".into()))
}

impl Store {
    pub fn snapshot(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        buf.extend_from_slice(SNAPSHOT_MAGIC);
        SNAPSHOT_VERSION.encode(&mut buf);

        // encode the function type per entry
        (self.functions.len() as u32).encode(&mut buf);
        for fi in &self.functions {
            match fi {
                FunctionInstance::Local { function_type, .. } => {
                    0u8.encode(&mut buf);
                    function_type.encode(&mut buf);
                }
                FunctionInstance::Host {
                    function_type,
                    module_name,
                    function_name,
                } => {
                    1u8.encode(&mut buf);
                    function_type.encode(&mut buf);
                    module_name.encode(&mut buf);
                    function_name.encode(&mut buf);
                }
            }
        }

        // tables
        (self.tables.len() as u32).encode(&mut buf);
        for table in &self.tables {
            table.table_type.encode(&mut buf);
            table.elem.encode(&mut buf);
        }

        // memories
        (self.memories.len() as u32).encode(&mut buf);
        for mem in &self.memories {
            mem.memory_type.encode(&mut buf);
            (mem.data.len() as u64).encode(&mut buf);
            buf.extend_from_slice(&mem.data);
        }

        // globals
        (self.globals.len() as u32).encode(&mut buf);
        for g in &self.globals {
            g.global_type.encode(&mut buf);
            g.value.encode(&mut buf);
        }

        // tags
        (self.tags.len() as u32).encode(&mut buf);
        for t in &self.tags {
            t.tag_type.encode(&mut buf);
        }

        // element segments
        (self.element_segments.len() as u32).encode(&mut buf);
        for es in &self.element_segments {
            es.ref_type.encode(&mut buf);
            es.elem.encode(&mut buf);
        }

        // data segments
        (self.data_segments.len() as u32).encode(&mut buf);
        for ds in &self.data_segments {
            (ds.data.len() as u32).encode(&mut buf);
            buf.extend_from_slice(&ds.data);
        }

        // instances
        self.instances.encode(&mut buf);

        // func_addr_to_module
        self.func_addr_to_module.encode(&mut buf);

        // value stack
        let (stack_data, stack_cursor) = self.stack.snapshot_data();
        (stack_data.len() as u32).encode(&mut buf);
        encode_bulk(stack_data, &mut buf);
        stack_cursor.encode(&mut buf);

        // call stack
        self.call_stack.encode(&mut buf);

        // fuel + pending_arity
        self.fuel.encode(&mut buf);
        self.pending_arity.encode(&mut buf);

        buf
    }

    pub fn from_snapshot(bytes: &[u8]) -> Self {
        let buf = &mut &bytes[..];

        let magic: [u8; 4] = buf[..4].try_into().unwrap();
        assert_eq!(&magic, SNAPSHOT_MAGIC, "invalid snapshot magic");
        *buf = &buf[4..];
        let version = u32::decode(buf);
        assert_eq!(
            version, SNAPSHOT_VERSION,
            "unsupported snapshot version {version}"
        );

        // functions
        let num_funcs = u32::decode(buf) as usize;
        let mut functions = Vec::with_capacity(num_funcs);
        let dummy_address_map = Rc::new(AddressMap::default());
        let dummy_function = Function {
            type_index: 0,
            locals: vec![],
            body: vec![],
        };

        for _ in 0..num_funcs {
            let tag = u8::decode(buf);
            match tag {
                0 => {
                    let function_type = FunctionType::decode(buf);
                    functions.push(FunctionInstance::Local {
                        function_type,
                        address_map: Rc::clone(&dummy_address_map),
                        code: dummy_function.clone(),
                    });
                }
                1 => {
                    let function_type = FunctionType::decode(buf);
                    let module_name = String::decode(buf);
                    let function_name = String::decode(buf);
                    functions.push(FunctionInstance::Host {
                        function_type,
                        module_name,
                        function_name,
                    });
                }
                _ => panic!("invalid function instance tag: {tag}"),
            }
        }

        // tables
        let num_tables = u32::decode(buf) as usize;
        let mut tables = Vec::with_capacity(num_tables);
        for _ in 0..num_tables {
            let table_type = TableType::decode(buf);
            let elem = Vec::decode(buf);
            tables.push(TableInstance { table_type, elem });
        }

        // memories
        let num_memories = u32::decode(buf) as usize;
        let mut memories = Vec::with_capacity(num_memories);
        for _ in 0..num_memories {
            let memory_type = MemoryType::decode(buf);
            let data_len = u64::decode(buf) as usize;
            let data = buf[..data_len].to_vec();
            *buf = &buf[data_len..];
            memories.push(MemoryInstance { memory_type, data });
        }

        // globals
        let num_globals = u32::decode(buf) as usize;
        let mut globals = Vec::with_capacity(num_globals);
        for _ in 0..num_globals {
            let global_type = GlobalType::decode(buf);
            let value = RawValue::decode(buf);
            globals.push(GlobalInstance { global_type, value });
        }

        // tags
        let num_tags = u32::decode(buf) as usize;
        let mut tags = Vec::with_capacity(num_tags);
        for _ in 0..num_tags {
            let tag_type = FunctionType::decode(buf);
            tags.push(TagInstance { tag_type });
        }

        // element segments
        let num_elems = u32::decode(buf) as usize;
        let mut element_segments = Vec::with_capacity(num_elems);
        for _ in 0..num_elems {
            let ref_type = RefType::decode(buf);
            let elem = Vec::decode(buf);
            element_segments.push(ElementInstance { ref_type, elem });
        }

        // data segments
        let num_data = u32::decode(buf) as usize;
        let mut data_segments = Vec::with_capacity(num_data);
        for _ in 0..num_data {
            let len = u32::decode(buf) as usize;
            let data = buf[..len].to_vec();
            *buf = &buf[len..];
            data_segments.push(DataInstance { data });
        }

        // instances
        let instances = Vec::decode(buf);

        // func_addr_to_module
        let func_addr_to_module = Vec::decode(buf);

        // value stack
        let _stack_capacity = u32::decode(buf) as usize;
        let stack_data = decode_bulk(buf);
        let stack_cursor = usize::decode(buf);
        let stack = ValueStack::from_snapshot(stack_data, stack_cursor);

        // call stack
        let call_stack = Vec::decode(buf);

        // fuel + pending_arity
        let fuel = Option::decode(buf);
        let pending_arity = Option::decode(buf);

        Self {
            functions,
            tables,
            memories,
            globals,
            tags,
            element_segments,
            data_segments,
            instances,
            func_addr_to_module,
            stack,
            call_stack,
            catch_stack: Vec::new(),
            exceptions: Vec::new(),
            fuel,
            pending_arity,
            pending_suspension: None,
            component_instances: Vec::new(),
        }
    }
}
