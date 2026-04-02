use std::collections::HashMap;
use std::sync::Arc;
use std::{mem, slice};

use crate::binary_grammar::{
    AddrType, ArrayType, CompositeType, FieldType, FunctionType, GlobalType, HeapType, Limit,
    MemoryType, Mutability, RefType, ResultType, StorageType, StructType, SubType, TableType,
    ValueType,
};
use crate::compiler::ModuleCode;
use crate::component::binary_grammar::{ComponentValueKind, PrimitiveValueKind, StringEncoding};
use crate::component::execution_grammar::{InstantiatedComponent, LiftedFunc};
use crate::execution_grammar::{ExportInstance, ExternalValue, RawValue, Ref};
use crate::ir::{CatchKind, CompiledCatchClause, CompiledFunction, JumpTableEntry, Op};
use crate::store::{CallFrame, InstantiatedModule};

pub const SNAPSHOT_MAGIC: &[u8; 4] = b"gaba";
pub const SNAPSHOT_VERSION: u32 = 1;

pub trait Snapshot: Sized {
    fn encode(&self, buf: &mut Vec<u8>);
    fn decode(buf: &mut &[u8]) -> Self;
}

pub fn encode_bulk<T: Copy>(slice: &[T], buf: &mut Vec<u8>) {
    (slice.len() as u32).encode(buf);
    let byte_len = mem::size_of_val(slice);
    let ptr = slice.as_ptr() as *const u8;
    buf.extend_from_slice(unsafe { slice::from_raw_parts(ptr, byte_len) });
}

pub fn decode_bulk<T: Copy>(buf: &mut &[u8]) -> Vec<T> {
    let len = u32::decode(buf) as usize;
    let byte_len = len * mem::size_of::<T>();
    let mut vec = Vec::with_capacity(len);
    unsafe {
        std::ptr::copy_nonoverlapping(buf.as_ptr(), vec.as_mut_ptr() as *mut u8, byte_len);
        vec.set_len(len);
    }
    *buf = &buf[byte_len..];
    vec
}

impl Snapshot for bool {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.push(*self as u8);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let v = buf[0] != 0;
        *buf = &buf[1..];
        v
    }
}

impl Snapshot for u8 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.push(*self);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let v = buf[0];
        *buf = &buf[1..];
        v
    }
}

impl Snapshot for u16 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let v = Self::from_le_bytes(buf[..2].try_into().unwrap());
        *buf = &buf[2..];
        v
    }
}

impl Snapshot for u32 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let v = Self::from_le_bytes(buf[..4].try_into().unwrap());
        *buf = &buf[4..];
        v
    }
}

impl Snapshot for u64 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let v = Self::from_le_bytes(buf[..8].try_into().unwrap());
        *buf = &buf[8..];
        v
    }
}

impl Snapshot for i32 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let v = Self::from_le_bytes(buf[..4].try_into().unwrap());
        *buf = &buf[4..];
        v
    }
}

impl Snapshot for i64 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let v = Self::from_le_bytes(buf[..8].try_into().unwrap());
        *buf = &buf[8..];
        v
    }
}

impl Snapshot for i128 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let v = Self::from_le_bytes(buf[..16].try_into().unwrap());
        *buf = &buf[16..];
        v
    }
}

impl Snapshot for f32 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let v = Self::from_le_bytes(buf[..4].try_into().unwrap());
        *buf = &buf[4..];
        v
    }
}

impl Snapshot for f64 {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.to_le_bytes());
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let v = Self::from_le_bytes(buf[..8].try_into().unwrap());
        *buf = &buf[8..];
        v
    }
}

impl Snapshot for usize {
    fn encode(&self, buf: &mut Vec<u8>) {
        (*self as u64).encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        u64::decode(buf) as Self
    }
}

impl Snapshot for String {
    fn encode(&self, buf: &mut Vec<u8>) {
        (self.len() as u32).encode(buf);
        buf.extend_from_slice(self.as_bytes());
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let len = u32::decode(buf) as usize;
        let s = std::str::from_utf8(&buf[..len]).unwrap().to_owned();
        *buf = &buf[len..];
        s
    }
}

