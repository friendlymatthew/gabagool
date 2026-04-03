use std::{collections::HashSet, num::NonZeroU64};

use crate::{
    exponential_decay::{Entry, ExponentialDecayBuffer},
    ExecutionState, GlobalType, Instance, RawValue, Result, Store, Trap, ValueType,
};

pub struct FrameInfo {
    pub module_i: u16,
    pub compiled_func_i: u32,
    pub pc: usize,
    pub locals: Vec<RawValue>,
    pub local_types: Vec<ValueType>,
    /// Pre-order instruction index from the original wasm binary.
    /// Use this to map back to .wat line numbers.
    pub source_position: u32,
}

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct Breakpoint {
    module_i: u16,
    compiled_func_i: u32,
    pc: usize,
}

impl Breakpoint {
    pub const fn new(module_i: u16, compiled_func_i: u32, pc: usize) -> Self {
        Self {
            module_i,
            compiled_func_i,
            pc,
        }
    }
}

#[derive(Debug)]
pub enum StepResult {
    Stepped,
    BreakpointHit,
    ReachedStart,
    Trap(Trap),
    Completed,
}

#[derive(Debug)]
pub struct DebuggerConfig {
    decay_buffer_capacity: usize,
    half_life: f32,
    instructions_between_snapshots: u64,
}

impl Default for DebuggerConfig {
    fn default() -> Self {
        Self {
            decay_buffer_capacity: 1_000,
            half_life: 50.0,
            instructions_between_snapshots: 1_000,
        }
    }
}

#[derive(Debug)]
pub struct Debugger {
    store: Store,
    instance: Instance,
    history: ExponentialDecayBuffer<Vec<u8>>,

    instruction_count: u64,
    instructions_between_snapshots: u64,

    breakpoints: HashSet<Breakpoint>,
    completed: Option<Vec<RawValue>>,
}

impl Debugger {
    pub fn new(store: Store, instance: Instance) -> Self {
        Self::new_with_config(store, instance, DebuggerConfig::default())
    }

    pub fn new_with_config(store: Store, instance: Instance, config: DebuggerConfig) -> Self {
        let DebuggerConfig {
            decay_buffer_capacity,
            half_life,
            instructions_between_snapshots,
        } = config;

        Self {
            store,
            instance,
            history: ExponentialDecayBuffer::new(
                decay_buffer_capacity,
                half_life,
                NonZeroU64::new(0xBADA_B100).unwrap(),
            ),
            instruction_count: 0,
            instructions_between_snapshots,
            breakpoints: HashSet::new(),
            completed: None,
        }
    }

    pub fn start(&mut self, func_name: &str, args: Vec<RawValue>) -> Result<()> {
        self.store.set_fuel(0);
        self.store.invoke(self.instance, func_name, args)?;

        let snapshot = self.store.snapshot();
        self.history.push(Entry {
            timestamp: 0,
            value: snapshot,
        });

        self.instruction_count = 0;

        Ok(())
    }

    pub const fn instruction_count(&self) -> u64 {
        self.instruction_count
    }

    pub fn result(&self) -> Option<&[RawValue]> {
        self.completed.as_deref()
    }

    pub const fn is_completed(&self) -> bool {
        self.completed.is_some()
    }

