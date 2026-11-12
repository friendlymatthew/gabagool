use core::ffi::c_void;

use crate::{
    module::AddrType,
    runtime::{
        DataInstance, ElementInstance, GlobalInstance, InstantiatedModule, MemoryInstance,
        TableInstance,
    },
    Error, Result, Trap,
};

// C sees only this view, never Rust's Vec or MemoryInstance layout.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct MemoryView {
    pub bytes: *mut u8,
    pub length: u64,
    pub address64: u64,
}

impl MemoryView {
    pub const EMPTY: Self = Self {
        bytes: core::ptr::null_mut(),
        length: 0,
        address64: 0,
    };
}

// borrow the actual runtime resources for one native execution. Nothing is
// copied into the code buffer, so segment drops and writes remain visible to
// snapshots.
pub struct RuntimeContext<'a> {
    pub module: &'a InstantiatedModule,
    pub memories: &'a mut [MemoryInstance],
    pub globals: &'a mut [GlobalInstance],
    pub tables: &'a mut [TableInstance],
    pub data_segments: &'a mut [DataInstance],
    pub element_segments: &'a mut [ElementInstance],
}

const fn address(value: u64, address_type: AddrType) -> usize {
    match address_type {
        AddrType::I32 => value as u32 as usize,
        AddrType::I64 => value as usize,
    }
}

impl RuntimeContext<'_> {
    pub(super) fn memory_view(&mut self, index: usize) -> MemoryView {
        let memory = &mut self.memories[self.module.mem_addrs[index]];

        MemoryView {
            bytes: memory.data.as_mut_ptr(),
            length: memory.data.len() as u64,
            address64: u64::from(matches!(memory.memory_type.addr_type, AddrType::I64)),
        }
    }

    pub fn global_values(&mut self, output: &mut [*mut u64]) {
        let globals = self.globals.as_mut_ptr();

        for (slot, &address) in output.iter_mut().zip(&self.module.global_addrs) {
            *slot = unsafe { core::ptr::addr_of_mut!((*globals.add(address)).value).cast() };
        }
    }

    fn bulk_operation(
        &mut self,
        operation: u32,
        segment: u32,
        destination: u32,
        offset: u64,
        source: u64,
        length: u64,
    ) -> Result<()> {
        match operation {
            0 => {
                let source = source as usize;
                let length = length as usize;
                let memory = &mut self.memories[self.module.mem_addrs[destination as usize]];
                let data = &self.data_segments[self.module.data_addrs[segment as usize]].data;
                let offset = address(offset, memory.memory_type.addr_type);
                let bytes = data
                    .get(source..source.saturating_add(length))
                    .ok_or(Trap::OutOfBoundsMemoryAccess)?;

                memory.data.write_bytes(offset, bytes)?;
            }
            1 => {
                self.data_segments[self.module.data_addrs[segment as usize]]
                    .data
                    .clear();
            }
            2 => {
                let source = source as usize;
                let length = length as usize;
                let table = &mut self.tables[self.module.table_addrs[destination as usize]];
                let elements =
                    &self.element_segments[self.module.elem_addrs[segment as usize]].elem;
                let offset = address(offset, table.table_type.addr_type);
                let elements = elements
                    .get(source..source.saturating_add(length))
                    .ok_or(Trap::OutOfBoundsTableAccess)?;
                let target = table
                    .elem
                    .get_mut(offset..offset.saturating_add(length))
                    .ok_or(Trap::OutOfBoundsTableAccess)?;

                target.copy_from_slice(elements);
            }
            3 => {
                self.element_segments[self.module.elem_addrs[segment as usize]]
                    .elem
                    .clear();
            }
            4 => {
                let src = self.module.mem_addrs[segment as usize];
                let dst = self.module.mem_addrs[destination as usize];
                let src_type = self.memories[src].memory_type.addr_type;
                let dst_type = self.memories[dst].memory_type.addr_type;
                let length_type = if src_type == AddrType::I64 && dst_type == AddrType::I64 {
                    AddrType::I64
                } else {
                    AddrType::I32
                };
                let source = address(source, src_type);
                let offset = address(offset, dst_type);
                let length = address(length, length_type);

                if src == dst {
                    self.memories[dst]
                        .data
                        .copy_within(source, offset, length)?;
                } else {
                    let (src_memory, dst_memory) = if src < dst {
                        let (lower, upper) = self.memories.split_at_mut(dst);
                        (&lower[src].data, &mut upper[0].data)
                    } else {
                        let (lower, upper) = self.memories.split_at_mut(src);
                        (&upper[0].data, &mut lower[dst].data)
                    };
                    let bytes = src_memory.read_bytes(source, length)?;

                    dst_memory.write_bytes(offset, bytes)?;
                }
            }
            5 => {
                let memory = &mut self.memories[self.module.mem_addrs[destination as usize]];
                let address_type = memory.memory_type.addr_type;

                memory.data.fill(
                    address(offset, address_type),
                    address(length, address_type),
                    source as u8,
                )?;
            }
            _ => return Err(Error::jit("unknown bulk operation")),
        }

        Ok(())
    }
}

pub type BulkOperation = unsafe extern "C" fn(*mut c_void, u32, u32, u32, u64, u64, u64) -> u64;

pub unsafe extern "C" fn bulk_operation(
    runtime: *mut c_void,
    operation: u32,
    segment: u32,
    destination: u32,
    offset: u64,
    source: u64,
    length: u64,
) -> u64 {
    if runtime.is_null() {
        return 255;
    }

    let runtime = unsafe { &mut *runtime.cast::<RuntimeContext<'_>>() };

    match runtime.bulk_operation(operation, segment, destination, offset, source, length) {
        Ok(()) => 0,
        Err(Error::Trap(Trap::OutOfBoundsMemoryAccess)) => 4,
        Err(Error::Trap(Trap::OutOfBoundsTableAccess)) => 5,
        Err(_) => 255,
    }
}