impl<T: Snapshot> Snapshot for Vec<T> {
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_slice(self, buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let len = u32::decode(buf) as usize;
        (0..len).map(|_| T::decode(buf)).collect()
    }
}

pub fn encode_slice<T: Snapshot>(slice: &[T], buf: &mut Vec<u8>) {
    (slice.len() as u32).encode(buf);
    for item in slice {
        item.encode(buf);
    }
}

impl<K: Snapshot + Eq + std::hash::Hash, V: Snapshot> Snapshot for HashMap<K, V> {
    fn encode(&self, buf: &mut Vec<u8>) {
        (self.len() as u32).encode(buf);

        for (k, v) in self {
            k.encode(buf);
            v.encode(buf);
        }
    }

    fn decode(buf: &mut &[u8]) -> Self {
        let len = u32::decode(buf) as usize;
        let mut map = Self::with_capacity(len);

        for _ in 0..len {
            map.insert(K::decode(buf), V::decode(buf));
        }

        map
    }
}

impl<T: Snapshot> Snapshot for Option<T> {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            None => 0u8.encode(buf),
            Some(v) => {
                1u8.encode(buf);
                v.encode(buf);
            }
        }
    }
    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => None,
            _ => Some(T::decode(buf)),
        }
    }
}

impl<A: Snapshot, B: Snapshot> Snapshot for (A, B) {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.0.encode(buf);
        self.1.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        (A::decode(buf), B::decode(buf))
    }
}

impl Snapshot for HeapType {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Self::Func => 0u8.encode(buf),
            Self::Extern => 1u8.encode(buf),
            Self::Any => 2u8.encode(buf),
            Self::Eq => 3u8.encode(buf),
            Self::I31 => 4u8.encode(buf),
            Self::Struct => 5u8.encode(buf),
            Self::Array => 6u8.encode(buf),
            Self::Exn => 7u8.encode(buf),
            Self::None => 8u8.encode(buf),
            Self::NoExtern => 9u8.encode(buf),
            Self::NoFunc => 10u8.encode(buf),
            Self::NoExn => 11u8.encode(buf),
            Self::TypeIndex(i) => {
                12u8.encode(buf);
                i.encode(buf);
            }
        }
    }
    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => Self::Func,
            1 => Self::Extern,
            2 => Self::Any,
            3 => Self::Eq,
            4 => Self::I31,
            5 => Self::Struct,
            6 => Self::Array,
            7 => Self::Exn,
            8 => Self::None,
            9 => Self::NoExtern,
            10 => Self::NoFunc,
            11 => Self::NoExn,
            12 => Self::TypeIndex(u32::decode(buf)),
            d => panic!("invalid HeapType discriminant: {d}"),
        }
    }
}

impl Snapshot for RefType {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Self::FuncRef => 0u8.encode(buf),
            Self::ExternRef => 1u8.encode(buf),
            Self::Ref {
                nullable,
                heap_type,
            } => {
                2u8.encode(buf);
                nullable.encode(buf);
                heap_type.encode(buf);
            }
        }
    }
    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => Self::FuncRef,
            1 => Self::ExternRef,
            2 => Self::Ref {
                nullable: bool::decode(buf),
                heap_type: HeapType::decode(buf),
            },
            d => panic!("invalid RefType discriminant: {d}"),
        }
    }
}

impl Snapshot for ValueType {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Self::I32 => 0u8.encode(buf),
            Self::I64 => 1u8.encode(buf),
            Self::F32 => 2u8.encode(buf),
            Self::F64 => 3u8.encode(buf),
            Self::V128 => 4u8.encode(buf),
            Self::Ref(rt) => {
                5u8.encode(buf);
                rt.encode(buf);
            }
        }
    }
    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => Self::I32,
            1 => Self::I64,
            2 => Self::F32,
            3 => Self::F64,
            4 => Self::V128,
            5 => Self::Ref(RefType::decode(buf)),
            d => panic!("invalid ValueType discriminant: {d}"),
        }
    }
}

impl Snapshot for Mutability {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Self::Const => 0u8.encode(buf),
            Self::Var => 1u8.encode(buf),
        }
    }
    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => Self::Const,
            _ => Self::Var,
        }
    }
}

