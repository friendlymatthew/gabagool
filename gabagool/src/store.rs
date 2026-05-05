use std::collections::HashMap;
use std::fmt::Debug;
use std::ops::Neg;
use std::rc::Rc;
use std::sync::Arc;

use crate::compiler::ModuleCode;
use crate::component::binary_grammar::{
    ComponentDefinedKind, ComponentTypeDef, ComponentValueKind, PrimitiveValueKind,
};
use crate::component::flatten::Initializer;
use crate::error::{Error, Result};
#[cfg(feature = "jit")]
use crate::jit::assembler::JitFunction;
use crate::{
    compiler, ensure, instantiation_err, trap, AddrType, Component, ComponentInstance,
    ComponentValue, DataMode, ElementMode, GuestMemory, ImportDescription, InstantiatedComponent,
    Instruction, LiftedFunc, Module, Mutability, Trap,
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
use crate::ir::{CatchKind, CompiledFunction, CompilerMode, Op};
use crate::snapshot::{decode_bulk, encode_bulk, Snapshot, SNAPSHOT_MAGIC, SNAPSHOT_VERSION};
use crate::value_stack::ValueStack;
use crate::RawValue;

pub const PAGE_SIZE: usize = 65536;
pub const MAX_CALL_DEPTH: usize = 1024;
const MAX_FLAT_RESULTS: usize = 1;

#[derive(Debug, Clone)]
pub enum ExecutionState<V = RawValue> {
    Completed(Vec<V>),
    FuelExhausted,
    Suspended {
        module_name: String,
        func_name: String,
        args: Vec<RawValue>,
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
    ($self:expr, $depth:expr, $mi:expr, $local_i:expr, $offset:expr, $memory:expr, $width:literal, |$bytes:ident| $convert:expr) => {{
        let locals = &$self.call_stack[$depth].locals;
        let mem_addr = $self.instances[$mi].mem_addrs[$memory as usize];
        let mem = &$self.memories[mem_addr];
        let base = match mem.memory_type.addr_type {
            AddrType::I32 => locals[$local_i as usize].as_i32() as u64,
            AddrType::I64 => locals[$local_i as usize].as_i64() as u64,
        };
        let ea = base
            .checked_add($offset as u64)
            .and_then(|v| usize::try_from(v).ok());
        let Some(ea) = ea.filter(|&ea| ea.saturating_add($width) <= mem.data.len()) else {
            trap!(Trap::OutOfBoundsMemoryAccess);
        };
        let $bytes: [u8; $width] = mem.data.read_fixed::<$width>(ea);
        $self.stack.push($convert);
    }};
}

macro_rules! local_get_store {
    ($self:expr, $depth:expr, $mi:expr, $local_i:expr, $offset:expr, $memory:expr, $width:literal, $to_bytes:expr) => {{
        let locals = &$self.call_stack[$depth].locals;
        let val = locals[$local_i as usize];
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
        mem.data.write_bytes(ea, &bytes);
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
        let $bytes: [u8; $width] = mem.data.read_fixed::<$width>(ea);

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
        mem.data.write_bytes(ea, &bytes);
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

pub struct CallFrame {
    pub module_i: u16,
    pub compiled_func_i: u32,
    pub pc: usize,
    pub locals: Vec<RawValue>,
    pub stack_base: usize,
    pub arity: usize,
}

struct CatchFrame {
    call_depth: usize,
    stack_restore: usize,
    module_i: u16,
    handler_i: u32,
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
    /// maps func addr → (instance_i, compiled_func_i)
    func_addr_to_module: Vec<Option<(u16, u32)>>,

    mmap_backing: bool,

    // execution state
    stack: ValueStack,
    call_stack: Vec<CallFrame>,
    catch_stack: Vec<CatchFrame>,
    exceptions: Vec<Exception>,
    fuel: Option<u64>,
    pending_arity: Option<usize>,
    pending_suspension: Option<(String, String, Vec<RawValue>)>,
    pending_lifted: Option<(LiftedFunc, Vec<ComponentTypeDef>)>,

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
        Self::with_backing(false)
    }

    #[cfg(unix)]
    pub fn new_cow() -> Self {
        Self::with_backing(true)
    }

    fn with_backing(mmap_backing: bool) -> Self {
        Self {
            functions: Vec::new(),
            tables: Vec::new(),
            memories: Vec::new(),
            globals: Vec::new(),
            tags: Vec::new(),
            element_segments: Vec::new(),
            data_segments: Vec::new(),
            stack: ValueStack::with_capacity(1024),
            call_stack: Vec::new(),
            catch_stack: Vec::new(),
            exceptions: Vec::new(),
            fuel: None,
            pending_arity: None,
            pending_suspension: None,
            pending_lifted: None,
            instances: Vec::new(),
            func_addr_to_module: Vec::new(),
            component_instances: Vec::new(),
            mmap_backing,
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

    fn get_memory(&self, instance: Instance, name: &str) -> Result<usize> {
        for export in self.exports(instance) {
            if export.name == name {
                if let ExternalValue::Memory { addr } = export.value {
                    return Ok(addr);
                }
                instantiation_err!("export '{}' is not a memory", name);
            }
        }
        instantiation_err!("memory export '{}' not found", name)
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

        let data = if self.mmap_backing {
            #[cfg(unix)]
            {
                const MAX_PAGES: usize = 65536;
                let max_pages = (memory_type.limit.max as usize).clamp(1, MAX_PAGES);
                let capacity = max_pages * PAGE_SIZE;
                GuestMemory::with_mmap(n, capacity)
                    .expect("mmap allocation for linear memory failed")
            }
            #[cfg(not(unix))]
            {
                unimplemented!()
            }
        } else {
            GuestMemory::new(n)
        };

        self.memories.push(MemoryInstance { memory_type, data });

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
                (ExternalValue::Function { addr }, ImportDescription::Func(type_i)) => {
                    let expected = match &module.types()[*type_i as usize].composite_type {
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
                (ExternalValue::Tag { addr }, ImportDescription::Tag(type_i)) => {
                    let expected = match &module.types()[*type_i as usize].composite_type {
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

        let start_func_i = module.start;
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
            customs: Vec::new(),
        };
        let module_instance = self.allocate_module(
            parsed_clone,
            external_addresses,
            initial_global_values,
            initial_table_refs,
            element_segment_refs,
        )?;

        // build the InstanceEntity with shared code + address mappings
        let instance_i = self.instances.len() as u16;
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

        // build a mapping from func_addr to (instance_i, compiled func index)
        if self.func_addr_to_module.len() < self.functions.len() {
            self.func_addr_to_module.resize(self.functions.len(), None);
        }
        let first_compiled_i = entity.code.compiled_funcs.len() - num_local_funcs;
        for (i, &addr) in module_instance
            .function_addrs
            .iter()
            .rev()
            .take(num_local_funcs)
            .rev()
            .enumerate()
        {
            self.func_addr_to_module[addr] = Some((instance_i, (first_compiled_i + i) as u32));
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
                let cf = compiler::compile_function_into_code(
                    &types_for_compile,
                    code,
                    code_mut,
                    module.compile_mode,
                );
                let i = code_mut.compiled_funcs.len();
                code_mut.compiled_funcs.push(cf);
                if addr < self.func_addr_to_module.len() {
                    self.func_addr_to_module[addr] = Some((instance_i, i as u32));
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
        let instance = Instance(instance_i as usize);

        // step 27 - execute element segment initialization
        // step 28 - execute data segment initialization
        let init_instructions = [element_instructions, data_instructions].concat();
        if !init_instructions.is_empty() {
            self.run_init_instructions(&init_instructions, instance_i)?;
        }

        // step 29: invoke start function if present
        if let Some(start_i) = start_func_i {
            let func_addr = *module_instance
                .function_addrs
                .get(start_i as usize)
                .ok_or_else(|| {
                    Error::Instantiation(format!("start function index {} oob", start_i))
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
        let flattened = &component.flattened;

        let mut compiled_modules = Vec::with_capacity(flattened.modules.len());

        for module in &flattened.modules {
            compiled_modules.push(Module::from_parsed(
                (**module).clone(),
                CompilerMode::Optimize,
            )?);
        }

        let mut core_instances = Vec::new();
        let mut core_funcs = Vec::new();
        let mut core_memories = Vec::new();
        let mut component_funcs = Vec::new();
        let mut component_exports = HashMap::new();

        for init in &flattened.initializers {
            match init {
                Initializer::InstantiateModule { module_i } => {
                    let inst = self.instantiate(&compiled_modules[*module_i], vec![])?;
                    core_instances.push(inst);
                }
                Initializer::AliasCoreFunc { instance_i, name } => {
                    let addr = self.get_func(core_instances[*instance_i], name)?;
                    core_funcs.push(addr);
                }
                Initializer::AliasCoreMemory { instance_i, name } => {
                    let addr = self.get_memory(core_instances[*instance_i], name)?;
                    core_memories.push(addr);
                }
                Initializer::Lift {
                    core_func_i,
                    opts,
                    type_i,
                } => {
                    let func_addr = core_funcs[*core_func_i];
                    let memory_addr = opts.memory.map(|i| core_memories[i as usize]);
                    let realloc_addr = opts.realloc.map(|i| core_funcs[i as usize]);

                    let func_type = match &flattened.types[*type_i as usize] {
                        ComponentTypeDef::Func(ft) => ft.clone(),
                        other => {
                            instantiation_err!("canon lift type_i points to non-func: {other:?}")
                        }
                    };

                    component_funcs.push(LiftedFunc {
                        func_addr,
                        memory_addr,
                        realloc_addr,
                        string_encoding: opts.string_encoding,
                        func_type,
                    });
                }
                Initializer::Export { name, func_i } => {
                    let lifted = component_funcs[*func_i].clone();
                    component_exports.insert(name.clone(), lifted);
                }
            }
        }

        self.component_instances.push(InstantiatedComponent {
            exports: component_exports,
            types: flattened.types.clone(),
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
        I: IntoIterator,
        I::Item: Into<ComponentValue>,
    {
        let instance = &self.component_instances[instance.0];
        let lifted = instance
            .exports
            .get(name)
            .ok_or_else(|| Error::Instantiation(format!("export '{name}' not found")))?
            .clone();

        let types = instance.types.clone();

        let component_args = args.into_iter().map(Into::into);

        let mut raw_args = self.lower_values(component_args, &lifted, &types)?;

        let result_types = lifted.func_type.results.types();
        let flat_result_count = result_types.iter().map(|ty| ty.flat_count(&types)).sum();

        let retptr = (flat_result_count > MAX_FLAT_RESULTS)
            .then(|| {
                let mem_addr = lifted
                    .memory_addr
                    .ok_or_else(|| Error::Instantiation("canon lift missing memory".into()))?;

                let realloc_addr = lifted
                    .realloc_addr
                    .ok_or_else(|| Error::Instantiation("canon lift missing realloc".into()))?;

                let byte_size = (flat_result_count * 4) as i32;
                let ptr = self.call_realloc(realloc_addr, 0, 0, 4, byte_size)?;

                raw_args.push(RawValue::from(ptr));

                Ok::<_, Error>((ptr as usize, mem_addr))
            })
            .transpose()?;

        let result = self.invoke_by_addr(lifted.func_addr, raw_args)?;

        Ok(match result {
            ExecutionState::Completed(raw_values) => {
                self.pending_lifted = None;
                let flat = if let Some((ptr, mem_addr)) = retptr {
                    let mem = &self.memories[mem_addr];

                    (0..flat_result_count)
                        .map(|n| {
                            let offset = ptr + n * 4;
                            let bytes = mem.data.read_fixed::<4>(offset);
                            RawValue::from(i32::from_le_bytes(bytes))
                        })
                        .collect::<Vec<_>>()
                } else {
                    raw_values
                };

                ExecutionState::Completed(self.lift_values(&flat, &lifted, &types)?)
            }
            ExecutionState::FuelExhausted => {
                self.pending_lifted = Some((lifted.clone(), types.clone()));
                ExecutionState::FuelExhausted
            }
            ExecutionState::Suspended {
                module_name,
                func_name,
                args,
            } => {
                self.pending_lifted = Some((lifted.clone(), types.clone()));
                ExecutionState::Suspended {
                    module_name,
                    func_name,
                    args,
                }
            }
        })
    }

    fn call_realloc(
        &mut self,
        realloc_addr: usize,
        old_ptr: i32,
        old_size: i32,
        align: i32,
        new_size: i32,
    ) -> Result<i32> {
        let result = self.invoke_by_addr(
            realloc_addr,
            vec![
                RawValue::from(old_ptr),
                RawValue::from(old_size),
                RawValue::from(align),
                RawValue::from(new_size),
            ],
        )?;

        let out = match result {
            ExecutionState::Completed(vals) => vals[0].as_i32(),
            _ => instantiation_err!("realloc did not complete"),
        };

        Ok(out)
    }

    fn lower_values<I>(
        &mut self,
        args: I,
        lifted: &LiftedFunc,
        types: &[ComponentTypeDef],
    ) -> Result<Vec<RawValue>>
    where
        I: IntoIterator<Item = ComponentValue>,
    {
        let mut flat = Vec::new();
        let param_types = lifted
            .func_type
            .params
            .iter()
            .map(|(_, ty)| ty)
            .collect::<Vec<_>>();

        for (param_i, arg) in args.into_iter().enumerate() {
            let param_ty = param_types.get(param_i).copied();
            self.lower_value(arg, param_ty, types, lifted, &mut flat)?;
        }

        Ok(flat)
    }

    fn lower_value(
        &mut self,
        value: ComponentValue,
        param_ty: Option<&ComponentValueKind>,
        types: &[ComponentTypeDef],
        lifted: &LiftedFunc,
        flat: &mut Vec<RawValue>,
    ) -> Result<()> {
        match value {
            ComponentValue::Bool(v) => flat.push(RawValue::from(v as i32)),
            ComponentValue::S8(v) => flat.push(RawValue::from(v as i32)),
            ComponentValue::U8(v) => flat.push(RawValue::from(v as i32)),
            ComponentValue::S16(v) => flat.push(RawValue::from(v as i32)),
            ComponentValue::U16(v) => flat.push(RawValue::from(v as i32)),
            ComponentValue::S32(v) => flat.push(RawValue::from(v)),
            ComponentValue::U32(v) => flat.push(RawValue::from(v as i32)),
            ComponentValue::S64(v) => flat.push(RawValue::from(v)),
            ComponentValue::U64(v) => flat.push(RawValue::from(v as i64)),
            ComponentValue::F32(v) => flat.push(RawValue::from(v)),
            ComponentValue::F64(v) => flat.push(RawValue::from(v)),
            ComponentValue::Char(v) => flat.push(RawValue::from(v as i32)),
            ComponentValue::String(s) => {
                let bytes = s.as_bytes();
                let len = bytes.len();

                let mem_addr = lifted
                    .memory_addr
                    .ok_or_else(|| Error::Instantiation("canon lift missing memory".into()))?;
                let realloc_addr = lifted
                    .realloc_addr
                    .ok_or_else(|| Error::Instantiation("canon lift missing realloc".into()))?;

                let ptr = self.call_realloc(realloc_addr, 0, 0, 1, len as i32)?;

                let mem = &mut self.memories[mem_addr];
                mem.data.write_bytes(ptr as usize, bytes);

                flat.push(RawValue::from(ptr));
                flat.push(RawValue::from(len as i32));
            }
            ComponentValue::List(elements) => {
                let mem_addr = lifted
                    .memory_addr
                    .ok_or_else(|| Error::Instantiation("canon lift missing memory".into()))?;
                let realloc_addr = lifted
                    .realloc_addr
                    .ok_or_else(|| Error::Instantiation("canon lift missing realloc".into()))?;

                let elem_size = match param_ty {
                    Some(ComponentValueKind::Type(i)) => match &types[*i as usize] {
                        ComponentTypeDef::Defined(ComponentDefinedKind::List(
                            ComponentValueKind::Primitive(p),
                        )) => primitive_byte_size(p),
                        _ => elements.first().map_or(1, component_value_byte_size),
                    },
                    _ => elements.first().map_or(1, component_value_byte_size),
                };

                let len = elements.len();
                let byte_len = len * elem_size;
                let ptr =
                    self.call_realloc(realloc_addr, 0, 0, elem_size as i32, byte_len as i32)?;

                let dest = ptr as usize;
                for (i, elem) in elements.iter().enumerate() {
                    let off = dest + i * elem_size;
                    match elem {
                        ComponentValue::U8(v) => self.memories[mem_addr].data.write_u8(off, *v),
                        ComponentValue::S8(v) => {
                            self.memories[mem_addr].data.write_u8(off, *v as u8)
                        }
                        ComponentValue::U16(v) => self.memories[mem_addr]
                            .data
                            .write_bytes(off, &v.to_le_bytes()),
                        ComponentValue::S16(v) => self.memories[mem_addr]
                            .data
                            .write_bytes(off, &v.to_le_bytes()),
                        ComponentValue::U32(v) => self.memories[mem_addr]
                            .data
                            .write_bytes(off, &v.to_le_bytes()),
                        ComponentValue::S32(v) => self.memories[mem_addr]
                            .data
                            .write_bytes(off, &v.to_le_bytes()),
                        ComponentValue::U64(v) => self.memories[mem_addr]
                            .data
                            .write_bytes(off, &v.to_le_bytes()),
                        ComponentValue::S64(v) => self.memories[mem_addr]
                            .data
                            .write_bytes(off, &v.to_le_bytes()),
                        ComponentValue::F32(v) => self.memories[mem_addr]
                            .data
                            .write_bytes(off, &v.to_le_bytes()),
                        ComponentValue::F64(v) => self.memories[mem_addr]
                            .data
                            .write_bytes(off, &v.to_le_bytes()),
                        _ => todo!("lower list element {:?}", elem),
                    }
                }

                flat.push(RawValue::from(ptr));
                flat.push(RawValue::from(len as i32));
            }
            ComponentValue::Record(fields) => {
                for (_, value) in fields {
                    self.lower_value(value, None, types, lifted, flat)?;
                }
            }
            ComponentValue::Tuple(values) => {
                for value in values {
                    self.lower_value(value, None, types, lifted, flat)?;
                }
            }
            ComponentValue::Variant(case_name, payload) => {
                let defined = resolve_defined_type(param_ty, types);
                let cases = match defined {
                    Some(ComponentDefinedKind::Variant(cases)) => cases,
                    _ => instantiation_err!("variant lower requires variant type"),
                };

                let Some(case_i) = cases.iter().position(|c| c.name == case_name) else {
                    instantiation_err!("enum case not found: {case_name}")
                };

                let max_payload = cases
                    .iter()
                    .map(|c| c.ty.as_ref().map_or(0, |ty| ty.flat_count(types)))
                    .max()
                    .unwrap_or(0);

                flat.push(RawValue::from(case_i as i32));

                let before = flat.len();
                if let Some(value) = payload {
                    self.lower_value(*value, None, types, lifted, flat)?;
                }

                let written = flat.len() - before - 1;
                let padding = max_payload - written;
                flat.extend(std::iter::repeat_n(RawValue::from(i32::MAX), padding));
            }
            ComponentValue::Enum(case_name) => {
                let defined = resolve_defined_type(param_ty, types);

                let cases = match defined {
                    Some(ComponentDefinedKind::Enum(cases)) => cases,
                    _ => instantiation_err!("enum lower requires enum type"),
                };

                let Some(case_i) = cases.iter().position(|c| *c == case_name) else {
                    instantiation_err!("enum case not found: {case_name}")
                };

                flat.push(RawValue::from(case_i as i32));
            }
            ComponentValue::Option(opt) => {
                let defined = resolve_defined_type(param_ty, types);

                let inner_ty = match defined {
                    Some(ComponentDefinedKind::Option(ty)) => ty,
                    _ => instantiation_err!("option lower requires option type"),
                };

                let payload_count = inner_ty.flat_count(types);

                match opt {
                    None => {
                        flat.extend(
                            std::iter::once(RawValue::from(0i32)).chain(std::iter::repeat_n(
                                RawValue::from(i32::MAX),
                                payload_count,
                            )),
                        );
                    }
                    Some(value) => {
                        flat.push(RawValue::from(1i32));
                        self.lower_value(*value, None, types, lifted, flat)?;
                    }
                }
            }
            ComponentValue::Result(result) => {
                let defined = resolve_defined_type(param_ty, types);
                let (ok_ty, err_ty) = match defined {
                    Some(ComponentDefinedKind::Result { ok, err }) => (ok, err),
                    _ => instantiation_err!("result lower requires result type"),
                };

                let ok_count = ok_ty.as_ref().map_or(0, |ty| ty.flat_count(types));
                let err_count = err_ty.as_ref().map_or(0, |ty| ty.flat_count(types));
                let max_payload = ok_count.max(err_count);

                let before = flat.len();

                match result {
                    Ok(value) => {
                        flat.push(RawValue::from(0i32));

                        if let Some(value) = value {
                            self.lower_value(*value, None, types, lifted, flat)?;
                        }
                    }
                    Err(value) => {
                        flat.push(RawValue::from(1i32));

                        if let Some(value) = value {
                            self.lower_value(*value, None, types, lifted, flat)?;
                        }
                    }
                }

                let written = flat.len() - before - 1;
                let padding = max_payload - written;
                flat.extend(std::iter::repeat_n(RawValue::from(i32::MAX), padding));
            }
            ComponentValue::Flags(set_flags) => {
                let all_flags = match resolve_defined_type(param_ty, types) {
                    Some(ComponentDefinedKind::Flags(names)) => names,
                    _ => instantiation_err!("flags lower requires flags type"),
                };

                let num_i32s = all_flags.len().div_ceil(32).max(1);
                let mut words = vec![0u32; num_i32s];

                for name in &set_flags {
                    let bit = all_flags
                        .iter()
                        .position(|n| n == name)
                        .ok_or_else(|| Error::Instantiation(format!("unknown flag: {name}")))?;

                    words[bit / 32] |= 1 << (bit % 32);
                }

                for w in words {
                    flat.push(RawValue::from(w as i32));
                }
            }
        }
        Ok(())
    }

    fn lift_values(
        &self,
        flat: &[RawValue],
        lifted: &LiftedFunc,
        types: &[ComponentTypeDef],
    ) -> Result<Vec<ComponentValue>> {
        let mut results = Vec::new();
        let mut cursor = 0;

        for kind in lifted.func_type.results.types() {
            results.push(self.lift_value(kind, flat, &mut cursor, lifted, types)?);
        }

        Ok(results)
    }

    fn lift_value(
        &self,
        kind: &ComponentValueKind,
        flat: &[RawValue],
        cursor: &mut usize,
        lifted: &LiftedFunc,
        types: &[ComponentTypeDef],
    ) -> Result<ComponentValue> {
        match kind {
            ComponentValueKind::Primitive(p) => {
                let val = flat[*cursor];
                *cursor += 1;

                Ok(match p {
                    PrimitiveValueKind::Bool => ComponentValue::Bool(val.as_i32() != 0),
                    PrimitiveValueKind::S8 => ComponentValue::S8(val.as_i32() as i8),
                    PrimitiveValueKind::U8 => ComponentValue::U8(val.as_i32() as u8),
                    PrimitiveValueKind::S16 => ComponentValue::S16(val.as_i32() as i16),
                    PrimitiveValueKind::U16 => ComponentValue::U16(val.as_i32() as u16),
                    PrimitiveValueKind::S32 => ComponentValue::S32(val.as_i32()),
                    PrimitiveValueKind::U32 => ComponentValue::U32(val.as_i32() as u32),
                    PrimitiveValueKind::S64 => ComponentValue::S64(val.as_i64()),
                    PrimitiveValueKind::U64 => ComponentValue::U64(val.as_i64() as u64),
                    PrimitiveValueKind::F32 => {
                        ComponentValue::F32(f32::from_bits(val.as_i32() as u32))
                    }
                    PrimitiveValueKind::F64 => {
                        ComponentValue::F64(f64::from_bits(val.as_i64() as u64))
                    }
                    PrimitiveValueKind::Char => ComponentValue::Char(
                        char::from_u32(val.as_i32() as u32).unwrap_or('\u{FFFD}'),
                    ),
                    PrimitiveValueKind::String => {
                        let ptr = val.as_i32() as usize;
                        let len = flat[*cursor].as_i32() as usize;
                        *cursor += 1;

                        let mem_addr = lifted.memory_addr.ok_or_else(|| {
                            Error::Instantiation("canon lift missing memory".into())
                        })?;

                        let bytes = self.memories[mem_addr].data.read_bytes(ptr, len);
                        let s = std::str::from_utf8(bytes)
                            .map_err(|e| Error::Instantiation(format!("{e}")))?;

                        ComponentValue::String(s.to_string())
                    }
                })
            }
            ComponentValueKind::Type(i) => match &types[*i as usize] {
                ComponentTypeDef::Defined(ComponentDefinedKind::List(elem_ty)) => {
                    let ptr = flat[*cursor].as_i32() as usize;
                    let len = flat[*cursor + 1].as_i32() as usize;
                    *cursor += 2;

                    let mem_addr = lifted
                        .memory_addr
                        .ok_or_else(|| Error::Instantiation("canon lift missing memory".into()))?;
                    let mem = &self.memories[mem_addr];

                    let elem_prim = match elem_ty {
                        ComponentValueKind::Primitive(p) => p,
                        _ => todo!("lift list of non-primitive"),
                    };

                    let size = primitive_byte_size(elem_prim);
                    let read_elem: fn(&[u8], usize) -> ComponentValue = match elem_prim {
                        PrimitiveValueKind::U8 => |d, o| ComponentValue::U8(d[o]),
                        PrimitiveValueKind::S8 => |d, o| ComponentValue::S8(d[o] as i8),
                        PrimitiveValueKind::U16 => |d, o| {
                            ComponentValue::U16(u16::from_le_bytes(d[o..o + 2].try_into().unwrap()))
                        },
                        PrimitiveValueKind::S16 => |d, o| {
                            ComponentValue::S16(i16::from_le_bytes(d[o..o + 2].try_into().unwrap()))
                        },
                        PrimitiveValueKind::U32 => |d, o| {
                            ComponentValue::U32(u32::from_le_bytes(d[o..o + 4].try_into().unwrap()))
                        },
                        PrimitiveValueKind::S32 => |d, o| {
                            ComponentValue::S32(i32::from_le_bytes(d[o..o + 4].try_into().unwrap()))
                        },
                        PrimitiveValueKind::U64 => |d, o| {
                            ComponentValue::U64(u64::from_le_bytes(d[o..o + 8].try_into().unwrap()))
                        },
                        PrimitiveValueKind::S64 => |d, o| {
                            ComponentValue::S64(i64::from_le_bytes(d[o..o + 8].try_into().unwrap()))
                        },
                        PrimitiveValueKind::F32 => |d, o| {
                            ComponentValue::F32(f32::from_le_bytes(d[o..o + 4].try_into().unwrap()))
                        },
                        PrimitiveValueKind::F64 => |d, o| {
                            ComponentValue::F64(f64::from_le_bytes(d[o..o + 8].try_into().unwrap()))
                        },
                        _ => todo!("lift list element {:?}", elem_prim),
                    };

                    let elements = (0..len)
                        .map(|j| read_elem(mem.data.as_slice(), ptr + j * size))
                        .collect();

                    Ok(ComponentValue::List(elements))
                }
                ComponentTypeDef::Defined(ComponentDefinedKind::Record(field_types)) => {
                    let mut fields = Vec::with_capacity(field_types.len());

                    for (name, ty) in field_types {
                        let value = self.lift_value(ty, flat, cursor, lifted, types)?;
                        fields.push((name.clone(), value));
                    }

                    Ok(ComponentValue::Record(fields))
                }
                ComponentTypeDef::Defined(ComponentDefinedKind::Tuple(tys)) => {
                    let mut values = Vec::with_capacity(tys.len());

                    for ty in tys {
                        values.push(self.lift_value(ty, flat, cursor, lifted, types)?);
                    }

                    Ok(ComponentValue::Tuple(values))
                }
                ComponentTypeDef::Defined(ComponentDefinedKind::Variant(cases)) => {
                    let discriminant = flat[*cursor].as_i32() as usize;
                    *cursor += 1;

                    let max_payload = cases
                        .iter()
                        .map(|c| c.ty.as_ref().map_or(0, |ty| ty.flat_count(types)))
                        .max()
                        .unwrap_or(0);

                    let case = &cases[discriminant];
                    let payload = if let Some(ty) = &case.ty {
                        let val = self.lift_value(ty, flat, cursor, lifted, types)?;
                        let this_count = ty.flat_count(types);

                        *cursor += max_payload - this_count;

                        Some(Box::new(val))
                    } else {
                        *cursor += max_payload;

                        None
                    };

                    Ok(ComponentValue::Variant(case.name.clone(), payload))
                }
                ComponentTypeDef::Defined(ComponentDefinedKind::Enum(cases)) => {
                    let discriminant = flat[*cursor].as_i32() as usize;
                    *cursor += 1;

                    Ok(ComponentValue::Enum(cases[discriminant].clone()))
                }
                ComponentTypeDef::Defined(ComponentDefinedKind::Option(inner_ty)) => {
                    let discriminant = flat[*cursor].as_i32();
                    *cursor += 1;

                    let payload_count = inner_ty.flat_count(types);
                    if discriminant == 0 {
                        *cursor += payload_count;

                        return Ok(ComponentValue::Option(None));
                    }

                    let val = self.lift_value(inner_ty, flat, cursor, lifted, types)?;

                    Ok(ComponentValue::Option(Some(Box::new(val))))
                }
                ComponentTypeDef::Defined(ComponentDefinedKind::Result { ok, err }) => {
                    let discriminant = flat[*cursor].as_i32();
                    *cursor += 1;

                    let ok_count = ok.as_ref().map_or(0, |ty| ty.flat_count(types));
                    let err_count = err.as_ref().map_or(0, |ty| ty.flat_count(types));
                    let max_payload = ok_count.max(err_count);

                    if discriminant == 0 {
                        let val = if let Some(ty) = ok {
                            let v = self.lift_value(ty, flat, cursor, lifted, types)?;
                            *cursor += max_payload - ok_count;

                            Some(Box::new(v))
                        } else {
                            *cursor += max_payload;

                            None
                        };
                        return Ok(ComponentValue::Result(Ok(val)));
                    }

                    let val = if let Some(ty) = err {
                        let v = self.lift_value(ty, flat, cursor, lifted, types)?;
                        *cursor += max_payload - err_count;

                        Some(Box::new(v))
                    } else {
                        *cursor += max_payload;

                        None
                    };

                    Ok(ComponentValue::Result(Err(val)))
                }
                ComponentTypeDef::Defined(ComponentDefinedKind::Flags(all_flags)) => {
                    let num_i32s = all_flags.len().div_ceil(32).max(1);

                    let mut set = Vec::new();

                    for word_i in 0..num_i32s {
                        let bits = flat[*cursor].as_i32() as u32;
                        *cursor += 1;

                        for bit in 0..32 {
                            let flag_i = word_i * 32 + bit;

                            if flag_i < all_flags.len() && (bits >> bit) & 1 != 0 {
                                set.push(all_flags[flag_i].clone());
                            }
                        }
                    }

                    Ok(ComponentValue::Flags(set))
                }
                other => todo!("lift type {:?}", other),
            },
        }
    }

    pub fn resume(&mut self) -> Result<ExecutionState> {
        let arity = self
            .pending_arity
            .ok_or_else(|| Error::Instantiation("no pending execution to resume".into()))?;

        self.finish_run(arity)
    }

    pub fn resume_component(&mut self) -> Result<ExecutionState<ComponentValue>> {
        let (lifted, types) = self
            .pending_lifted
            .clone()
            .ok_or_else(|| Error::Instantiation("no pending component execution".into()))?;

        let result = self.resume()?;

        Ok(match result {
            ExecutionState::Completed(raw_values) => {
                self.pending_lifted = None;
                ExecutionState::Completed(self.lift_values(&raw_values, &lifted, &types)?)
            }
            ExecutionState::FuelExhausted => ExecutionState::FuelExhausted,
            ExecutionState::Suspended {
                module_name,
                func_name,
                args,
            } => ExecutionState::Suspended {
                module_name,
                func_name,
                args,
            },
        })
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

        let Some((module_i, compiled_i)) = self.compiled_func_index(func_addr) else {
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
        let mut locals = self.stack.slice_from(args_start).to_vec();
        self.stack.truncate(args_start);

        let cf = &self.instances[module_i as usize].code.compiled_funcs[compiled_i as usize];
        for _ in &cf.local_types[num_args..] {
            locals.push(RawValue::default());
        }

        let stack_base = self.stack.len();

        self.call_stack.push(CallFrame {
            module_i,
            compiled_func_i: compiled_i,
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

            let mi = self.call_stack[depth].module_i as usize;
            let func_i = self.call_stack[depth].compiled_func_i;
            let pc = self.call_stack[depth].pc;

            let Some(jit_function) = &self.instances[mi].jit_functions[func_i as usize] else {
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

            let mi = self.call_stack[depth].module_i as usize;
            let func_i = self.call_stack[depth].compiled_func_i;
            let pc = self.call_stack[depth].pc;
            assert!(
                (func_i as usize) < self.instances[mi].code.compiled_funcs.len(),
                "compiler error: compiled function index {func_i} oob"
            );

            let func_ops = &self.instances[mi].code.compiled_funcs[func_i as usize].ops;
            assert!(
                pc < func_ops.len(),
                "compiler error: pc {pc} past end of function {func_i} (len {})",
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
                Op::Call { func_i } => {
                    let func_addr = self.instances[mi].function_addrs[func_i as usize];
                    if self.push_function_call(func_addr)? {
                        return Ok(RunOutcome::Suspended);
                    }
                }
                Op::CallIndirect { type_i, table_i } => {
                    let table_addr = self.instances[mi].table_addrs[table_i as usize];
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
                        match &self.instances[mi].code.types[type_i as usize].composite_type {
                            CompositeType::Func(ft) => ft,
                            _ => instantiation_err!("type index {} not a func type", type_i),
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
                Op::ReturnCall { func_i } => {
                    let func_addr = self.instances[mi].function_addrs[func_i as usize];
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
                Op::ReturnCallIndirect { type_i, table_i } => {
                    let table_addr = self.instances[mi].table_addrs[table_i as usize];
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
                        match &self.instances[mi].code.types[type_i as usize].composite_type {
                            CompositeType::Func(ft) => ft,
                            _ => instantiation_err!("type index {} not a func type", type_i),
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
                Op::V128Const { table_i } => {
                    let v = self.instances[mi].code.v128_constants[table_i as usize];
                    self.stack.push_v128(v);
                }
                Op::LocalGet { local_i } => self.do_local_get(local_i as usize, depth),
                Op::LocalSet { local_i } => {
                    let val = self.stack.pop();
                    let locals = &mut self.call_stack[depth].locals;
                    assert!(
                        (local_i as usize) < locals.len(),
                        "compiler error: local index {local_i} oob (func has {} locals)",
                        locals.len()
                    );
                    locals[local_i as usize] = val;
                }
                Op::LocalTee { local_i } => {
                    let val = *self.stack.last();
                    let locals = &mut self.call_stack[depth].locals;
                    assert!(
                        (local_i as usize) < locals.len(),
                        "compiler error: local index {local_i} oob (func has {} locals)",
                        locals.len()
                    );
                    locals[local_i as usize] = val;
                }
                Op::GlobalGet { global_i } => {
                    let addr = self.instances[mi].global_addrs[global_i as usize];
                    self.stack.push(self.globals[addr].value);
                }
                Op::GlobalSet { global_i } => {
                    let addr = self.instances[mi].global_addrs[global_i as usize];
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
                Op::RefFunc { func_i } => {
                    let addr = self.instances[mi].function_addrs[func_i as usize];
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
                Op::TryCatchPush { handler_i } => {
                    self.catch_stack.push(CatchFrame {
                        call_depth: self.call_stack.len(),
                        stack_restore: self.stack.len(),
                        module_i: mi as u16,
                        handler_i,
                    });
                }
                Op::TryCatchPop => {
                    self.catch_stack.pop();
                }
                Op::Throw { tag_i } => {
                    let tag_addr = self.instances[mi].tag_addrs[tag_i as usize];
                    let n_values = self.tags[tag_addr].tag_type.0 .0.len();
                    let values = self.stack.pop_n(n_values).to_vec();
                    self.handle_exception(tag_addr, values)?;
                }
                Op::ThrowRef => {
                    let exn_ref = self.stack.pop().as_ref();
                    match exn_ref {
                        Ref::Null => trap!(Trap::NullReference),
                        Ref::ExnRef(i) => {
                            let exn = self.exceptions[i].clone();
                            self.handle_exception(exn.tag_addr, exn.values)?;
                        }
                        _ => trap!(Trap::NullReference),
                    }
                }
                Op::TableGet { table_i } => {
                    let ta = self.instances[mi].table_addrs[table_i as usize];
                    let addr_type = self.tables[ta].table_type.addr_type;

                    let i = self.stack.pop_address(addr_type);

                    let elem = self.tables[ta]
                        .elem
                        .get(i)
                        .ok_or(Error::Trap(Trap::OutOfBoundsTableAccess))?;

                    self.stack.push(RawValue::from_ref(*elem));
                }
                Op::TableSet { table_i } => {
                    let ta = self.instances[mi].table_addrs[table_i as usize];
                    let r = self.stack.pop().as_ref();

                    let at = self.tables[ta].table_type.addr_type;
                    let i = self.stack.pop_address(at);

                    let elem = self.tables[ta]
                        .elem
                        .get_mut(i)
                        .ok_or(Error::Trap(Trap::OutOfBoundsTableAccess))?;

                    *elem = r;
                }
                Op::TableInit { elem_i, table_i } => {
                    let ta = self.instances[mi].table_addrs[table_i as usize];
                    let ea = self.instances[mi].elem_addrs[elem_i as usize];

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
                Op::ElemDrop { elem_i } => {
                    let ea = self.instances[mi].elem_addrs[elem_i as usize];
                    self.element_segments[ea].elem.clear();
                }
                Op::TableCopy {
                    dst_table_i,
                    src_table_i,
                } => {
                    let dst_a = self.instances[mi].table_addrs[dst_table_i as usize];
                    let src_a = self.instances[mi].table_addrs[src_table_i as usize];

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
                Op::TableGrow { table_i } => {
                    let ta = self.instances[mi].table_addrs[table_i as usize];
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
                Op::TableSize { table_i } => {
                    let ta = self.instances[mi].table_addrs[table_i as usize];
                    let size = self.tables[ta].elem.len();
                    let at = self.tables[ta].table_type.addr_type;

                    self.stack.push_address(size, at);
                }
                Op::TableFill { table_i } => {
                    let ta = self.instances[mi].table_addrs[table_i as usize];
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
                Op::MemorySize { memory_i } => {
                    let ma = self.instances[mi].mem_addrs[memory_i as usize];
                    let mem = &self.memories[ma];
                    let size = mem.data.len() / PAGE_SIZE;

                    self.stack.push_address(size, mem.memory_type.addr_type);
                }
                Op::MemoryGrow { memory_i } => {
                    let ma = self.instances[mi].mem_addrs[memory_i as usize];
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
                Op::MemoryInit { data_i, memory_i } => {
                    let ma = self.instances[mi].mem_addrs[memory_i as usize];
                    let da = self.instances[mi].data_addrs[data_i as usize];

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
                        self.memories[ma].data.write_bytes(d, &src);
                    }
                }
                Op::DataDrop { data_i } => {
                    let da = self.instances[mi].data_addrs[data_i as usize];
                    self.data_segments[da].data.clear();
                }
                Op::MemoryCopy {
                    dst_memory_i,
                    src_memory_i,
                } => {
                    let m1 = self.instances[mi].mem_addrs[dst_memory_i as usize];
                    let m2 = self.instances[mi].mem_addrs[src_memory_i as usize];
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
                            self.memories[m1].data.copy_within(i2, i2 + n, i1);
                        } else {
                            let src = self.memories[m2].data.read_bytes(i2, n).to_vec();
                            self.memories[m1].data.write_bytes(i1, &src);
                        }
                    }
                }
                Op::MemoryFill { memory_i } => {
                    let ma = self.instances[mi].mem_addrs[memory_i as usize];
                    let at = self.memories[ma].memory_type.addr_type;
                    let n = self.stack.pop_address(at);
                    let val = pop_val!(self, I32);
                    let i = self.stack.pop_address(at);

                    if i.saturating_add(n) > self.memories[ma].data.len() {
                        trap!(Trap::OutOfBoundsMemoryAccess);
                    }

                    if n > 0 {
                        self.memories[ma].data.fill(i, n, val as u8);
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
                    local_i_a,
                    local_i_b,
                } => {
                    let locals = &self.call_stack[depth].locals;

                    self.stack.extend_from_slice(&[
                        locals[local_i_a as usize],
                        locals[local_i_b as usize],
                    ]);
                }
                Op::LocalGetReturn { local_i } => {
                    self.do_local_get(local_i as usize, depth);
                    self.do_return(depth);
                }
                Op::LocalGetI32Load {
                    local_i,
                    offset,
                    memory,
                } => local_get_load!(self, depth, mi, local_i, offset, memory, 4, |b| {
                    i32::from_le_bytes(b)
                }),
                Op::LocalGetI64Load {
                    local_i,
                    offset,
                    memory,
                } => local_get_load!(self, depth, mi, local_i, offset, memory, 8, |b| {
                    i64::from_le_bytes(b)
                }),
                Op::LocalGetF32Load {
                    local_i,
                    offset,
                    memory,
                } => local_get_load!(self, depth, mi, local_i, offset, memory, 4, |b| {
                    f32::from_le_bytes(b)
                }),
                Op::LocalGetF64Load {
                    local_i,
                    offset,
                    memory,
                } => local_get_load!(self, depth, mi, local_i, offset, memory, 8, |b| {
                    f64::from_le_bytes(b)
                }),
                Op::LocalGetI32Store {
                    local_i,
                    offset,
                    memory,
                } => local_get_store!(
                    self,
                    depth,
                    mi,
                    local_i,
                    offset,
                    memory,
                    4,
                    |v: RawValue| v.as_i32().to_le_bytes()
                ),
                Op::LocalGetI64Store {
                    local_i,
                    offset,
                    memory,
                } => local_get_store!(
                    self,
                    depth,
                    mi,
                    local_i,
                    offset,
                    memory,
                    8,
                    |v: RawValue| v.as_i64().to_le_bytes()
                ),
                Op::LocalGetF32Store {
                    local_i,
                    offset,
                    memory,
                } => local_get_store!(
                    self,
                    depth,
                    mi,
                    local_i,
                    offset,
                    memory,
                    4,
                    |v: RawValue| v.as_f32().to_le_bytes()
                ),
                Op::LocalGetF64Store {
                    local_i,
                    offset,
                    memory,
                } => local_get_store!(
                    self,
                    depth,
                    mi,
                    local_i,
                    offset,
                    memory,
                    8,
                    |v: RawValue| v.as_f64().to_le_bytes()
                ),
                Op::LocalGetLocalSet {
                    local_get_i,
                    local_set_i,
                } => {
                    let locals = &mut self.call_stack[depth].locals;
                    let out = locals[local_get_i as usize];

                    locals[local_set_i as usize] = out;
                }
                _ => todo!(),
            }
        }
    }

    fn do_local_get(&mut self, local_i: usize, depth: usize) {
        let locals = &self.call_stack[depth].locals;
        debug_assert!(
            local_i < locals.len(),
            "compiler error: local index {local_i} oob (func has {} locals)",
            locals.len()
        );

        self.stack.push(locals[local_i]);
    }

    fn handle_exception(&mut self, tag_addr: usize, values: Vec<RawValue>) -> Result<()> {
        while let Some(frame) = self.catch_stack.last() {
            let mi = frame.module_i as usize;
            let handler_i = frame.handler_i as usize;
            let call_depth = frame.call_depth;
            let stack_restore = frame.stack_restore;

            let clauses = self.instances[mi].code.catch_handlers[handler_i].clone();

            for clause in &clauses {
                let matches = match clause.kind {
                    CatchKind::Catch | CatchKind::CatchRef => {
                        let clause_tag_addr = self.instances[mi].tag_addrs[clause.tag_i as usize];
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
                            let exn_i = self.exceptions.len();
                            self.exceptions.push(Exception { tag_addr, values });
                            self.stack.push(RawValue::from_ref(Ref::ExnRef(exn_i)));
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
            self.call_stack[depth].compiled_func_i,
            self.call_stack[depth].module_i,
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

    fn run_init_instructions(&mut self, instructions: &[Instruction], module_i: u16) -> Result<()> {
        let mut ops = Vec::with_capacity(instructions.len() + 1);
        for instr in instructions {
            match instr {
                Instruction::I32Const(v) => ops.push(Op::I32Const { value: *v }),
                Instruction::I64Const(v) => ops.push(Op::I64Const { value: *v }),
                Instruction::MemoryInit(data_i, mem) => ops.push(Op::MemoryInit {
                    data_i: *data_i,
                    memory_i: *mem,
                }),
                Instruction::DataDrop(i) => ops.push(Op::DataDrop { data_i: *i }),
                Instruction::TableInit(table_i, elem_i) => ops.push(Op::TableInit {
                    table_i: *table_i,
                    elem_i: *elem_i,
                }),
                Instruction::ElemDrop(i) => ops.push(Op::ElemDrop { elem_i: *i }),
                Instruction::RefNull(ht) => ops.push(Op::RefNull(*ht)),
                Instruction::RefFunc(i) => ops.push(Op::RefFunc { func_i: *i }),
                Instruction::GlobalGet(i) => ops.push(Op::GlobalGet { global_i: *i }),
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
            source_positions: Vec::new(),
            ops,
            type_index: 0,
            num_args: 0,
            local_types: Vec::new(),
            max_stack_height,
        };

        let code = Arc::make_mut(&mut self.instances[module_i as usize].code);
        let compiled_func_i = code.compiled_funcs.len();
        code.compiled_funcs.push(cf);

        self.call_stack.push(CallFrame {
            module_i,
            compiled_func_i: compiled_func_i as u32,
            pc: 0,
            locals: Vec::new(),
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

fn resolve_defined_type<'a>(
    param_ty: Option<&ComponentValueKind>,
    types: &'a [ComponentTypeDef],
) -> Option<&'a ComponentDefinedKind> {
    match param_ty? {
        ComponentValueKind::Type(i) => match &types[*i as usize] {
            ComponentTypeDef::Defined(d) => Some(d),
            _ => None,
        },
        _ => None,
    }
}

fn component_value_byte_size(v: &ComponentValue) -> usize {
    match v {
        ComponentValue::Bool(_) | ComponentValue::S8(_) | ComponentValue::U8(_) => 1,
        ComponentValue::S16(_) | ComponentValue::U16(_) => 2,
        ComponentValue::S32(_) | ComponentValue::U32(_) | ComponentValue::F32(_) => 4,
        ComponentValue::S64(_) | ComponentValue::U64(_) | ComponentValue::F64(_) => 8,
        _ => todo!("byte size for {:?}", v),
    }
}

fn primitive_byte_size(p: &PrimitiveValueKind) -> usize {
    match p {
        PrimitiveValueKind::Bool | PrimitiveValueKind::S8 | PrimitiveValueKind::U8 => 1,
        PrimitiveValueKind::S16 | PrimitiveValueKind::U16 => 2,
        PrimitiveValueKind::S32 | PrimitiveValueKind::U32 | PrimitiveValueKind::F32 => 4,
        PrimitiveValueKind::S64 | PrimitiveValueKind::U64 | PrimitiveValueKind::F64 => 8,
        _ => todo!(),
    }
}

fn run_data(index: u32, data_segment: &DataSegment) -> Vec<Instruction> {
    match &data_segment.mode {
        DataMode::Passive => Vec::new(),
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
        ElementMode::Passive => Vec::new(),
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
            Instruction::RefFunc(i) => {
                let addr = *address_map
                    .function_addrs
                    .get(*i as usize)
                    .ok_or_else(|| Error::Instantiation(format!("ref.func index {} oob", i)))?;
                stack.push(RawValue::from_ref(Ref::FunctionAddr(addr)));
            }
            Instruction::GlobalGet(i) => {
                let store_i = *address_map.global_addrs.get(*i as usize).ok_or_else(|| {
                    Error::Instantiation(format!("global index {} oob in const expr", i))
                })?;
                let global = store.globals.get(store_i).ok_or_else(|| {
                    Error::Instantiation(format!(
                        "global store index {} oob in const expr",
                        store_i
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
    pub fn to_bytes(&self) -> Vec<u8> {
        self.encode(true)
    }

    pub fn to_blueprint(&self) -> Vec<u8> {
        self.encode(false)
    }

    pub fn from_blueprint_with_memories(bytes: &[u8], memories: Vec<MemoryInstance>) -> Self {
        Self::decode(bytes, Some(memories))
    }

    fn encode(&self, include_memory_data: bool) -> Vec<u8> {
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
            if include_memory_data {
                buf.extend_from_slice(mem.data.as_slice());
            }
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
        self.pending_lifted.encode(&mut buf);

        // component instances
        (self.component_instances.len() as u32).encode(&mut buf);
        for ci in &self.component_instances {
            ci.encode(&mut buf);
        }

        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self::decode(bytes, None)
    }

    fn decode(bytes: &[u8], provided_memories: Option<Vec<MemoryInstance>>) -> Self {
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
            locals: Vec::new(),
            body: Vec::new(),
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

        let num_memories = u32::decode(buf) as usize;
        let provided_was_some = provided_memories.is_some();
        let memories = if let Some(provided) = provided_memories {
            assert_eq!(provided.len(), num_memories);

            for _ in 0..num_memories {
                let _ = MemoryType::decode(buf);
                let _ = u64::decode(buf) as usize;
            }

            provided
        } else {
            let mut memories = Vec::with_capacity(num_memories);

            for _ in 0..num_memories {
                let memory_type = MemoryType::decode(buf);
                let data_len = u64::decode(buf) as usize;

                let data = buf[..data_len].to_vec();
                *buf = &buf[data_len..];

                memories.push(MemoryInstance {
                    memory_type,
                    data: GuestMemory::from_vec(data),
                });
            }

            memories
        };

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

        // fuel + pending_arity + pending_lifted
        let fuel = Option::decode(buf);
        let pending_arity = Option::decode(buf);
        let pending_lifted = Option::decode(buf);

        // component instances
        let num_component_instances = u32::decode(buf) as usize;
        let mut component_instances = Vec::with_capacity(num_component_instances);
        for _ in 0..num_component_instances {
            component_instances.push(InstantiatedComponent::decode(buf));
        }

        let mmap_backing = provided_was_some;

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
            pending_lifted,
            component_instances,
            mmap_backing,
        }
    }
}

#[cfg(unix)]
pub struct StoreSnapshot {
    blueprint: Vec<u8>,
    memories: Vec<MemoryInstance>,
}

#[cfg(unix)]
impl Store {
    pub fn snapshot(self) -> StoreSnapshot {
        assert!(self.memories.iter().all(|m| m.data.is_mmap()));

        let blueprint = self.to_blueprint();

        StoreSnapshot {
            blueprint,
            memories: self.memories,
        }
    }
}

#[cfg(unix)]
impl StoreSnapshot {
    pub const fn memory_count(&self) -> usize {
        self.memories.len()
    }

    pub fn fork(&self, n: usize) -> std::io::Result<Vec<Store>> {
        use std::io;

        if n == 0 {
            return Ok(Vec::new());
        }

        let forked_per_memory = self
            .memories
            .iter()
            .map(|m| m.data.fork_private(n))
            .collect::<io::Result<Vec<_>>>()?;

        let mut child_iters = forked_per_memory
            .into_iter()
            .map(IntoIterator::into_iter)
            .collect::<Vec<_>>();

        let mut stores = Vec::with_capacity(n);

        for _ in 0..n {
            let child_memories = self
                .memories
                .iter()
                .zip(child_iters.iter_mut())
                .map(|(parent_mem, iter)| MemoryInstance {
                    memory_type: parent_mem.memory_type.clone(),
                    data: iter
                        .next()
                        .expect("fork_private returned fewer children than requested"),
                })
                .collect::<Vec<_>>();

            stores.push(Store::from_blueprint_with_memories(
                &self.blueprint,
                child_memories,
            ));
        }

        Ok(stores)
    }
}