    /// Returns call stack frames (bottom to top).
    pub fn call_stack(&self) -> impl Iterator<Item = FrameInfo> + use<'_> {
        self.store.call_stack().iter().map(|frame| {
            let cf = &self.store.instances[frame.module_i as usize]
                .code
                .compiled_funcs[frame.compiled_func_i as usize];
            FrameInfo {
                module_i: frame.module_i,
                compiled_func_i: frame.compiled_func_i,
                pc: frame.pc,
                locals: frame.locals.clone(),
                local_types: cf.local_types.clone(),
                source_position: cf
                    .source_positions
                    .get(frame.pc)
                    .copied()
                    .unwrap_or(frame.pc as u32),
            }
        })
    }

    pub fn value_stack(&self) -> &[RawValue] {
        let Some(frame) = self.store.top_frame() else {
            return &[];
        };
        self.store.value_stack_from(frame.stack_base)
    }

    pub fn globals(&self, module_i: u16) -> Vec<(&GlobalType, &RawValue)> {
        let inst = &self.store.instances[module_i as usize];
        inst.global_addrs
            .iter()
            .map(|&addr| {
                let g = &self.store.globals[addr];
                (&g.global_type, &g.value)
            })
            .collect()
    }

    pub fn read_memory(&self, module_i: u16, mem_i: usize, offset: usize, length: usize) -> &[u8] {
        let inst = &self.store.instances[module_i as usize];
        let mem_addr = inst.mem_addrs[mem_i];
        self.store.memories[mem_addr]
            .data
            .read_bytes(offset, length)
    }

    pub fn memory_size(&self, module_i: u16, mem_i: usize) -> Option<usize> {
        let inst = &self.store.instances[module_i as usize];
        let mem_addr = *inst.mem_addrs.get(mem_i)?;
        Some(self.store.memories[mem_addr].data.len())
    }

    pub fn into_store(self) -> Store {
        self.store
    }

    pub fn set_breakpoint(&mut self, breakpoint: Breakpoint) -> bool {
        self.breakpoints.insert(breakpoint)
    }

    pub fn remove_breakpoint(&mut self, breakpoint: &Breakpoint) -> bool {
        self.breakpoints.remove(breakpoint)
    }

    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.clear();
    }

    fn at_breakpoint(&self) -> bool {
        let Some(frame) = self.store.top_frame() else {
            return false;
        };

        self.breakpoints.contains(&Breakpoint {
            module_i: frame.module_i,
            compiled_func_i: frame.compiled_func_i,
            pc: frame.pc,
        })
    }

    pub fn step_forward(&mut self) -> Result<StepResult> {
        if self.completed.is_some() {
            return Ok(StepResult::Completed);
        }

        self.store.set_fuel(1);

        let out = match self.store.resume()? {
            ExecutionState::Completed(raw_values) => {
                self.instruction_count += 1;
                self.completed = Some(raw_values);

                StepResult::Completed
            }
            ExecutionState::FuelExhausted => {
                self.instruction_count += 1;

                if self
                    .instruction_count
                    .is_multiple_of(self.instructions_between_snapshots)
                {
                    self.history.push(Entry {
                        timestamp: self.instruction_count,
                        value: self.store.snapshot(),
                    })
                }

                if self.at_breakpoint() {
                    return Ok(StepResult::BreakpointHit);
                }

                StepResult::Stepped
            }
            ExecutionState::Suspended { .. } => todo!("should this happen?"),
        };

        Ok(out)
    }

    pub fn step_back(&mut self) -> Result<StepResult> {
        if self.instruction_count == 0 {
            return Ok(StepResult::ReachedStart);
        }

        self.completed = None;

        let nearest_snapshot = self
            .history
            .find_nearest_before(self.instruction_count - 1)
            .expect("should always exist since we snapshot at instr count = 0!");

        self.store = Store::from_snapshot(&nearest_snapshot.value);

        let steps_to_replay = self.instruction_count - 1 - nearest_snapshot.timestamp;
        self.store.set_fuel(steps_to_replay);
        let _ = self.store.resume()?;

        self.instruction_count -= 1;

        if self.at_breakpoint() {
            return Ok(StepResult::BreakpointHit);
        }

        Ok(StepResult::Stepped)
    }

    pub fn step_out(&mut self) -> Result<StepResult> {
        let target_depth = self.store.call_stack().len().saturating_sub(1);
        loop {
            let out = self.step_forward()?;
            match out {
                StepResult::Stepped if self.store.call_stack().len() <= target_depth => {
                    return Ok(StepResult::Stepped);
                }
                StepResult::Stepped => continue,
                other => return Ok(other),
            }
        }
    }

    pub fn continue_forward(&mut self) -> Result<StepResult> {
        loop {
            let out = self.step_forward();
            if !matches!(out, Ok(StepResult::Stepped)) {
                return out;
            }
        }
    }

    pub fn continue_backward(&mut self) -> Result<StepResult> {
        loop {
            let out = self.step_back();
            if !matches!(out, Ok(StepResult::Stepped)) {
                return out;
            }
        }
    }
}

#[cfg(all(test, not(feature = "jit")))]
mod tests {
    use super::*;
    use crate::{ir::CompilerMode, Module};