impl Snapshot for AddrType {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Self::I32 => 0u8.encode(buf),
            Self::I64 => 1u8.encode(buf),
        }
    }
    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => Self::I32,
            _ => Self::I64,
        }
    }
}

impl Snapshot for Limit {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.min.encode(buf);
        self.max.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            min: u64::decode(buf),
            max: u64::decode(buf),
        }
    }
}

impl Snapshot for MemoryType {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.addr_type.encode(buf);
        self.limit.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            addr_type: AddrType::decode(buf),
            limit: Limit::decode(buf),
        }
    }
}

impl Snapshot for TableType {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.element_reference_type.encode(buf);
        self.addr_type.encode(buf);
        self.limit.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            element_reference_type: RefType::decode(buf),
            addr_type: AddrType::decode(buf),
            limit: Limit::decode(buf),
        }
    }
}

impl Snapshot for GlobalType {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.value_type.encode(buf);
        self.mutability.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            value_type: ValueType::decode(buf),
            mutability: Mutability::decode(buf),
        }
    }
}

impl Snapshot for ResultType {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.0.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self(Vec::<ValueType>::decode(buf))
    }
}

impl Snapshot for FunctionType {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.0.encode(buf);
        self.1.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self(ResultType::decode(buf), ResultType::decode(buf))
    }
}

impl Snapshot for StorageType {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Self::Val(vt) => {
                0u8.encode(buf);
                vt.encode(buf);
            }
            Self::I8 => 1u8.encode(buf),
            Self::I16 => 2u8.encode(buf),
        }
    }
    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => Self::Val(ValueType::decode(buf)),
            1 => Self::I8,
            2 => Self::I16,
            d => panic!("invalid StorageType discriminant: {d}"),
        }
    }
}

impl Snapshot for FieldType {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.storage_type.encode(buf);
        self.mutability.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            storage_type: StorageType::decode(buf),
            mutability: Mutability::decode(buf),
        }
    }
}

impl Snapshot for StructType {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.fields.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            fields: Vec::<FieldType>::decode(buf),
        }
    }
}

impl Snapshot for ArrayType {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.field_type.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            field_type: FieldType::decode(buf),
        }
    }
}

impl Snapshot for CompositeType {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Self::Func(ft) => {
                0u8.encode(buf);
                ft.encode(buf);
            }
            Self::Struct(st) => {
                1u8.encode(buf);
                st.encode(buf);
            }
            Self::Array(at) => {
                2u8.encode(buf);
                at.encode(buf);
            }
        }
    }
    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => Self::Func(FunctionType::decode(buf)),
            1 => Self::Struct(StructType::decode(buf)),
            2 => Self::Array(ArrayType::decode(buf)),
            d => panic!("invalid CompositeType discriminant: {d}"),
        }
    }
}

impl Snapshot for SubType {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.is_final.encode(buf);
        self.supertypes.encode(buf);
        self.composite_type.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            is_final: bool::decode(buf),
            supertypes: Vec::<u32>::decode(buf),
            composite_type: CompositeType::decode(buf),
        }
    }
}

impl Snapshot for RawValue {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.as_i64().encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        i64::decode(buf).into()
    }
}

impl Snapshot for Ref {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Self::Null => 0u8.encode(buf),
            Self::FunctionAddr(a) => {
                1u8.encode(buf);
                a.encode(buf);
            }
            Self::RefExtern(a) => {
                2u8.encode(buf);
                a.encode(buf);
            }
            Self::I31(v) => {
                3u8.encode(buf);
                v.encode(buf);
            }
            Self::ExnRef(a) => {
                4u8.encode(buf);
                a.encode(buf);
            }
        }
    }
    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => Self::Null,
            1 => Self::FunctionAddr(usize::decode(buf)),
            2 => Self::RefExtern(usize::decode(buf)),
            3 => Self::I31(i32::decode(buf)),
            4 => Self::ExnRef(usize::decode(buf)),
            d => panic!("invalid Ref discriminant: {d}"),
        }
    }
}

impl Snapshot for ExternalValue {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Self::Function { addr } => {
                0u8.encode(buf);
                addr.encode(buf);
            }
            Self::Table { addr } => {
                1u8.encode(buf);
                addr.encode(buf);
            }
            Self::Memory { addr } => {
                2u8.encode(buf);
                addr.encode(buf);
            }
            Self::Global { addr } => {
                3u8.encode(buf);
                addr.encode(buf);
            }
            Self::Tag { addr } => {
                4u8.encode(buf);
                addr.encode(buf);
            }
        }
    }
    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => Self::Function {
                addr: usize::decode(buf),
            },
            1 => Self::Table {
                addr: usize::decode(buf),
            },
            2 => Self::Memory {
                addr: usize::decode(buf),
            },
            3 => Self::Global {
                addr: usize::decode(buf),
            },
            4 => Self::Tag {
                addr: usize::decode(buf),
            },
            d => panic!("invalid ExternalValue discriminant: {d}"),
        }
    }
}

impl Snapshot for ExportInstance {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.name.encode(buf);
        self.value.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            name: String::decode(buf),
            value: ExternalValue::decode(buf),
        }
    }
}

impl Snapshot for JumpTableEntry {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.target.encode(buf);
        self.drop.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            target: u32::decode(buf),
            drop: u16::decode(buf),
        }
    }
}

impl Snapshot for CompiledFunction {
    fn encode(&self, buf: &mut Vec<u8>) {
        encode_bulk(&self.ops, buf);
        self.type_index.encode(buf);
        self.num_args.encode(buf);
        self.local_types.encode(buf);
        self.max_stack_height.encode(buf);
        self.source_positions.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let ops = decode_bulk::<Op>(buf);
        Self {
            ops,
            type_index: u32::decode(buf),
            num_args: u32::decode(buf),
            local_types: Vec::<ValueType>::decode(buf),
            max_stack_height: u32::decode(buf),
            source_positions: Vec::<u32>::decode(buf),
        }
    }
}

impl Snapshot for [u8; 16] {
    fn encode(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let arr: [u8; 16] = buf[..16].try_into().unwrap();
        *buf = &buf[16..];
        arr
    }
}

impl Snapshot for ModuleCode {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.compiled_funcs.encode(buf);
        self.types.encode(buf);
        self.v128_constants.encode(buf);
        (self.jump_tables.len() as u32).encode(buf);
        for table in &self.jump_tables {
            table.encode(buf);
        }
        self.shuffle_masks.encode(buf);
        (self.catch_handlers.len() as u32).encode(buf);
        for handler in &self.catch_handlers {
            (handler.len() as u32).encode(buf);
            for clause in handler {
                (clause.kind as u8).encode(buf);
                clause.tag_i.encode(buf);
                clause.target.encode(buf);
                clause.n_values.encode(buf);
                clause.drop.encode(buf);
            }
        }
    }
    fn decode(buf: &mut &[u8]) -> Self {
        let compiled_funcs = Vec::<CompiledFunction>::decode(buf);
        let types = Vec::<SubType>::decode(buf);
        let v128_constants = Vec::<i128>::decode(buf);
        let num_tables = u32::decode(buf) as usize;
        let jump_tables = (0..num_tables)
            .map(|_| Vec::<JumpTableEntry>::decode(buf))
            .collect();
        let shuffle_masks = Vec::<[u8; 16]>::decode(buf);
        let num_handlers = u32::decode(buf) as usize;
        let catch_handlers = (0..num_handlers)
            .map(|_| {
                let num_clauses = u32::decode(buf) as usize;
                (0..num_clauses)
                    .map(|_| {
                        let kind = match u8::decode(buf) {
                            0 => CatchKind::Catch,
                            1 => CatchKind::CatchRef,
                            2 => CatchKind::CatchAll,
                            3 => CatchKind::CatchAllRef,
                            d => panic!("invalid CatchKind discriminant: {d}"),
                        };
                        CompiledCatchClause {
                            kind,
                            tag_i: u32::decode(buf),
                            target: u32::decode(buf),
                            n_values: u16::decode(buf),
                            drop: u16::decode(buf),
                        }
                    })
                    .collect()
            })
            .collect();
        Self {
            compiled_funcs,
            types,
            v128_constants,
            jump_tables,
            shuffle_masks,
            catch_handlers,
        }
    }
}