    fn setup_debugger(wasm_path: &str, func: &str, args: Vec<RawValue>) -> Debugger {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let wasm = std::fs::read(workspace_root.join(wasm_path)).unwrap();
        let module = Module::new_with_mode(&wasm, CompilerMode::Debug).unwrap();
        let mut store = Store::new();
        let instance = store.instantiate(&module, vec![]).unwrap();
        let mut debugger = Debugger::new_with_config(
            store,
            instance,
            DebuggerConfig {
                decay_buffer_capacity: 100,
                half_life: 10.0,
                instructions_between_snapshots: 10,
            },
        );
        debugger.start(func, args).unwrap();
        debugger
    }

    #[test]
    fn test_step_forward_completes() {
        let mut dbg = setup_debugger(
            "programs/stair_climb.wasm",
            "stair_climb",
            vec![RawValue::from(3_i32)],
        );

        loop {
            match dbg.step_forward().unwrap() {
                StepResult::Completed => break,
                StepResult::Stepped => {}
                other => panic!("unexpected: {other:?}"),
            }
        }

        let result = dbg.result().unwrap();
        assert_eq!(result[0].as_i32(), 4);
    }

    #[test]
    fn test_step_forward_then_back() {
        let mut dbg = setup_debugger(
            "programs/stair_climb.wasm",
            "stair_climb",
            vec![RawValue::from(3_i32)],
        );

        for _ in 0..20 {
            dbg.step_forward().unwrap();
        }
        assert_eq!(dbg.instruction_count(), 20);

        dbg.step_back().unwrap();
        assert_eq!(dbg.instruction_count(), 19);

        dbg.step_back().unwrap();
        assert_eq!(dbg.instruction_count(), 18);
    }

    #[test]
    fn test_step_back_to_start() {
        let mut dbg = setup_debugger(
            "programs/stair_climb.wasm",
            "stair_climb",
            vec![RawValue::from(3_i32)],
        );

        for _ in 0..5 {
            dbg.step_forward().unwrap();
        }

        for _ in 0..5 {
            dbg.step_back().unwrap();
        }
        assert_eq!(dbg.instruction_count(), 0);

        let result = dbg.step_back().unwrap();
        assert!(matches!(result, StepResult::ReachedStart));
    }

    #[test]
    fn test_step_back_at_start_returns_reached_start() {
        let mut dbg = setup_debugger(
            "programs/stair_climb.wasm",
            "stair_climb",
            vec![RawValue::from(3_i32)],
        );

        let result = dbg.step_back().unwrap();
        assert!(matches!(result, StepResult::ReachedStart));
    }

    #[test]
    fn test_step_forward_after_step_back_same_result() {
        let mut dbg = setup_debugger(
            "programs/stair_climb.wasm",
            "stair_climb",
            vec![RawValue::from(3_i32)],
        );

        // step forward 5, back 3, then run to completion
        for _ in 0..5 {
            dbg.step_forward().unwrap();
        }
        for _ in 0..3 {
            dbg.step_back().unwrap();
        }

        loop {
            match dbg.step_forward().unwrap() {
                StepResult::Completed => break,
                StepResult::Stepped => {}
                other => panic!("unexpected: {other:?}"),
            }
        }

        let result = dbg.result().unwrap();
        assert_eq!(result[0].as_i32(), 4);
    }

    #[test]
    fn test_completed_then_step_back() {
        let mut dbg = setup_debugger(
            "programs/stair_climb.wasm",
            "stair_climb",
            vec![RawValue::from(3_i32)],
        );

        loop {
            match dbg.step_forward().unwrap() {
                StepResult::Completed => break,
                StepResult::Stepped => {}
                other => panic!("unexpected: {other:?}"),
            }
        }
        assert!(dbg.result().is_some());

        dbg.step_back().unwrap();
        assert!(dbg.result().is_none());
        assert!(dbg.instruction_count() > 0);

        match dbg.step_forward().unwrap() {
            StepResult::Completed => {}
            other => panic!("unexpected: {other:?}"),
        }
        assert_eq!(dbg.result().unwrap()[0].as_i32(), 4);
    }