impl Snapshot for CallFrame {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.module_i.encode(buf);
        self.compiled_func_i.encode(buf);
        self.pc.encode(buf);
        encode_bulk(&self.locals, buf);
        self.stack_base.encode(buf);
        self.arity.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            module_i: u16::decode(buf),
            compiled_func_i: u32::decode(buf),
            pc: usize::decode(buf),
            locals: decode_bulk::<RawValue>(buf),
            stack_base: usize::decode(buf),
            arity: usize::decode(buf),
        }
    }
}

impl Snapshot for InstantiatedModule {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.code.as_ref().encode(buf);
        self.function_addrs.encode(buf);
        self.table_addrs.encode(buf);
        self.mem_addrs.encode(buf);
        self.global_addrs.encode(buf);
        self.tag_addrs.encode(buf);
        self.elem_addrs.encode(buf);
        self.data_addrs.encode(buf);
        self.exports.encode(buf);
    }
    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            code: Arc::new(ModuleCode::decode(buf)),
            function_addrs: Vec::<usize>::decode(buf),
            table_addrs: Vec::<usize>::decode(buf),
            mem_addrs: Vec::<usize>::decode(buf),
            global_addrs: Vec::<usize>::decode(buf),
            tag_addrs: Vec::<usize>::decode(buf),
            elem_addrs: Vec::<usize>::decode(buf),
            data_addrs: Vec::<usize>::decode(buf),
            exports: Vec::<ExportInstance>::decode(buf),
            // todo: figure out this
            #[cfg(feature = "jit")]
            jit_functions: Vec::new(),
        }
    }
}

impl Snapshot for PrimitiveValueKind {
    fn encode(&self, buf: &mut Vec<u8>) {
        (*self as u8).encode(buf);
    }

    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => Self::Bool,
            1 => Self::S8,
            2 => Self::U8,
            3 => Self::S16,
            4 => Self::U16,
            5 => Self::S32,
            6 => Self::U32,
            7 => Self::S64,
            8 => Self::U64,
            9 => Self::F32,
            10 => Self::F64,
            11 => Self::Char,
            12 => Self::String,
            n => panic!("invalid PrimitiveValueKind tag: {n}"),
        }
    }
}

impl Snapshot for ComponentValueKind {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Self::Type(i) => {
                0u8.encode(buf);
                i.encode(buf);
            }
            Self::Primitive(p) => {
                1u8.encode(buf);
                p.encode(buf);
            }
        }
    }

    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => Self::Type(u32::decode(buf)),
            1 => Self::Primitive(PrimitiveValueKind::decode(buf)),
            n => panic!("invalid ComponentValueKind tag: {n}"),
        }
    }
}

impl Snapshot for StringEncoding {
    fn encode(&self, buf: &mut Vec<u8>) {
        (*self as u8).encode(buf);
    }

    fn decode(buf: &mut &[u8]) -> Self {
        match u8::decode(buf) {
            0 => Self::Utf8,
            1 => Self::Utf16,
            2 => Self::Latin1Utf16,
            n => panic!("invalid StringEncoding tag: {n}"),
        }
    }
}

impl Snapshot for LiftedFunc {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.func_addr.encode(buf);
        self.memory_addr.encode(buf);
        self.realloc_addr.encode(buf);
        self.string_encoding.encode(buf);
        self.result_types.encode(buf);
    }

    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            func_addr: usize::decode(buf),
            memory_addr: Option::decode(buf),
            realloc_addr: Option::decode(buf),
            string_encoding: StringEncoding::decode(buf),
            result_types: Vec::decode(buf),
        }
    }
}

impl Snapshot for InstantiatedComponent {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.exports.encode(buf);
        self.may_leave.encode(buf);
    }

    fn decode(buf: &mut &[u8]) -> Self {
        Self {
            exports: HashMap::decode(buf),
            may_leave: bool::decode(buf),
        }
    }
}