    #[test]
    fn test_step_back_restores_memory() {
        let mut dbg = setup_debugger("programs/sieve.wasm", "count_primes", vec![]);

        for _ in 0..100 {
            dbg.step_forward().unwrap();
        }

        let mem_at_100 = dbg.store.memories[0].data.clone();

        // we've stepped to a point where we're modifying memory
        assert!(mem_at_100.as_slice().iter().any(|&b| b != 0));

        for _ in 0..100 {
            dbg.step_forward().unwrap();
        }

        assert_eq!(dbg.instruction_count(), 200);
        let mem_at_200 = dbg.store.memories[0].data.clone();
        assert_ne!(mem_at_100, mem_at_200);

        for _ in 0..100 {
            dbg.step_back().unwrap();
        }
        assert_eq!(dbg.instruction_count(), 100);
        assert_eq!(dbg.store.memories[0].data, mem_at_100);
    }

    #[test]
    fn test_continue_forward_completes() {
        let mut dbg = setup_debugger(
            "programs/stair_climb.wasm",
            "stair_climb",
            vec![RawValue::from(3_i32)],
        );

        let result = dbg.continue_forward().unwrap();
        assert!(matches!(result, StepResult::Completed));
        assert_eq!(dbg.result().unwrap()[0].as_i32(), 4);
    }

    #[test]
    fn test_continue_forward_stops_at_breakpoint() {
        let mut dbg = setup_debugger(
            "programs/stair_climb.wasm",
            "stair_climb",
            vec![RawValue::from(3_i32)],
        );

        for _ in 0..5 {
            dbg.step_forward().unwrap();
        }
        let frame = dbg.store.top_frame().unwrap();
        let bp = Breakpoint {
            module_i: frame.module_i,
            compiled_func_i: frame.compiled_func_i,
            pc: frame.pc,
        };
        let bp_ic = dbg.instruction_count();

        for _ in 0..5 {
            dbg.step_back().unwrap();
        }
        assert_eq!(dbg.instruction_count(), 0);

        dbg.set_breakpoint(bp);
        let result = dbg.continue_forward().unwrap();
        assert!(matches!(result, StepResult::BreakpointHit));
        assert_eq!(dbg.instruction_count(), bp_ic);
    }

    #[test]
    fn test_continue_backward_stops_at_breakpoint() {
        let mut dbg = setup_debugger(
            "programs/stair_climb.wasm",
            "stair_climb",
            vec![RawValue::from(3_i32)],
        );

        for _ in 0..5 {
            dbg.step_forward().unwrap();
        }
        let frame = dbg.store.top_frame().unwrap();
        let bp = Breakpoint {
            module_i: frame.module_i,
            compiled_func_i: frame.compiled_func_i,
            pc: frame.pc,
        };

        for _ in 0..20 {
            dbg.step_forward().unwrap();
        }
        let ic_before = dbg.instruction_count();

        dbg.set_breakpoint(bp);
        let result = dbg.continue_backward().unwrap();
        assert!(matches!(result, StepResult::BreakpointHit));
        assert!(dbg.instruction_count() < ic_before);
        assert!(dbg.instruction_count() > 0);
    }

    #[test]
    fn test_continue_backward_reaches_start() {
        let mut dbg = setup_debugger(
            "programs/stair_climb.wasm",
            "stair_climb",
            vec![RawValue::from(3_i32)],
        );

        for _ in 0..10 {
            dbg.step_forward().unwrap();
        }

        let result = dbg.continue_backward().unwrap();
        assert!(matches!(result, StepResult::ReachedStart));
        assert_eq!(dbg.instruction_count(), 0);
    }

    #[test]
    fn test_step_back_preserves_source_positions() {
        let mut dbg = setup_debugger(
            "programs/stair_climb.wasm",
            "stair_climb",
            vec![RawValue::from(3_i32)],
        );

        let mut positions_at = Vec::new();
        let frame = dbg.call_stack().last().unwrap();
        positions_at.push(frame.source_position);

        for _ in 0..10 {
            dbg.step_forward().unwrap();
            let frame = dbg.call_stack().last().unwrap();
            positions_at.push(frame.source_position);
        }

        for _ in 0..10 {
            dbg.step_back().unwrap();
            let ic = dbg.instruction_count() as usize;
            let frame = dbg.call_stack().last().unwrap();
            assert_eq!(
                frame.source_position, positions_at[ic],
                "source position mismatch at instruction_count={}",
                ic
            );
        }
    }
}
