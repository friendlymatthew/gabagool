use crate::binary_grammar::{
    BlockType, CatchClause, CompositeType, Function, Instruction, ParsedModule, SubType, ValueType,
};
use crate::ir::{
    CatchKind, CompiledCatchClause, CompiledFunction, CompilerMode, JumpTableEntry, Op,
};
use crate::ImportDescription;

const UNREACHABLE_DEPTH: i32 = i32::MIN;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct LabelId(u32);

#[derive(Debug, Clone, Copy)]
enum CompilerOp {
    Op(Op),
    Label(LabelId),
}

impl From<Op> for CompilerOp {
    fn from(op: Op) -> Self {
        Self::Op(op)
    }
}

impl From<LabelId> for CompilerOp {
    fn from(l: LabelId) -> Self {
        Self::Label(l)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BlockKind {
    Function,
    Block,
    Loop,
    IfElse,
}

#[derive(Debug)]
struct BlockContext {
    kind: BlockKind,
    entry_stack_height: i32,
    branch_arity: usize,
    start_label: LabelId,
    end_label: LabelId,
}

/// Compiled IR and constant tables for a parsed Wasm module
#[derive(Debug, Clone)]
pub struct ModuleCode {
    pub(crate) compiled_funcs: Vec<CompiledFunction>,
    pub(crate) types: Vec<SubType>,
    pub(crate) v128_constants: Vec<i128>,
    pub(crate) jump_tables: Vec<Vec<JumpTableEntry>>,
    pub(crate) shuffle_masks: Vec<[u8; 16]>,
    pub(crate) catch_handlers: Vec<Vec<CompiledCatchClause>>,
}

struct Compiler<'a> {
    mode: CompilerMode,
    types: &'a [SubType],
    func_signatures: Vec<(usize, usize)>,
    tag_signatures: Vec<usize>,
    ops: Vec<CompilerOp>,
    source_positions: Vec<u32>,
    current_source_pos: u32,
    next_source_pos: u32,
    block_stack: Vec<BlockContext>,
    stack_height: i32,
    max_stack_height: i32,
    next_label: u32,
    jump_table_base: usize,
    catch_handler_base: usize,
    v128_constants: Vec<i128>,
    jump_tables: Vec<Vec<JumpTableEntry>>,
    shuffle_masks: Vec<[u8; 16]>,
    catch_handlers: Vec<Vec<CompiledCatchClause>>,
}

pub fn compile(module: &ParsedModule, mode: CompilerMode) -> ModuleCode {
    let mut v128_constants = Vec::new();
    let mut jump_tables = Vec::new();
    let mut shuffle_masks = Vec::new();
    let mut catch_handlers = Vec::new();

    let resolve_sig = |type_idx: u32, types: &[SubType]| -> (usize, usize) {
        match &types[type_idx as usize].composite_type {
            CompositeType::Func(ft) => (ft.0 .0.len(), ft.1 .0.len()),
            _ => (0, 0),
        }
    };
    let mut func_signatures = module
        .import_declarations
        .iter()
        .filter_map(|imp| match &imp.description {
            ImportDescription::Func(type_idx) => Some(resolve_sig(*type_idx, &module.types)),
            _ => None,
        })
        .collect::<Vec<_>>();
    for f in &module.functions {
        func_signatures.push(resolve_sig(f.type_index, &module.types));
    }

    let resolve_tag_sig = |type_idx: u32, types: &[SubType]| -> usize {
        match &types[type_idx as usize].composite_type {
            CompositeType::Func(ft) => ft.0 .0.len(),
            _ => 0,
        }
    };
    let mut tag_signatures: Vec<usize> = module
        .import_declarations
        .iter()
        .filter_map(|imp| match &imp.description {
            ImportDescription::Tag(type_idx) => Some(resolve_tag_sig(*type_idx, &module.types)),
            _ => None,
        })
        .collect();
    for tag in &module.tags {
        tag_signatures.push(resolve_tag_sig(tag.type_index, &module.types));
    }

    let functions = module
        .functions
        .iter()
        .map(|f| {
            let mut compiler = Compiler {
                mode,
                types: &module.types,
                func_signatures: func_signatures.clone(),
                tag_signatures: tag_signatures.clone(),
                ops: Vec::new(),
                source_positions: Vec::new(),
                current_source_pos: 0,
                next_source_pos: 0,
                block_stack: Vec::new(),
                stack_height: 0,
                max_stack_height: 0,
                next_label: 0,
                jump_table_base: 0,
                catch_handler_base: 0,
                v128_constants: std::mem::take(&mut v128_constants),
                jump_tables: std::mem::take(&mut jump_tables),
                shuffle_masks: std::mem::take(&mut shuffle_masks),
                catch_handlers: std::mem::take(&mut catch_handlers),
            };
            let cf = compiler.compile_function(f);

            v128_constants = compiler.v128_constants;
            jump_tables = compiler.jump_tables;
            shuffle_masks = compiler.shuffle_masks;
            catch_handlers = compiler.catch_handlers;
            cf
        })
        .collect();

    ModuleCode {
        compiled_funcs: functions,
        types: module.types.clone(),
        v128_constants,
        jump_tables,
        shuffle_masks,
        catch_handlers,
    }
}

pub fn compile_function_into_code(
    types: &[SubType],
    func: &Function,
    code: &mut ModuleCode,
    mode: CompilerMode,
) -> CompiledFunction {
    let mut compiler = Compiler {
        mode,
        types,
        func_signatures: Vec::new(),
        tag_signatures: Vec::new(),
        ops: Vec::new(),
        source_positions: Vec::new(),
        current_source_pos: 0,
        next_source_pos: 0,
        block_stack: Vec::new(),
        stack_height: 0,
        max_stack_height: 0,
        next_label: 0,
        jump_table_base: 0,
        catch_handler_base: 0,
        v128_constants: std::mem::take(&mut code.v128_constants),
        jump_tables: std::mem::take(&mut code.jump_tables),
        shuffle_masks: std::mem::take(&mut code.shuffle_masks),
        catch_handlers: std::mem::take(&mut code.catch_handlers),
    };
    let cf = compiler.compile_function(func);
    code.v128_constants = compiler.v128_constants;
    code.jump_tables = compiler.jump_tables;
    code.shuffle_masks = compiler.shuffle_masks;
    code.catch_handlers = compiler.catch_handlers;
    cf
}

impl<'a> Compiler<'a> {
    fn resolve_type_sig(&self, type_idx: u32) -> (usize, usize) {
        match &self.types[type_idx as usize].composite_type {
            CompositeType::Func(ft) => (ft.0 .0.len(), ft.1 .0.len()),
            _ => (0, 0),
        }
    }

    fn resolve_block_type(&self, bt: &BlockType) -> (usize, usize) {
        match bt {
            BlockType::Empty => (0, 0),
            BlockType::SingleValue(_) => (0, 1),
            BlockType::TypeIndex(idx) => {
                let st = &self.types[*idx as usize];
                match &st.composite_type {
                    CompositeType::Func(ft) => (ft.0 .0.len(), ft.1 .0.len()),
                    _ => (0, 0),
                }
            }
        }
    }

    fn compile_function(&mut self, func: &Function) -> CompiledFunction {
        let st = &self.types[func.type_index as usize];

        let (num_args, num_results) = if let CompositeType::Func(ft) = &st.composite_type {
            (ft.0 .0.len(), ft.1 .0.len())
        } else {
            (0, 0)
        };

        self.jump_table_base = self.jump_tables.len();
        self.catch_handler_base = self.catch_handlers.len();

        let start_label = self.next_label();
        let end_label = self.next_label();

        self.stack_height = num_args as i32;
        self.emit_label(start_label);
        self.block_stack.push(BlockContext {
            kind: BlockKind::Function,
            entry_stack_height: num_args as i32,
            branch_arity: num_results,
            start_label,
            end_label,
        });

        for instr in &func.body {
            self.compile_instruction(instr);
        }

        self.block_stack.pop().unwrap();
        self.emit_label(end_label);
        self.emit(Op::Return);
        self.strip_dead_labels();
        if self.mode == CompilerMode::Optimize {
            self.fuse_ops();
        }

        let assembled = self.assemble();

        let extra_locals: usize = func.locals.iter().map(|l| l.count as usize).sum();
        let mut local_types: Vec<ValueType> = match &st.composite_type {
            CompositeType::Func(ft) => {
                let mut v = Vec::with_capacity(ft.0 .0.len() + extra_locals);
                v.extend_from_slice(&ft.0 .0);
                v
            }
            _ => Vec::with_capacity(extra_locals),
        };

        for local in &func.locals {
            for _ in 0..local.count {
                local_types.push(local.value_type.clone());
            }
        }

        CompiledFunction {
            ops: assembled,
            type_index: func.type_index,
            num_args: num_args as u32,
            local_types,
            max_stack_height: self.max_stack_height as u32,
            source_positions: if self.mode == CompilerMode::Debug {
                std::mem::take(&mut self.source_positions)
            } else {
                Vec::new()
            },
        }
    }

    const fn next_label(&mut self) -> LabelId {
        let id = LabelId(self.next_label);
        self.next_label += 1;
        id
    }

    fn emit_label(&mut self, label: LabelId) {
        self.ops.push(CompilerOp::Label(label));
        if self.mode == CompilerMode::Debug {
            self.source_positions.push(u32::MAX);
        }
    }

    fn emit(&mut self, op: Op) {
        self.ops.push(CompilerOp::Op(op));
        if self.mode == CompilerMode::Debug {
            self.source_positions.push(self.current_source_pos);
        }
    }

    fn compile_catch_clauses(
        &self,
        catches: &[CatchClause],
        try_entry_height: i32,
    ) -> Vec<CompiledCatchClause> {
        catches
            .iter()
            .map(|c| {
                let (kind, tag_idx, label) = match c {
                    CatchClause::Catch { tag, label } => (CatchKind::Catch, *tag, *label),
                    CatchClause::CatchRef { tag, label } => (CatchKind::CatchRef, *tag, *label),
                    CatchClause::CatchAll { label } => (CatchKind::CatchAll, 0, *label),
                    CatchClause::CatchAllRef { label } => (CatchKind::CatchAllRef, 0, *label),
                };

                let n_tag_values = match kind {
                    CatchKind::Catch | CatchKind::CatchRef => {
                        self.tag_signatures[tag_idx as usize] as u16
                    }
                    _ => 0,
                };
                let n_values = n_tag_values
                    + match kind {
                        CatchKind::CatchRef | CatchKind::CatchAllRef => 1,
                        _ => 0,
                    };

                let idx = self.block_stack.len() - 2 - label as usize;
                let ctx = &self.block_stack[idx];
                let target = if ctx.kind == BlockKind::Loop {
                    ctx.start_label
                } else {
                    ctx.end_label
                };

                let drop = (try_entry_height - ctx.entry_stack_height) as u16;

                CompiledCatchClause {
                    kind,
                    tag_idx,
                    target: target.0,
                    n_values,
                    drop,
                }
            })
            .collect()
    }

    fn emit_branch(&mut self, depth: u32, conditional: bool, negate: bool) {
        let idx = self.block_stack.len() - 1 - depth as usize;
        let ctx = &self.block_stack[idx];

        let keep = ctx.branch_arity as u16;
        let drop = (self.stack_height - ctx.entry_stack_height - keep as i32) as u16;

        let target = if ctx.kind == BlockKind::Loop {
            ctx.start_label
        } else {
            ctx.end_label
        };

        match (conditional, negate) {
            (true, true) => self.emit(Op::JumpIfNot {
                target: target.0,
                keep,
                drop,
            }),
            (true, false) => self.emit(Op::JumpIf {
                target: target.0,
                keep,
                drop,
            }),
            (false, _) => self.emit(Op::Jump {
                target: target.0,
                keep,
                drop,
            }),
        };
    }

    fn assemble(&mut self) -> Vec<Op> {
        // build map from label id -> output position
        let mut label_positions = vec![0u32; self.next_label as usize];
        let mut pos: u32 = 0;
        for cop in &self.ops {
            match cop {
                CompilerOp::Label(id) => label_positions[id.0 as usize] = pos,
                CompilerOp::Op(_) => pos += 1,
            }
        }

        // resolve targets and strip labels
        let mut out = Vec::with_capacity(pos as usize);
        let mut assembled_positions = if self.mode == CompilerMode::Debug {
            Vec::with_capacity(pos as usize)
        } else {
            Vec::new()
        };
        for (idx, cop) in self.ops.iter().enumerate() {
            if let CompilerOp::Op(mut op) = *cop {
                Self::resolve_targets(&mut op, &label_positions);
                out.push(op);
                if self.mode == CompilerMode::Debug {
                    assembled_positions.push(self.source_positions[idx]);
                }
            }
        }

        // resolve jump table entries (only tables from the current function)
        for table in &mut self.jump_tables[self.jump_table_base..] {
            for entry in table.iter_mut() {
                entry.target = label_positions[entry.target as usize];
            }
        }

        // resolve catch handler targets (only handlers from the current function)
        for handler in &mut self.catch_handlers[self.catch_handler_base..] {
            for clause in handler.iter_mut() {
                clause.target = label_positions[clause.target as usize];
            }
        }

        self.ops.clear();
        if self.mode == CompilerMode::Debug {
            self.source_positions = assembled_positions;
        }

        out
    }

    fn resolve_targets(op: &mut Op, labels: &[u32]) {
        match op {
            Op::Jump { target, .. }
            | Op::JumpIf { target, .. }
            | Op::JumpIfNot { target, .. }
            | Op::BrOnNull { target, .. }
            | Op::BrOnNonNull { target, .. }
            | Op::I32EqZeroJumpIf { target, .. }
            | Op::I32EqZeroJumpIfNot { target, .. }
            | Op::I32EqJumpIf { target, .. }
            | Op::I32NeJumpIf { target, .. }
            | Op::I32LtSignedJumpIf { target, .. }
            | Op::I32LtUnsignedJumpIf { target, .. }
            | Op::I32GtSignedJumpIf { target, .. }
            | Op::I32GtUnsignedJumpIf { target, .. }
            | Op::I32LeSignedJumpIf { target, .. }
            | Op::I32LeUnsignedJumpIf { target, .. }
            | Op::I32GeSignedJumpIf { target, .. }
            | Op::I32GeUnsignedJumpIf { target, .. }
            | Op::I64EqZeroJumpIf { target, .. }
            | Op::I64EqJumpIf { target, .. }
            | Op::I64NeJumpIf { target, .. }
            | Op::I64LtSignedJumpIf { target, .. }
            | Op::I64LtUnsignedJumpIf { target, .. }
            | Op::I64GtSignedJumpIf { target, .. }
            | Op::I64GtUnsignedJumpIf { target, .. }
            | Op::I64LeSignedJumpIf { target, .. }
            | Op::I64LeUnsignedJumpIf { target, .. }
            | Op::I64GeSignedJumpIf { target, .. }
            | Op::I64GeUnsignedJumpIf { target, .. }
            | Op::F32EqJumpIf { target, .. }
            | Op::F32NeJumpIf { target, .. }
            | Op::F32LtJumpIf { target, .. }
            | Op::F32GtJumpIf { target, .. }
            | Op::F32LeJumpIf { target, .. }
            | Op::F32GeJumpIf { target, .. }
            | Op::F64EqJumpIf { target, .. }
            | Op::F64NeJumpIf { target, .. }
            | Op::F64LtJumpIf { target, .. }
            | Op::F64GtJumpIf { target, .. }
            | Op::F64LeJumpIf { target, .. }
            | Op::F64GeJumpIf { target, .. } => {
                *target = labels[*target as usize];
            }
            _ => {}
        }
    }

    fn strip_dead_labels(&mut self) {
        let mut live = vec![false; self.next_label as usize];

        for cop in &self.ops {
            if let CompilerOp::Op(op) = cop {
                if let Some(target) = op.jump_target() {
                    live[target as usize] = true;
                }
            }
        }
        for table in &self.jump_tables[self.jump_table_base..] {
            for entry in table {
                live[entry.target as usize] = true;
            }
        }
        for handler in &self.catch_handlers[self.catch_handler_base..] {
            for clause in handler {
                live[clause.target as usize] = true;
            }
        }
        if self.mode == CompilerMode::Debug {
            let mut j = 0;
            for i in 0..self.ops.len() {
                let keep = match &self.ops[i] {
                    CompilerOp::Label(id) => live[id.0 as usize],
                    _ => true,
                };
                if keep {
                    self.ops[j] = self.ops[i];
                    self.source_positions[j] = self.source_positions[i];
                    j += 1;
                }
            }
            self.ops.truncate(j);
            self.source_positions.truncate(j);
        } else {
            self.ops.retain(|cop| match cop {
                CompilerOp::Label(id) => live[id.0 as usize],
                _ => true,
            });
        }
    }

    fn fuse_ops(&mut self) {
        let mut out = Vec::with_capacity(self.ops.len());
        let mut i = 0;

        while i < self.ops.len() {
            match &self.ops[i..] {
                [CompilerOp::Op(Op::LocalGet {
                    local_idx: local_get_idx,
                }), CompilerOp::Op(Op::LocalSet {
                    local_idx: local_set_idx,
                }), ..] => {
                    out.push(
                        Op::LocalGetLocalSet {
                            local_get_idx: *local_get_idx,
                            local_set_idx: *local_set_idx,
                        }
                        .into(),
                    );

                    i += 2;
                }
                [CompilerOp::Op(Op::LocalGet { local_idx }), CompilerOp::Op(Op::I32Store { offset, memory }), ..] =>
                {
                    out.push(
                        Op::LocalGetI32Store {
                            local_idx: *local_idx,
                            offset: *offset,
                            memory: *memory,
                        }
                        .into(),
                    );

                    i += 2;
                }
                [CompilerOp::Op(Op::LocalGet { local_idx }), CompilerOp::Op(Op::I32Load { offset, memory }), ..] =>
                {
                    out.push(
                        Op::LocalGetI32Load {
                            local_idx: *local_idx,
                            offset: *offset,
                            memory: *memory,
                        }
                        .into(),
                    );

                    i += 2;
                }
                [CompilerOp::Op(Op::LocalGet { local_idx }), CompilerOp::Op(Op::I64Load { offset, memory }), ..] =>
                {
                    out.push(
                        Op::LocalGetI64Load {
                            local_idx: *local_idx,
                            offset: *offset,
                            memory: *memory,
                        }
                        .into(),
                    );

                    i += 2;
                }
                [CompilerOp::Op(Op::LocalGet { local_idx }), CompilerOp::Op(Op::F32Load { offset, memory }), ..] =>
                {
                    out.push(
                        Op::LocalGetF32Load {
                            local_idx: *local_idx,
                            offset: *offset,
                            memory: *memory,
                        }
                        .into(),
                    );

                    i += 2;
                }
                [CompilerOp::Op(Op::LocalGet { local_idx }), CompilerOp::Op(Op::F64Load { offset, memory }), ..] =>
                {
                    out.push(
                        Op::LocalGetF64Load {
                            local_idx: *local_idx,
                            offset: *offset,
                            memory: *memory,
                        }
                        .into(),
                    );

                    i += 2;
                }
                [CompilerOp::Op(Op::LocalGet { local_idx }), CompilerOp::Op(Op::I64Store { offset, memory }), ..] =>
                {
                    out.push(
                        Op::LocalGetI64Store {
                            local_idx: *local_idx,
                            offset: *offset,
                            memory: *memory,
                        }
                        .into(),
                    );

                    i += 2;
                }
                [CompilerOp::Op(Op::LocalGet { local_idx }), CompilerOp::Op(Op::F32Store { offset, memory }), ..] =>
                {
                    out.push(
                        Op::LocalGetF32Store {
                            local_idx: *local_idx,
                            offset: *offset,
                            memory: *memory,
                        }
                        .into(),
                    );

                    i += 2;
                }
                [CompilerOp::Op(Op::LocalGet { local_idx }), CompilerOp::Op(Op::F64Store { offset, memory }), ..] =>
                {
                    out.push(
                        Op::LocalGetF64Store {
                            local_idx: *local_idx,
                            offset: *offset,
                            memory: *memory,
                        }
                        .into(),
                    );

                    i += 2;
                }
                [CompilerOp::Op(Op::LocalGet { local_idx }), CompilerOp::Op(Op::Return), ..] => {
                    out.push(
                        Op::LocalGetReturn {
                            local_idx: *local_idx,
                        }
                        .into(),
                    );

                    i += 2;
                }
                [CompilerOp::Op(Op::LocalGet {
                    local_idx: local_idx_a,
                }), CompilerOp::Op(Op::LocalGet {
                    local_idx: local_idx_b,
                }), ..] => {
                    out.push(
                        Op::LocalGet2 {
                            local_idx_a: *local_idx_a,
                            local_idx_b: *local_idx_b,
                        }
                        .into(),
                    );

                    i += 2;
                }
                [CompilerOp::Op(Op::I32EqZero), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I32EqZeroJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I32EqZero), CompilerOp::Op(Op::JumpIfNot { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I32EqZeroJumpIfNot {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I32Eq), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I32EqJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I32Ne), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I32NeJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I32LtSigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I32LtSignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I32LtUnsigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I32LtUnsignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I32GtSigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I32GtSignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I32GtUnsigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I32GtUnsignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I32LeSigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I32LeSignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I32LeUnsigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I32LeUnsignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I32GeSigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I32GeSignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I32GeUnsigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I32GeUnsignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I64EqZero), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I64EqZeroJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I64Eq), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I64EqJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I64Ne), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I64NeJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I64LtSigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I64LtSignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I64LtUnsigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I64LtUnsignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I64GtSigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I64GtSignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I64GtUnsigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I64GtUnsignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I64LeSigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I64LeSignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I64LeUnsigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I64LeUnsignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I64GeSigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I64GeSignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::I64GeUnsigned), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::I64GeUnsignedJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::F32Eq), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::F32EqJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::F32Ne), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::F32NeJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::F32Lt), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::F32LtJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::F32Gt), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::F32GtJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::F32Le), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::F32LeJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::F32Ge), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::F32GeJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::F64Eq), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::F64EqJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::F64Ne), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::F64NeJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::F64Lt), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::F64LtJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::F64Gt), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::F64GtJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::F64Le), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::F64LeJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [CompilerOp::Op(Op::F64Ge), CompilerOp::Op(Op::JumpIf { target, keep, drop }), ..] =>
                {
                    out.push(
                        Op::F64GeJumpIf {
                            target: *target,
                            keep: *keep,
                            drop: *drop,
                        }
                        .into(),
                    );
                    i += 2;
                }
                [op, ..] => {
                    out.push(*op);
                    i += 1;
                }
                [] => break,
            }
        }

        self.ops = out;
    }

    #[cfg(all(test, not(any(feature = "core-tests", feature = "component-tests"))))]
    fn compile_and_get_ops(types: &[SubType], func: &Function, mode: CompilerMode) -> Vec<Op> {
        let mut compiler = Compiler {
            mode,
            types,
            func_signatures: Vec::new(),
            tag_signatures: Vec::new(),
            ops: Vec::new(),
            source_positions: Vec::new(),
            current_source_pos: 0,
            next_source_pos: 0,
            block_stack: Vec::new(),
            stack_height: 0,
            max_stack_height: 0,
            next_label: 0,
            jump_table_base: 0,
            catch_handler_base: 0,
            v128_constants: Vec::new(),
            jump_tables: Vec::new(),
            shuffle_masks: Vec::new(),
            catch_handlers: Vec::new(),
        };
        let cf = compiler.compile_function(func);
        cf.ops
    }

    fn compile_instruction(&mut self, instr: &Instruction) {
        if self.mode == CompilerMode::Debug {
            self.current_source_pos = self.next_source_pos;
            self.next_source_pos += 1;
        }

        match instr {
            Instruction::Block(..) | Instruction::Loop(..) | Instruction::IfElse(..) => {}
            _ => {
                if self.stack_height == UNREACHABLE_DEPTH {
                    return;
                }
            }
        }

        match instr {
            Instruction::Block(bt, body) => {
                let (m, n) = self.resolve_block_type(bt);

                let entry = ((self.stack_height != UNREACHABLE_DEPTH) as i32)
                    .wrapping_mul(self.stack_height.wrapping_sub(m as i32));

                let start_label = self.next_label();
                let end_label = self.next_label();

                self.emit_label(start_label);

                self.block_stack.push(BlockContext {
                    kind: BlockKind::Block,
                    entry_stack_height: entry,
                    branch_arity: n,
                    start_label,
                    end_label,
                });

                for i in body {
                    self.compile_instruction(i);
                }

                self.block_stack.pop().expect("we push right before");
                self.emit_label(end_label);
                self.stack_height = entry + n as i32;
            }
            Instruction::Loop(bt, body) => {
                let (m, n) = self.resolve_block_type(bt);

                let entry = ((self.stack_height != UNREACHABLE_DEPTH) as i32)
                    .wrapping_mul(self.stack_height.wrapping_sub(m as i32));

                let start_label = self.next_label();
                let end_label = self.next_label();

                self.emit_label(start_label);

                self.block_stack.push(BlockContext {
                    kind: BlockKind::Loop,
                    entry_stack_height: entry,
                    branch_arity: m,
                    start_label,
                    end_label,
                });

                for i in body {
                    self.compile_instruction(i);
                }

                self.block_stack.pop().unwrap();
                self.emit_label(end_label);
                self.stack_height = entry + n as i32;
            }
            Instruction::IfElse(bt, then_body, else_body) => {
                if self.stack_height != UNREACHABLE_DEPTH {
                    self.stack_height -= 1;
                }

                let (m, n) = self.resolve_block_type(bt);
                let entry = ((self.stack_height != UNREACHABLE_DEPTH) as i32)
                    .wrapping_mul(self.stack_height.wrapping_sub(m as i32));

                let else_label = self.next_label();
                let end_label = self.next_label();

                self.emit(Op::JumpIfNot {
                    target: else_label.0,
                    keep: 0,
                    drop: 0,
                });

                let start_label = self.next_label();
                self.emit_label(start_label);

                self.block_stack.push(BlockContext {
                    kind: BlockKind::IfElse,
                    entry_stack_height: entry,
                    branch_arity: n,
                    start_label,
                    end_label,
                });

                let saved_height = self.stack_height;

                for i in then_body {
                    self.compile_instruction(i);
                }

                if else_body.is_empty() {
                    self.emit_label(else_label);
                } else {
                    self.emit(Op::Jump {
                        target: end_label.0,
                        keep: 0,
                        drop: 0,
                    });

                    self.emit_label(else_label);

                    self.stack_height = saved_height;
                    for i in else_body {
                        self.compile_instruction(i);
                    }
                }

                self.block_stack.pop().unwrap();
                self.emit_label(end_label);
                self.stack_height = entry + n as i32;
            }
            Instruction::Br(depth) => {
                self.emit_branch(*depth, false, false);
                self.stack_height = UNREACHABLE_DEPTH;
            }
            Instruction::BrIf(depth) => {
                self.stack_height -= 1;
                self.emit_branch(*depth, true, false);
            }
            Instruction::BrTable(labels, default) => {
                self.stack_height -= 1;
                let default_idx = self.block_stack.len() - 1 - *default as usize;
                let keep = self.block_stack[default_idx].branch_arity as u16;

                let mut entries = Vec::with_capacity(labels.len() + 1);
                for label in labels.iter().chain(std::iter::once(default)) {
                    let idx = self.block_stack.len() - 1 - *label as usize;
                    let ctx = &self.block_stack[idx];
                    let drop = (self.stack_height - ctx.entry_stack_height - keep as i32) as u16;

                    let target = if ctx.kind == BlockKind::Loop {
                        ctx.start_label
                    } else {
                        ctx.end_label
                    };

                    entries.push(JumpTableEntry {
                        target: target.0,
                        drop,
                    });
                }

                let table_idx = self.jump_tables.len();
                self.jump_tables.push(entries);

                self.emit(Op::JumpTable {
                    index: table_idx as u32,
                    keep,
                });
                self.stack_height = UNREACHABLE_DEPTH;
            }
            Instruction::Return => {
                self.emit(Op::Return);
                self.stack_height = UNREACHABLE_DEPTH;
            }
            Instruction::Unreachable => {
                self.emit(Op::Unreachable);
                self.stack_height = UNREACHABLE_DEPTH;
            }
            Instruction::Nop => {
                self.emit(Op::Nop);
            }
            Instruction::Call(func_idx) => {
                let (n_params, n_results) = self.func_signatures[*func_idx as usize];
                self.emit(Op::Call {
                    func_idx: *func_idx,
                });
                self.stack_height -= n_params as i32;
                self.stack_height += n_results as i32;
            }
            Instruction::CallIndirect(type_idx, table_idx) => {
                let (n_params, n_results) = self.resolve_type_sig(*type_idx);
                self.stack_height -= 1;
                self.emit(Op::CallIndirect {
                    type_idx: *type_idx,
                    table_idx: *table_idx,
                });
                self.stack_height -= n_params as i32;
                self.stack_height += n_results as i32;
            }
            Instruction::ReturnCall(func_idx) => {
                self.emit(Op::ReturnCall {
                    func_idx: *func_idx,
                });
                self.stack_height = UNREACHABLE_DEPTH;
            }
            Instruction::ReturnCallIndirect(type_idx, table_idx) => {
                self.emit(Op::ReturnCallIndirect {
                    type_idx: *type_idx,
                    table_idx: *table_idx,
                });
                self.stack_height = UNREACHABLE_DEPTH;
            }
            Instruction::CallRef(type_idx) => {
                let (n_params, n_results) = self.resolve_type_sig(*type_idx);
                self.stack_height -= 1;
                self.emit(Op::CallRef {
                    type_idx: *type_idx,
                });
                self.stack_height -= n_params as i32;
                self.stack_height += n_results as i32;
            }
            Instruction::ReturnCallRef(type_idx) => {
                self.emit(Op::ReturnCallRef {
                    type_idx: *type_idx,
                });
                self.stack_height = UNREACHABLE_DEPTH;
            }
            Instruction::I32Const(v) => {
                self.emit(Op::I32Const { value: *v });
                self.stack_height += 1;
            }
            Instruction::I64Const(v) => {
                self.emit(Op::I64Const { value: *v });
                self.stack_height += 1;
            }
            Instruction::F32Const(v) => {
                self.emit(Op::F32Const { value: *v });
                self.stack_height += 1;
            }
            Instruction::F64Const(v) => {
                self.emit(Op::F64Const { value: *v });
                self.stack_height += 1;
            }
            Instruction::V128Const(v) => {
                let table_idx = self.v128_constants.len() as u32;
                self.v128_constants.push(*v);
                self.emit(Op::V128Const { table_idx });
                self.stack_height += 1;
            }
            Instruction::LocalGet(idx) => {
                self.emit(Op::LocalGet { local_idx: *idx });
                self.stack_height += 1;
            }
            Instruction::LocalSet(idx) => {
                self.emit(Op::LocalSet { local_idx: *idx });
                self.stack_height -= 1;
            }
            Instruction::LocalTee(idx) => {
                self.emit(Op::LocalTee { local_idx: *idx });
            }
            Instruction::GlobalGet(idx) => {
                self.emit(Op::GlobalGet { global_idx: *idx });
                self.stack_height += 1;
            }
            Instruction::GlobalSet(idx) => {
                self.emit(Op::GlobalSet { global_idx: *idx });
                self.stack_height -= 1;
            }
            Instruction::Drop => {
                self.emit(Op::Drop);
                self.stack_height -= 1;
            }
            Instruction::Select(_) => {
                self.emit(Op::Select);
                self.stack_height -= 2;
            }
            Instruction::RefNull(ht) => {
                self.emit(Op::RefNull(*ht));
                self.stack_height += 1;
            }
            Instruction::RefIsNull => {
                self.emit(Op::RefIsNull);
            }
            Instruction::RefEq => {
                self.emit(Op::RefEq);
                self.stack_height -= 1;
            }
            Instruction::RefAsNonNull => {
                self.emit(Op::RefAsNonNull);
            }
            Instruction::RefFunc(idx) => {
                self.emit(Op::RefFunc { func_idx: *idx });
                self.stack_height += 1;
            }
            Instruction::Throw(tag_idx) => {
                self.emit(Op::Throw { tag_idx: *tag_idx });
                self.stack_height = UNREACHABLE_DEPTH;
            }
            Instruction::ThrowRef => {
                self.emit(Op::ThrowRef);
                self.stack_height = UNREACHABLE_DEPTH;
            }
            Instruction::BrOnNull(depth) => {
                let idx = self.block_stack.len() - 1 - *depth as usize;
                let ctx = &self.block_stack[idx];
                let keep = ctx.branch_arity as u16;
                let drop = (self.stack_height - ctx.entry_stack_height - keep as i32 - 1) as u16;
                let target = if ctx.kind == BlockKind::Loop {
                    ctx.start_label
                } else {
                    ctx.end_label
                };
                self.emit(Op::BrOnNull {
                    target: target.0,
                    keep,
                    drop,
                });
            }
            Instruction::BrOnNonNull(depth) => {
                let idx = self.block_stack.len() - 1 - *depth as usize;
                let ctx = &self.block_stack[idx];
                let keep = ctx.branch_arity as u16;
                let drop = (self.stack_height - ctx.entry_stack_height - keep as i32) as u16;

                let target = if ctx.kind == BlockKind::Loop {
                    ctx.start_label
                } else {
                    ctx.end_label
                };

                self.emit(Op::BrOnNonNull {
                    target: target.0,
                    keep,
                    drop,
                });

                self.stack_height -= 1;
            }
            Instruction::TableGet(idx) => {
                self.emit(Op::TableGet { table_idx: *idx });
            }
            Instruction::TableSet(idx) => {
                self.emit(Op::TableSet { table_idx: *idx });
                self.stack_height -= 2;
            }
            Instruction::TableInit(table_idx, elem_idx) => {
                self.emit(Op::TableInit {
                    elem_idx: *elem_idx,
                    table_idx: *table_idx,
                });
                self.stack_height -= 3;
            }
            Instruction::ElemDrop(idx) => {
                self.emit(Op::ElemDrop { elem_idx: *idx });
            }
            Instruction::TableCopy(dst, src) => {
                self.emit(Op::TableCopy {
                    dst_table_idx: *dst,
                    src_table_idx: *src,
                });
                self.stack_height -= 3;
            }
            Instruction::TableGrow(idx) => {
                self.emit(Op::TableGrow { table_idx: *idx });
                self.stack_height -= 1;
            }
            Instruction::TableSize(idx) => {
                self.emit(Op::TableSize { table_idx: *idx });
                self.stack_height += 1;
            }
            Instruction::TableFill(idx) => {
                self.emit(Op::TableFill { table_idx: *idx });
                self.stack_height -= 3;
            }
            Instruction::I32Load(ma) => {
                self.emit(Op::I32Load {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::I64Load(ma) => {
                self.emit(Op::I64Load {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::F32Load(ma) => {
                self.emit(Op::F32Load {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::F64Load(ma) => {
                self.emit(Op::F64Load {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::I32Load8Signed(ma) => {
                self.emit(Op::I32Load8Signed {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::I32Load8Unsigned(ma) => {
                self.emit(Op::I32Load8Unsigned {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::I32Load16Signed(ma) => {
                self.emit(Op::I32Load16Signed {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::I32Load16Unsigned(ma) => {
                self.emit(Op::I32Load16Unsigned {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::I64Load8Signed(ma) => {
                self.emit(Op::I64Load8Signed {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::I64Load8Unsigned(ma) => {
                self.emit(Op::I64Load8Unsigned {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::I64Load16Signed(ma) => {
                self.emit(Op::I64Load16Signed {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::I64Load16Unsigned(ma) => {
                self.emit(Op::I64Load16Unsigned {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::I64Load32Signed(ma) => {
                self.emit(Op::I64Load32Signed {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::I64Load32Unsigned(ma) => {
                self.emit(Op::I64Load32Unsigned {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::I32Store(ma) => {
                self.emit(Op::I32Store {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
                self.stack_height -= 2;
            }
            Instruction::I64Store(ma) => {
                self.emit(Op::I64Store {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
                self.stack_height -= 2;
            }
            Instruction::F32Store(ma) => {
                self.emit(Op::F32Store {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
                self.stack_height -= 2;
            }
            Instruction::F64Store(ma) => {
                self.emit(Op::F64Store {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
                self.stack_height -= 2;
            }
            Instruction::I32Store8(ma) => {
                self.emit(Op::I32Store8 {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
                self.stack_height -= 2;
            }
            Instruction::I32Store16(ma) => {
                self.emit(Op::I32Store16 {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
                self.stack_height -= 2;
            }
            Instruction::I64Store8(ma) => {
                self.emit(Op::I64Store8 {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
                self.stack_height -= 2;
            }
            Instruction::I64Store16(ma) => {
                self.emit(Op::I64Store16 {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
                self.stack_height -= 2;
            }
            Instruction::I64Store32(ma) => {
                self.emit(Op::I64Store32 {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
                self.stack_height -= 2;
            }
            Instruction::MemorySize(idx) => {
                self.emit(Op::MemorySize { memory_idx: *idx });
                self.stack_height += 1;
            }
            Instruction::MemoryGrow(idx) => {
                self.emit(Op::MemoryGrow { memory_idx: *idx });
            }
            Instruction::MemoryInit(data_idx, mem_idx) => {
                self.emit(Op::MemoryInit {
                    data_idx: *data_idx,
                    memory_idx: *mem_idx,
                });
                self.stack_height -= 3;
            }
            Instruction::DataDrop(idx) => {
                self.emit(Op::DataDrop { data_idx: *idx });
            }
            Instruction::MemoryCopy(dst, src) => {
                self.emit(Op::MemoryCopy {
                    dst_memory_idx: *dst,
                    src_memory_idx: *src,
                });
                self.stack_height -= 3;
            }
            Instruction::MemoryFill(idx) => {
                self.emit(Op::MemoryFill { memory_idx: *idx });
                self.stack_height -= 3;
            }
            Instruction::I32EqZero => {
                self.emit(Op::I32EqZero);
            }
            Instruction::I32Eq => {
                self.emit(Op::I32Eq);
                self.stack_height -= 1;
            }
            Instruction::I32Ne => {
                self.emit(Op::I32Ne);
                self.stack_height -= 1;
            }
            Instruction::I32LtSigned => {
                self.emit(Op::I32LtSigned);
                self.stack_height -= 1;
            }
            Instruction::I32LtUnsigned => {
                self.emit(Op::I32LtUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I32GtSigned => {
                self.emit(Op::I32GtSigned);
                self.stack_height -= 1;
            }
            Instruction::I32GtUnsigned => {
                self.emit(Op::I32GtUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I32LeSigned => {
                self.emit(Op::I32LeSigned);
                self.stack_height -= 1;
            }
            Instruction::I32LeUnsigned => {
                self.emit(Op::I32LeUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I32GeSigned => {
                self.emit(Op::I32GeSigned);
                self.stack_height -= 1;
            }
            Instruction::I32GeUnsigned => {
                self.emit(Op::I32GeUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I64EqZero => {
                self.emit(Op::I64EqZero);
            }
            Instruction::I64Eq => {
                self.emit(Op::I64Eq);
                self.stack_height -= 1;
            }
            Instruction::I64Ne => {
                self.emit(Op::I64Ne);
                self.stack_height -= 1;
            }
            Instruction::I64LtSigned => {
                self.emit(Op::I64LtSigned);
                self.stack_height -= 1;
            }
            Instruction::I64LtUnsigned => {
                self.emit(Op::I64LtUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I64GtSigned => {
                self.emit(Op::I64GtSigned);
                self.stack_height -= 1;
            }
            Instruction::I64GtUnsigned => {
                self.emit(Op::I64GtUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I64LeSigned => {
                self.emit(Op::I64LeSigned);
                self.stack_height -= 1;
            }
            Instruction::I64LeUnsigned => {
                self.emit(Op::I64LeUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I64GeSigned => {
                self.emit(Op::I64GeSigned);
                self.stack_height -= 1;
            }
            Instruction::I64GeUnsigned => {
                self.emit(Op::I64GeUnsigned);
                self.stack_height -= 1;
            }
            Instruction::F32Eq => {
                self.emit(Op::F32Eq);
                self.stack_height -= 1;
            }
            Instruction::F32Ne => {
                self.emit(Op::F32Ne);
                self.stack_height -= 1;
            }
            Instruction::F32Lt => {
                self.emit(Op::F32Lt);
                self.stack_height -= 1;
            }
            Instruction::F32Gt => {
                self.emit(Op::F32Gt);
                self.stack_height -= 1;
            }
            Instruction::F32Le => {
                self.emit(Op::F32Le);
                self.stack_height -= 1;
            }
            Instruction::F32Ge => {
                self.emit(Op::F32Ge);
                self.stack_height -= 1;
            }
            Instruction::F64Eq => {
                self.emit(Op::F64Eq);
                self.stack_height -= 1;
            }
            Instruction::F64Ne => {
                self.emit(Op::F64Ne);
                self.stack_height -= 1;
            }
            Instruction::F64Lt => {
                self.emit(Op::F64Lt);
                self.stack_height -= 1;
            }
            Instruction::F64Gt => {
                self.emit(Op::F64Gt);
                self.stack_height -= 1;
            }
            Instruction::F64Le => {
                self.emit(Op::F64Le);
                self.stack_height -= 1;
            }
            Instruction::F64Ge => {
                self.emit(Op::F64Ge);
                self.stack_height -= 1;
            }
            Instruction::I32CountLeadingZeros => {
                self.emit(Op::I32CountLeadingZeros);
            }
            Instruction::I32CountTrailingZeros => {
                self.emit(Op::I32CountTrailingZeros);
            }
            Instruction::I32PopCount => {
                self.emit(Op::I32PopCount);
            }
            Instruction::I32Add => {
                self.emit(Op::I32Add);
                self.stack_height -= 1;
            }
            Instruction::I32Sub => {
                self.emit(Op::I32Sub);
                self.stack_height -= 1;
            }
            Instruction::I32Mul => {
                self.emit(Op::I32Mul);
                self.stack_height -= 1;
            }
            Instruction::I32DivSigned => {
                self.emit(Op::I32DivSigned);
                self.stack_height -= 1;
            }
            Instruction::I32DivUnsigned => {
                self.emit(Op::I32DivUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I32RemainderSigned => {
                self.emit(Op::I32RemainderSigned);
                self.stack_height -= 1;
            }
            Instruction::I32RemainderUnsigned => {
                self.emit(Op::I32RemainderUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I32And => {
                self.emit(Op::I32And);
                self.stack_height -= 1;
            }
            Instruction::I32Or => {
                self.emit(Op::I32Or);
                self.stack_height -= 1;
            }
            Instruction::I32Xor => {
                self.emit(Op::I32Xor);
                self.stack_height -= 1;
            }
            Instruction::I32Shl => {
                self.emit(Op::I32Shl);
                self.stack_height -= 1;
            }
            Instruction::I32ShrSigned => {
                self.emit(Op::I32ShrSigned);
                self.stack_height -= 1;
            }
            Instruction::I32ShrUnsigned => {
                self.emit(Op::I32ShrUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I32RotateLeft => {
                self.emit(Op::I32RotateLeft);
                self.stack_height -= 1;
            }
            Instruction::I32RotateRight => {
                self.emit(Op::I32RotateRight);
                self.stack_height -= 1;
            }
            Instruction::I64CountLeadingZeros => {
                self.emit(Op::I64CountLeadingZeros);
            }
            Instruction::I64CountTrailingZeros => {
                self.emit(Op::I64CountTrailingZeros);
            }
            Instruction::I64PopCount => {
                self.emit(Op::I64PopCount);
            }
            Instruction::I64Add => {
                self.emit(Op::I64Add);
                self.stack_height -= 1;
            }
            Instruction::I64Sub => {
                self.emit(Op::I64Sub);
                self.stack_height -= 1;
            }
            Instruction::I64Mul => {
                self.emit(Op::I64Mul);
                self.stack_height -= 1;
            }
            Instruction::I64DivSigned => {
                self.emit(Op::I64DivSigned);
                self.stack_height -= 1;
            }
            Instruction::I64DivUnsigned => {
                self.emit(Op::I64DivUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I64RemainderSigned => {
                self.emit(Op::I64RemainderSigned);
                self.stack_height -= 1;
            }
            Instruction::I64RemainderUnsigned => {
                self.emit(Op::I64RemainderUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I64And => {
                self.emit(Op::I64And);
                self.stack_height -= 1;
            }
            Instruction::I64Or => {
                self.emit(Op::I64Or);
                self.stack_height -= 1;
            }
            Instruction::I64Xor => {
                self.emit(Op::I64Xor);
                self.stack_height -= 1;
            }
            Instruction::I64Shl => {
                self.emit(Op::I64Shl);
                self.stack_height -= 1;
            }
            Instruction::I64ShrSigned => {
                self.emit(Op::I64ShrSigned);
                self.stack_height -= 1;
            }
            Instruction::I64ShrUnsigned => {
                self.emit(Op::I64ShrUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I64RotateLeft => {
                self.emit(Op::I64RotateLeft);
                self.stack_height -= 1;
            }
            Instruction::I64RotateRight => {
                self.emit(Op::I64RotateRight);
                self.stack_height -= 1;
            }
            Instruction::F32Abs => {
                self.emit(Op::F32Abs);
            }
            Instruction::F32Neg => {
                self.emit(Op::F32Neg);
            }
            Instruction::F32Ceil => {
                self.emit(Op::F32Ceil);
            }
            Instruction::F32Floor => {
                self.emit(Op::F32Floor);
            }
            Instruction::F32Trunc => {
                self.emit(Op::F32Trunc);
            }
            Instruction::F32Nearest => {
                self.emit(Op::F32Nearest);
            }
            Instruction::F32Sqrt => {
                self.emit(Op::F32Sqrt);
            }
            Instruction::F32Add => {
                self.emit(Op::F32Add);
                self.stack_height -= 1;
            }
            Instruction::F32Sub => {
                self.emit(Op::F32Sub);
                self.stack_height -= 1;
            }
            Instruction::F32Mul => {
                self.emit(Op::F32Mul);
                self.stack_height -= 1;
            }
            Instruction::F32Div => {
                self.emit(Op::F32Div);
                self.stack_height -= 1;
            }
            Instruction::F32Min => {
                self.emit(Op::F32Min);
                self.stack_height -= 1;
            }
            Instruction::F32Max => {
                self.emit(Op::F32Max);
                self.stack_height -= 1;
            }
            Instruction::F32CopySign => {
                self.emit(Op::F32CopySign);
                self.stack_height -= 1;
            }
            Instruction::F64Abs => {
                self.emit(Op::F64Abs);
            }
            Instruction::F64Neg => {
                self.emit(Op::F64Neg);
            }
            Instruction::F64Ceil => {
                self.emit(Op::F64Ceil);
            }
            Instruction::F64Floor => {
                self.emit(Op::F64Floor);
            }
            Instruction::F64Trunc => {
                self.emit(Op::F64Trunc);
            }
            Instruction::F64Nearest => {
                self.emit(Op::F64Nearest);
            }
            Instruction::F64Sqrt => {
                self.emit(Op::F64Sqrt);
            }
            Instruction::F64Add => {
                self.emit(Op::F64Add);
                self.stack_height -= 1;
            }
            Instruction::F64Sub => {
                self.emit(Op::F64Sub);
                self.stack_height -= 1;
            }
            Instruction::F64Mul => {
                self.emit(Op::F64Mul);
                self.stack_height -= 1;
            }
            Instruction::F64Div => {
                self.emit(Op::F64Div);
                self.stack_height -= 1;
            }
            Instruction::F64Min => {
                self.emit(Op::F64Min);
                self.stack_height -= 1;
            }
            Instruction::F64Max => {
                self.emit(Op::F64Max);
                self.stack_height -= 1;
            }
            Instruction::F64CopySign => {
                self.emit(Op::F64CopySign);
                self.stack_height -= 1;
            }
            Instruction::I32WrapI64 => {
                self.emit(Op::I32WrapI64);
            }
            Instruction::I32TruncF32Signed => {
                self.emit(Op::I32TruncF32Signed);
            }
            Instruction::I32TruncF32Unsigned => {
                self.emit(Op::I32TruncF32Unsigned);
            }
            Instruction::I32TruncF64Signed => {
                self.emit(Op::I32TruncF64Signed);
            }
            Instruction::I32TruncF64Unsigned => {
                self.emit(Op::I32TruncF64Unsigned);
            }
            Instruction::I64ExtendI32Signed => {
                self.emit(Op::I64ExtendI32Signed);
            }
            Instruction::I64ExtendI32Unsigned => {
                self.emit(Op::I64ExtendI32Unsigned);
            }
            Instruction::I64TruncF32Signed => {
                self.emit(Op::I64TruncF32Signed);
            }
            Instruction::I64TruncF32Unsigned => {
                self.emit(Op::I64TruncF32Unsigned);
            }
            Instruction::I64TruncF64Signed => {
                self.emit(Op::I64TruncF64Signed);
            }
            Instruction::I64TruncF64Unsigned => {
                self.emit(Op::I64TruncF64Unsigned);
            }
            Instruction::F32ConvertI32Signed => {
                self.emit(Op::F32ConvertI32Signed);
            }
            Instruction::F32ConvertI32Unsigned => {
                self.emit(Op::F32ConvertI32Unsigned);
            }
            Instruction::F32ConvertI64Signed => {
                self.emit(Op::F32ConvertI64Signed);
            }
            Instruction::F32ConvertI64Unsigned => {
                self.emit(Op::F32ConvertI64Unsigned);
            }
            Instruction::F32DemoteF64 => {
                self.emit(Op::F32DemoteF64);
            }
            Instruction::F64ConvertI32Signed => {
                self.emit(Op::F64ConvertI32Signed);
            }
            Instruction::F64ConvertI32Unsigned => {
                self.emit(Op::F64ConvertI32Unsigned);
            }
            Instruction::F64ConvertI64Signed => {
                self.emit(Op::F64ConvertI64Signed);
            }
            Instruction::F64ConvertI64Unsigned => {
                self.emit(Op::F64ConvertI64Unsigned);
            }
            Instruction::F64PromoteF32 => {
                self.emit(Op::F64PromoteF32);
            }
            Instruction::I32ReinterpretF32 => {
                self.emit(Op::I32ReinterpretF32);
            }
            Instruction::I64ReinterpretF64 => {
                self.emit(Op::I64ReinterpretF64);
            }
            Instruction::F32ReinterpretI32 => {
                self.emit(Op::F32ReinterpretI32);
            }
            Instruction::F64ReinterpretI64 => {
                self.emit(Op::F64ReinterpretI64);
            }
            Instruction::I32Extend8Signed => {
                self.emit(Op::I32Extend8Signed);
            }
            Instruction::I32Extend16Signed => {
                self.emit(Op::I32Extend16Signed);
            }
            Instruction::I64Extend8Signed => {
                self.emit(Op::I64Extend8Signed);
            }
            Instruction::I64Extend16Signed => {
                self.emit(Op::I64Extend16Signed);
            }
            Instruction::I64Extend32Signed => {
                self.emit(Op::I64Extend32Signed);
            }
            Instruction::I32TruncSaturatedF32Signed => {
                self.emit(Op::I32TruncSaturatedF32Signed);
            }
            Instruction::I32TruncSaturatedF32Unsigned => {
                self.emit(Op::I32TruncSaturatedF32Unsigned);
            }
            Instruction::I32TruncSaturatedF64Signed => {
                self.emit(Op::I32TruncSaturatedF64Signed);
            }
            Instruction::I32TruncSaturatedF64Unsigned => {
                self.emit(Op::I32TruncSaturatedF64Unsigned);
            }
            Instruction::I64TruncSaturatedF32Signed => {
                self.emit(Op::I64TruncSaturatedF32Signed);
            }
            Instruction::I64TruncSaturatedF32Unsigned => {
                self.emit(Op::I64TruncSaturatedF32Unsigned);
            }
            Instruction::I64TruncSaturatedF64Signed => {
                self.emit(Op::I64TruncSaturatedF64Signed);
            }
            Instruction::I64TruncSaturatedF64Unsigned => {
                self.emit(Op::I64TruncSaturatedF64Unsigned);
            }
            Instruction::V128Load(ma) => {
                self.emit(Op::V128Load {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::V128Load8x8Signed(ma) => {
                self.emit(Op::V128Load8x8Signed {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::V128Load8x8Unsigned(ma) => {
                self.emit(Op::V128Load8x8Unsigned {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::V128Load16x4Signed(ma) => {
                self.emit(Op::V128Load16x4Signed {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::V128Load16x4Unsigned(ma) => {
                self.emit(Op::V128Load16x4Unsigned {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::V128Load32x2Signed(ma) => {
                self.emit(Op::V128Load32x2Signed {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::V128Load32x2Unsigned(ma) => {
                self.emit(Op::V128Load32x2Unsigned {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::V128Load8Splat(ma) => {
                self.emit(Op::V128Load8Splat {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::V128Load16Splat(ma) => {
                self.emit(Op::V128Load16Splat {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::V128Load32Splat(ma) => {
                self.emit(Op::V128Load32Splat {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::V128Load64Splat(ma) => {
                self.emit(Op::V128Load64Splat {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::V128Load32Zero(ma) => {
                self.emit(Op::V128Load32Zero {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::V128Load64Zero(ma) => {
                self.emit(Op::V128Load64Zero {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
            }
            Instruction::V128Store(ma) => {
                self.emit(Op::V128Store {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                });
                self.stack_height -= 2;
            }
            Instruction::V128Load8Lane(ma, lane) => {
                self.emit(Op::V128Load8Lane {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                    lane: *lane,
                });
                self.stack_height -= 1;
            }
            Instruction::V128Load16Lane(ma, lane) => {
                self.emit(Op::V128Load16Lane {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                    lane: *lane,
                });
                self.stack_height -= 1;
            }
            Instruction::V128Load32Lane(ma, lane) => {
                self.emit(Op::V128Load32Lane {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                    lane: *lane,
                });
                self.stack_height -= 1;
            }
            Instruction::V128Load64Lane(ma, lane) => {
                self.emit(Op::V128Load64Lane {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                    lane: *lane,
                });
                self.stack_height -= 1;
            }
            Instruction::V128Store8Lane(ma, lane) => {
                self.emit(Op::V128Store8Lane {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                    lane: *lane,
                });
                self.stack_height -= 2;
            }
            Instruction::V128Store16Lane(ma, lane) => {
                self.emit(Op::V128Store16Lane {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                    lane: *lane,
                });
                self.stack_height -= 2;
            }
            Instruction::V128Store32Lane(ma, lane) => {
                self.emit(Op::V128Store32Lane {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                    lane: *lane,
                });
                self.stack_height -= 2;
            }
            Instruction::V128Store64Lane(ma, lane) => {
                self.emit(Op::V128Store64Lane {
                    offset: ma.offset as u32,
                    memory: ma.memory,
                    lane: *lane,
                });
                self.stack_height -= 2;
            }
            Instruction::I8x16Shuffle(mask) => {
                let table_idx = self.shuffle_masks.len() as u32;
                self.shuffle_masks.push(*mask);
                self.emit(Op::I8x16Shuffle { table_idx });
                self.stack_height -= 1;
            }
            Instruction::I8x16ExtractLaneSigned(l) => {
                self.emit(Op::I8x16ExtractLaneSigned(*l));
            }
            Instruction::I8x16ExtractLaneUnsigned(l) => {
                self.emit(Op::I8x16ExtractLaneUnsigned(*l));
            }
            Instruction::I16x8ExtractLaneSigned(l) => {
                self.emit(Op::I16x8ExtractLaneSigned(*l));
            }
            Instruction::I16x8ExtractLaneUnsigned(l) => {
                self.emit(Op::I16x8ExtractLaneUnsigned(*l));
            }
            Instruction::I32x4ExtractLane(l) => {
                self.emit(Op::I32x4ExtractLane(*l));
            }
            Instruction::I64x2ExtractLane(l) => {
                self.emit(Op::I64x2ExtractLane(*l));
            }
            Instruction::F32x4ExtractLane(l) => {
                self.emit(Op::F32x4ExtractLane(*l));
            }
            Instruction::F64x2ExtractLane(l) => {
                self.emit(Op::F64x2ExtractLane(*l));
            }
            Instruction::I8x16ReplaceLane(l) => {
                self.emit(Op::I8x16ReplaceLane(*l));
                self.stack_height -= 1;
            }
            Instruction::I16x8ReplaceLane(l) => {
                self.emit(Op::I16x8ReplaceLane(*l));
                self.stack_height -= 1;
            }
            Instruction::I32x4ReplaceLane(l) => {
                self.emit(Op::I32x4ReplaceLane(*l));
                self.stack_height -= 1;
            }
            Instruction::I64x2ReplaceLane(l) => {
                self.emit(Op::I64x2ReplaceLane(*l));
                self.stack_height -= 1;
            }
            Instruction::F32x4ReplaceLane(l) => {
                self.emit(Op::F32x4ReplaceLane(*l));
                self.stack_height -= 1;
            }
            Instruction::F64x2ReplaceLane(l) => {
                self.emit(Op::F64x2ReplaceLane(*l));
                self.stack_height -= 1;
            }
            Instruction::I8x16Swizzle => {
                self.emit(Op::I8x16Swizzle);
                self.stack_height -= 1;
            }
            Instruction::I8x16Splat => {
                self.emit(Op::I8x16Splat);
            }
            Instruction::I16x8Splat => {
                self.emit(Op::I16x8Splat);
            }
            Instruction::I32x4Splat => {
                self.emit(Op::I32x4Splat);
            }
            Instruction::I64x2Splat => {
                self.emit(Op::I64x2Splat);
            }
            Instruction::F32x4Splat => {
                self.emit(Op::F32x4Splat);
            }
            Instruction::F64x2Splat => {
                self.emit(Op::F64x2Splat);
            }
            Instruction::I8x16Eq => {
                self.emit(Op::I8x16Eq);
                self.stack_height -= 1;
            }
            Instruction::I8x16Ne => {
                self.emit(Op::I8x16Ne);
                self.stack_height -= 1;
            }
            Instruction::I8x16LtSigned => {
                self.emit(Op::I8x16LtSigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16LtUnsigned => {
                self.emit(Op::I8x16LtUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16GtSigned => {
                self.emit(Op::I8x16GtSigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16GtUnsigned => {
                self.emit(Op::I8x16GtUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16LeSigned => {
                self.emit(Op::I8x16LeSigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16LeUnsigned => {
                self.emit(Op::I8x16LeUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16GeSigned => {
                self.emit(Op::I8x16GeSigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16GeUnsigned => {
                self.emit(Op::I8x16GeUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8Eq => {
                self.emit(Op::I16x8Eq);
                self.stack_height -= 1;
            }
            Instruction::I16x8Ne => {
                self.emit(Op::I16x8Ne);
                self.stack_height -= 1;
            }
            Instruction::I16x8LtSigned => {
                self.emit(Op::I16x8LtSigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8LtUnsigned => {
                self.emit(Op::I16x8LtUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8GtSigned => {
                self.emit(Op::I16x8GtSigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8GtUnsigned => {
                self.emit(Op::I16x8GtUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8LeSigned => {
                self.emit(Op::I16x8LeSigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8LeUnsigned => {
                self.emit(Op::I16x8LeUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8GeSigned => {
                self.emit(Op::I16x8GeSigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8GeUnsigned => {
                self.emit(Op::I16x8GeUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4Eq => {
                self.emit(Op::I32x4Eq);
                self.stack_height -= 1;
            }
            Instruction::I32x4Ne => {
                self.emit(Op::I32x4Ne);
                self.stack_height -= 1;
            }
            Instruction::I32x4LtSigned => {
                self.emit(Op::I32x4LtSigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4LtUnsigned => {
                self.emit(Op::I32x4LtUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4GtSigned => {
                self.emit(Op::I32x4GtSigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4GtUnsigned => {
                self.emit(Op::I32x4GtUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4LeSigned => {
                self.emit(Op::I32x4LeSigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4LeUnsigned => {
                self.emit(Op::I32x4LeUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4GeSigned => {
                self.emit(Op::I32x4GeSigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4GeUnsigned => {
                self.emit(Op::I32x4GeUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I64x2Eq => {
                self.emit(Op::I64x2Eq);
                self.stack_height -= 1;
            }
            Instruction::I64x2Ne => {
                self.emit(Op::I64x2Ne);
                self.stack_height -= 1;
            }
            Instruction::I64x2LtSigned => {
                self.emit(Op::I64x2LtSigned);
                self.stack_height -= 1;
            }
            Instruction::I64x2GtSigned => {
                self.emit(Op::I64x2GtSigned);
                self.stack_height -= 1;
            }
            Instruction::I64x2LeSigned => {
                self.emit(Op::I64x2LeSigned);
                self.stack_height -= 1;
            }
            Instruction::I64x2GeSigned => {
                self.emit(Op::I64x2GeSigned);
                self.stack_height -= 1;
            }
            Instruction::F32X4Eq => {
                self.emit(Op::F32x4Eq);
                self.stack_height -= 1;
            }
            Instruction::F32x4Ne => {
                self.emit(Op::F32x4Ne);
                self.stack_height -= 1;
            }
            Instruction::F32x4Lt => {
                self.emit(Op::F32x4Lt);
                self.stack_height -= 1;
            }
            Instruction::F32x4Gt => {
                self.emit(Op::F32x4Gt);
                self.stack_height -= 1;
            }
            Instruction::F32x4Le => {
                self.emit(Op::F32x4Le);
                self.stack_height -= 1;
            }
            Instruction::F32x4Ge => {
                self.emit(Op::F32x4Ge);
                self.stack_height -= 1;
            }
            Instruction::F64x2Eq => {
                self.emit(Op::F64x2Eq);
                self.stack_height -= 1;
            }
            Instruction::F64x2Ne => {
                self.emit(Op::F64x2Ne);
                self.stack_height -= 1;
            }
            Instruction::F64x2Lt => {
                self.emit(Op::F64x2Lt);
                self.stack_height -= 1;
            }
            Instruction::F64x2Gt => {
                self.emit(Op::F64x2Gt);
                self.stack_height -= 1;
            }
            Instruction::F64x2Le => {
                self.emit(Op::F64x2Le);
                self.stack_height -= 1;
            }
            Instruction::F64x2Ge => {
                self.emit(Op::F64x2Ge);
                self.stack_height -= 1;
            }
            Instruction::V128Not => {
                self.emit(Op::V128Not);
            }
            Instruction::V128And => {
                self.emit(Op::V128And);
                self.stack_height -= 1;
            }
            Instruction::V128AndNot => {
                self.emit(Op::V128AndNot);
                self.stack_height -= 1;
            }
            Instruction::V128Or => {
                self.emit(Op::V128Or);
                self.stack_height -= 1;
            }
            Instruction::V128Xor => {
                self.emit(Op::V128Xor);
                self.stack_height -= 1;
            }
            Instruction::V128BitSelect => {
                self.emit(Op::V128BitSelect);
                self.stack_height -= 2;
            }
            Instruction::V128AnyTrue => {
                self.emit(Op::V128AnyTrue);
            }
            Instruction::I8x16Abs => {
                self.emit(Op::I8x16Abs);
            }
            Instruction::I8x16Neg => {
                self.emit(Op::I8x16Neg);
            }
            Instruction::I8x16PopCount => {
                self.emit(Op::I8x16PopCount);
            }
            Instruction::I8x16AllTrue => {
                self.emit(Op::I8x16AllTrue);
            }
            Instruction::I8x16BitMask => {
                self.emit(Op::I8x16BitMask);
            }
            Instruction::I8x16NarrowI16x8Signed => {
                self.emit(Op::I8x16NarrowI16x8Signed);
                self.stack_height -= 1;
            }
            Instruction::I8x16NarrowI16x8Unsigned => {
                self.emit(Op::I8x16NarrowI16x8Unsigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16Shl => {
                self.emit(Op::I8x16Shl);
                self.stack_height -= 1;
            }
            Instruction::I8x16ShrSigned => {
                self.emit(Op::I8x16ShrSigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16ShrUnsigned => {
                self.emit(Op::I8x16ShrUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16Add => {
                self.emit(Op::I8x16Add);
                self.stack_height -= 1;
            }
            Instruction::I8x16AddSaturatedSigned => {
                self.emit(Op::I8x16AddSaturatedSigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16AddSaturatedUnsigned => {
                self.emit(Op::I8x16AddSaturatedUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16Sub => {
                self.emit(Op::I8x16Sub);
                self.stack_height -= 1;
            }
            Instruction::I8x16SubSaturatedSigned => {
                self.emit(Op::I8x16SubSaturatedSigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16SubSaturatedUnsigned => {
                self.emit(Op::I8x16SubSaturatedUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16MinSigned => {
                self.emit(Op::I8x16MinSigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16MinUnsigned => {
                self.emit(Op::I8x16MinUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16MaxSigned => {
                self.emit(Op::I8x16MaxSigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16MaxUnsigned => {
                self.emit(Op::I8x16MaxUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I8x16AvgRangeUnsigned => {
                self.emit(Op::I8x16AvgRangeUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8ExtAddPairWiseI8x16Signed => {
                self.emit(Op::I16x8ExtAddPairWiseI8x16Signed);
            }
            Instruction::I16x8ExtAddPairWiseI8x16Unsigned => {
                self.emit(Op::I16x8ExtAddPairWiseI8x16Unsigned);
            }
            Instruction::I16x8Abs => {
                self.emit(Op::I16x8Abs);
            }
            Instruction::I16x8Neg => {
                self.emit(Op::I16x8Neg);
            }
            Instruction::I16xQ15MulRangeSaturatedSigned => {
                self.emit(Op::I16xQ15MulRangeSaturatedSigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8AllTrue => {
                self.emit(Op::I16x8AllTrue);
            }
            Instruction::I16x8BitMask => {
                self.emit(Op::I16x8BitMask);
            }
            Instruction::I16x8NarrowI32x4Signed => {
                self.emit(Op::I16x8NarrowI32x4Signed);
                self.stack_height -= 1;
            }
            Instruction::I16x8NarrowI32x4Unsigned => {
                self.emit(Op::I16x8NarrowI32x4Unsigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8ExtendLowI8x16Unsigned => {
                self.emit(Op::I16x8ExtendLowI8x16Unsigned);
            }
            Instruction::I16x8ExtendHighI8x16Unsigned => {
                self.emit(Op::I16x8ExtendHighI8x16Unsigned);
            }
            Instruction::I16x8ExtendLowI8x16Signed => {
                self.emit(Op::I16x8ExtendLowI8x16Signed);
            }
            Instruction::I16x8ExtendHighI8x16Signed => {
                self.emit(Op::I16x8ExtendHighI8x16Signed);
            }
            Instruction::I16x8Shl => {
                self.emit(Op::I16x8Shl);
                self.stack_height -= 1;
            }
            Instruction::I16x8ShrSigned => {
                self.emit(Op::I16x8ShrSigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8ShrUnsigned => {
                self.emit(Op::I16x8ShrUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8Add => {
                self.emit(Op::I16x8Add);
                self.stack_height -= 1;
            }
            Instruction::I16x8AddSaturatedSigned => {
                self.emit(Op::I16x8AddSaturatedSigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8AddSaturatedUnsigned => {
                self.emit(Op::I16x8AddSaturatedUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8Sub => {
                self.emit(Op::I16x8Sub);
                self.stack_height -= 1;
            }
            Instruction::I16x8SubSaturatedSigned => {
                self.emit(Op::I16x8SubSaturatedSigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8SubSaturatedUnsigned => {
                self.emit(Op::I16x8SubSaturatedUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8Mul => {
                self.emit(Op::I16x8Mul);
                self.stack_height -= 1;
            }
            Instruction::I16x8MinSigned => {
                self.emit(Op::I16x8MinSigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8MinUnsigned => {
                self.emit(Op::I16x8MinUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8MaxSigned => {
                self.emit(Op::I16x8MaxSigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8MaxUnsigned => {
                self.emit(Op::I16x8MaxUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8AvgRangeUnsigned => {
                self.emit(Op::I16x8AvgRangeUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8ExtMulLowI8x16Signed => {
                self.emit(Op::I16x8ExtMulLowI8x16Signed);
                self.stack_height -= 1;
            }
            Instruction::I16x8ExtMulHighI8x16Signed => {
                self.emit(Op::I16x8ExtMulHighI8x16Signed);
                self.stack_height -= 1;
            }
            Instruction::I16x8ExtMulLowI8x16Unsigned => {
                self.emit(Op::I16x8ExtMulLowI8x16Unsigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8ExtMulHighI8x16Unsigned => {
                self.emit(Op::I16x8ExtMulHighI8x16Unsigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4ExtAddPairWiseI16x8Signed => {
                self.emit(Op::I32x4ExtAddPairWiseI16x8Signed);
            }
            Instruction::I32x4ExtAddPairWiseI16x8Unsigned => {
                self.emit(Op::I32x4ExtAddPairWiseI16x8Unsigned);
            }
            Instruction::I32x4Abs => {
                self.emit(Op::I32x4Abs);
            }
            Instruction::I32x4Neg => {
                self.emit(Op::I32x4Neg);
            }
            Instruction::I32x4AllTrue => {
                self.emit(Op::I32x4AllTrue);
            }
            Instruction::I32x4BitMask => {
                self.emit(Op::I32x4BitMask);
            }
            Instruction::I32x4ExtendLowI16x8Signed => {
                self.emit(Op::I32x4ExtendLowI16x8Signed);
            }
            Instruction::I32x4ExtendHighI16x8Signed => {
                self.emit(Op::I32x4ExtendHighI16x8Signed);
            }
            Instruction::I32x4ExtendLowI16x8Unsigned => {
                self.emit(Op::I32x4ExtendLowI16x8Unsigned);
            }
            Instruction::I32x4ExtendHighI16x8Unsigned => {
                self.emit(Op::I32x4ExtendHighI16x8Unsigned);
            }
            Instruction::I32x4Shl => {
                self.emit(Op::I32x4Shl);
                self.stack_height -= 1;
            }
            Instruction::I32x4ShrSigned => {
                self.emit(Op::I32x4ShrSigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4ShrUnsigned => {
                self.emit(Op::I32x4ShrUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4Add => {
                self.emit(Op::I32x4Add);
                self.stack_height -= 1;
            }
            Instruction::I32x4Sub => {
                self.emit(Op::I32x4Sub);
                self.stack_height -= 1;
            }
            Instruction::I32x4Mul => {
                self.emit(Op::I32x4Mul);
                self.stack_height -= 1;
            }
            Instruction::I32x4MinSigned => {
                self.emit(Op::I32x4MinSigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4MinUnsigned => {
                self.emit(Op::I32x4MinUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4MaxSigned => {
                self.emit(Op::I32x4MaxSigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4MaxUnsigned => {
                self.emit(Op::I32x4MaxUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4DotI16x8Signed => {
                self.emit(Op::I32x4DotI16x8Signed);
                self.stack_height -= 1;
            }
            Instruction::I32x4ExtMulLowI16x8Signed => {
                self.emit(Op::I32x4ExtMulLowI16x8Signed);
                self.stack_height -= 1;
            }
            Instruction::I32x4ExtMulHighI16x8Signed => {
                self.emit(Op::I32x4ExtMulHighI16x8Signed);
                self.stack_height -= 1;
            }
            Instruction::I32x4ExtMulLowI16x8Unsigned => {
                self.emit(Op::I32x4ExtMulLowI16x8Unsigned);
                self.stack_height -= 1;
            }
            Instruction::I32x4ExtMulHighI16x8Unsigned => {
                self.emit(Op::I32x4ExtMulHighI16x8Unsigned);
                self.stack_height -= 1;
            }
            Instruction::I64x2Abs => {
                self.emit(Op::I64x2Abs);
            }
            Instruction::I64x2Neg => {
                self.emit(Op::I64x2Neg);
            }
            Instruction::I64x2AllTrue => {
                self.emit(Op::I64x2AllTrue);
            }
            Instruction::I64x2BitMask => {
                self.emit(Op::I64x2BitMask);
            }
            Instruction::I64x2ExtendLowI32x4Signed => {
                self.emit(Op::I64x2ExtendLowI32x4Signed);
            }
            Instruction::I64x2ExtendHighI32x4Signed => {
                self.emit(Op::I64x2ExtendHighI32x4Signed);
            }
            Instruction::I64x2ExtendLowI32x4Unsigned => {
                self.emit(Op::I64x2ExtendLowI32x4Unsigned);
            }
            Instruction::I64x2ExtendHighI32x4Unsigned => {
                self.emit(Op::I64x2ExtendHighI32x4Unsigned);
            }
            Instruction::I64x2Shl => {
                self.emit(Op::I64x2Shl);
                self.stack_height -= 1;
            }
            Instruction::I64x2ShrSigned => {
                self.emit(Op::I64x2ShrSigned);
                self.stack_height -= 1;
            }
            Instruction::I64x2ShrUnsigned => {
                self.emit(Op::I64x2ShrUnsigned);
                self.stack_height -= 1;
            }
            Instruction::I64x2Add => {
                self.emit(Op::I64x2Add);
                self.stack_height -= 1;
            }
            Instruction::I64x2Sub => {
                self.emit(Op::I64x2Sub);
                self.stack_height -= 1;
            }
            Instruction::I64x2Mul => {
                self.emit(Op::I64x2Mul);
                self.stack_height -= 1;
            }
            Instruction::I64x2ExtMulLowI32x4Signed => {
                self.emit(Op::I64x2ExtMulLowI32x4Signed);
                self.stack_height -= 1;
            }
            Instruction::I64x2ExtMulHighI32x4Signed => {
                self.emit(Op::I64x2ExtMulHighI32x4Signed);
                self.stack_height -= 1;
            }
            Instruction::I64x2ExtMulLowI32x4Unsigned => {
                self.emit(Op::I64x2ExtMulLowI32x4Unsigned);
                self.stack_height -= 1;
            }
            Instruction::I64x2ExtMulHighI32x4Unsigned => {
                self.emit(Op::I64x2ExtMulHighI32x4Unsigned);
                self.stack_height -= 1;
            }
            Instruction::F32x4Ceil => {
                self.emit(Op::F32x4Ceil);
            }
            Instruction::F32x4Floor => {
                self.emit(Op::F32x4Floor);
            }
            Instruction::F32x4Trunc => {
                self.emit(Op::F32x4Trunc);
            }
            Instruction::F32x4Nearest => {
                self.emit(Op::F32x4Nearest);
            }
            Instruction::F32x4Abs => {
                self.emit(Op::F32x4Abs);
            }
            Instruction::F32x4Neg => {
                self.emit(Op::F32x4Neg);
            }
            Instruction::F32x4Sqrt => {
                self.emit(Op::F32x4Sqrt);
            }
            Instruction::F32x4Add => {
                self.emit(Op::F32x4Add);
                self.stack_height -= 1;
            }
            Instruction::F32x4Sub => {
                self.emit(Op::F32x4Sub);
                self.stack_height -= 1;
            }
            Instruction::F32x4Mul => {
                self.emit(Op::F32x4Mul);
                self.stack_height -= 1;
            }
            Instruction::F32x4Div => {
                self.emit(Op::F32x4Div);
                self.stack_height -= 1;
            }
            Instruction::F32x4Min => {
                self.emit(Op::F32x4Min);
                self.stack_height -= 1;
            }
            Instruction::F32x4Max => {
                self.emit(Op::F32x4Max);
                self.stack_height -= 1;
            }
            Instruction::F32x4PMin => {
                self.emit(Op::F32x4PMin);
                self.stack_height -= 1;
            }
            Instruction::F32x4PMax => {
                self.emit(Op::F32x4PMax);
                self.stack_height -= 1;
            }
            Instruction::F64x2Ceil => {
                self.emit(Op::F64x2Ceil);
            }
            Instruction::F64x2Floor => {
                self.emit(Op::F64x2Floor);
            }
            Instruction::F64x2Trunc => {
                self.emit(Op::F64x2Trunc);
            }
            Instruction::F64x2Nearest => {
                self.emit(Op::F64x2Nearest);
            }
            Instruction::F64x2Abs => {
                self.emit(Op::F64x2Abs);
            }
            Instruction::F64x2Neg => {
                self.emit(Op::F64x2Neg);
            }
            Instruction::F64x2Sqrt => {
                self.emit(Op::F64x2Sqrt);
            }
            Instruction::F64x2Add => {
                self.emit(Op::F64x2Add);
                self.stack_height -= 1;
            }
            Instruction::F64x2Sub => {
                self.emit(Op::F64x2Sub);
                self.stack_height -= 1;
            }
            Instruction::F64x2Mul => {
                self.emit(Op::F64x2Mul);
                self.stack_height -= 1;
            }
            Instruction::F64x2Div => {
                self.emit(Op::F64x2Div);
                self.stack_height -= 1;
            }
            Instruction::F64x2Min => {
                self.emit(Op::F64x2Min);
                self.stack_height -= 1;
            }
            Instruction::F64x2Max => {
                self.emit(Op::F64x2Max);
                self.stack_height -= 1;
            }
            Instruction::F64x2PMin => {
                self.emit(Op::F64x2PMin);
                self.stack_height -= 1;
            }
            Instruction::F64x2PMax => {
                self.emit(Op::F64x2PMax);
                self.stack_height -= 1;
            }
            Instruction::I32x4TruncSaturatedF32x4Signed => {
                self.emit(Op::I32x4TruncSaturatedF32x4Signed);
            }
            Instruction::I32x4TruncSaturatedF32x4Unsigned => {
                self.emit(Op::I32x4TruncSaturatedF32x4Unsigned);
            }
            Instruction::F32x4ConvertI32x4Signed => {
                self.emit(Op::F32x4ConvertI32x4Signed);
            }
            Instruction::F32x4ConvertI32x4Unsigned => {
                self.emit(Op::F32x4ConvertI32x4Unsigned);
            }
            Instruction::I32x4TruncSaturatedF64x2SignedZero => {
                self.emit(Op::I32x4TruncSaturatedF64x2SignedZero);
            }
            Instruction::I32x4TruncSaturatedF64x2UnsignedZero => {
                self.emit(Op::I32x4TruncSaturatedF64x2UnsignedZero);
            }
            Instruction::F64x2ConvertLowI32x4Signed => {
                self.emit(Op::F64x2ConvertLowI32x4Signed);
            }
            Instruction::F64x2ConvertLowI32x4Unsigned => {
                self.emit(Op::F64x2ConvertLowI32x4Unsigned);
            }
            Instruction::F32x4DemoteF64x2Zero => {
                self.emit(Op::F32x4DemoteF64x2Zero);
            }
            Instruction::F64xPromoteLowF32x4 => {
                self.emit(Op::F64x2PromoteLowF32x4);
            }
            Instruction::I8x16RelaxedSwizzle => {
                self.emit(Op::I8x16RelaxedSwizzle);
                self.stack_height -= 1;
            }
            Instruction::I32x4RelaxedTruncF32x4Signed => {
                self.emit(Op::I32x4RelaxedTruncF32x4Signed);
            }
            Instruction::I32x4RelaxedTruncF32x4Unsigned => {
                self.emit(Op::I32x4RelaxedTruncF32x4Unsigned);
            }
            Instruction::I32x4RelaxedTruncF64x2SignedZero => {
                self.emit(Op::I32x4RelaxedTruncF64x2SignedZero);
            }
            Instruction::I32x4RelaxedTruncF64x2UnsignedZero => {
                self.emit(Op::I32x4RelaxedTruncF64x2UnsignedZero);
            }
            Instruction::F32x4RelaxedMadd => {
                self.emit(Op::F32x4RelaxedMadd);
                self.stack_height -= 2;
            }
            Instruction::F32x4RelaxedNmadd => {
                self.emit(Op::F32x4RelaxedNmadd);
                self.stack_height -= 2;
            }
            Instruction::F64x2RelaxedMadd => {
                self.emit(Op::F64x2RelaxedMadd);
                self.stack_height -= 2;
            }
            Instruction::F64x2RelaxedNmadd => {
                self.emit(Op::F64x2RelaxedNmadd);
                self.stack_height -= 2;
            }
            Instruction::I8x16RelaxedLaneselect => {
                self.emit(Op::I8x16RelaxedLaneselect);
                self.stack_height -= 2;
            }
            Instruction::I16x8RelaxedLaneselect => {
                self.emit(Op::I16x8RelaxedLaneselect);
                self.stack_height -= 2;
            }
            Instruction::I32x4RelaxedLaneselect => {
                self.emit(Op::I32x4RelaxedLaneselect);
                self.stack_height -= 2;
            }
            Instruction::I64x2RelaxedLaneselect => {
                self.emit(Op::I64x2RelaxedLaneselect);
                self.stack_height -= 2;
            }
            Instruction::F32x4RelaxedMin => {
                self.emit(Op::F32x4RelaxedMin);
                self.stack_height -= 1;
            }
            Instruction::F32x4RelaxedMax => {
                self.emit(Op::F32x4RelaxedMax);
                self.stack_height -= 1;
            }
            Instruction::F64x2RelaxedMin => {
                self.emit(Op::F64x2RelaxedMin);
                self.stack_height -= 1;
            }
            Instruction::F64x2RelaxedMax => {
                self.emit(Op::F64x2RelaxedMax);
                self.stack_height -= 1;
            }
            Instruction::I16x8RelaxedQ15mulrSigned => {
                self.emit(Op::I16x8RelaxedQ15mulrSigned);
                self.stack_height -= 1;
            }
            Instruction::I16x8RelaxedDotI8x16I7x16Signed => {
                self.emit(Op::I16x8RelaxedDotI8x16I7x16Signed);
                self.stack_height -= 1;
            }
            Instruction::I32x4RelaxedDotI8x16I7x16AddSigned => {
                self.emit(Op::I32x4RelaxedDotI8x16I7x16AddSigned);
                self.stack_height -= 2;
            }
            Instruction::TryTable(bt, catches, body) => {
                let (m, n) = self.resolve_block_type(bt);

                let entry = ((self.stack_height != UNREACHABLE_DEPTH) as i32)
                    .wrapping_mul(self.stack_height.wrapping_sub(m as i32));

                let start_label = self.next_label();
                let end_label = self.next_label();

                self.emit_label(start_label);

                self.block_stack.push(BlockContext {
                    kind: BlockKind::Block,
                    entry_stack_height: entry,
                    branch_arity: n,
                    start_label,
                    end_label,
                });

                let clauses = self.compile_catch_clauses(catches, entry);
                let handler_idx = self.catch_handlers.len() as u32;
                self.catch_handlers.push(clauses);

                self.emit(Op::TryCatchPush { handler_idx });

                for i in body {
                    self.compile_instruction(i);
                }

                self.emit(Op::TryCatchPop);
                self.block_stack.pop().expect("we push right before");
                self.emit_label(end_label);
                self.stack_height = entry + n as i32;
            }
            Instruction::StructNew(..) => todo!(),
            Instruction::StructNewDefault(..) => todo!(),
            Instruction::StructGet(..) => todo!(),
            Instruction::StructGetSigned(..) => todo!(),
            Instruction::StructGetUnsigned(..) => todo!(),
            Instruction::StructSet(..) => todo!(),
            Instruction::ArrayNew(..) => todo!(),
            Instruction::ArrayNewDefault(..) => todo!(),
            Instruction::ArrayNewFixed(..) => todo!(),
            Instruction::ArrayNewData(..) => todo!(),
            Instruction::ArrayNewElem(..) => todo!(),
            Instruction::ArrayGet(..) => todo!(),
            Instruction::ArrayGetSigned(..) => todo!(),
            Instruction::ArrayGetUnsigned(..) => todo!(),
            Instruction::ArraySet(..) => todo!(),
            Instruction::ArrayLen => todo!(),
            Instruction::ArrayFill(..) => todo!(),
            Instruction::ArrayCopy(..) => todo!(),
            Instruction::ArrayInitData(..) => todo!(),
            Instruction::ArrayInitElem(..) => todo!(),
            Instruction::RefTest(..) => todo!(),
            Instruction::RefTestNull(..) => todo!(),
            Instruction::RefCast(..) => todo!(),
            Instruction::RefCastNull(..) => todo!(),
            Instruction::BrOnCast(..) => todo!(),
            Instruction::BrOnCastFail(..) => todo!(),
            Instruction::AnyConvertExtern => todo!(),
            Instruction::ExternConvertAny => todo!(),
            Instruction::RefI31 => todo!(),
            Instruction::I31GetSigned => todo!(),
            Instruction::I31GetUnsigned => todo!(),
        }

        self.max_stack_height = self.max_stack_height.max(self.stack_height);
    }
}

#[cfg(all(test, not(any(feature = "core-tests", feature = "component-tests"))))]
mod tests {
    use super::*;
    use crate::binary_grammar::{
        BlockType, CompositeType, FunctionType, Instruction, MemArg, ResultType, SubType, ValueType,
    };

    fn i32_func_type() -> SubType {
        SubType {
            is_final: true,
            supertypes: vec![],
            composite_type: CompositeType::Func(FunctionType(
                ResultType(vec![ValueType::I32, ValueType::I32]),
                ResultType(vec![ValueType::I32]),
            )),
        }
    }

    fn make_func(type_index: u32, body: Vec<Instruction>) -> Function {
        Function {
            type_index,
            locals: vec![],
            body,
        }
    }

    fn compile_ops(body: Vec<Instruction>) -> Vec<Op> {
        let types = vec![i32_func_type()];
        Compiler::compile_and_get_ops(&types, &make_func(0, body), CompilerMode::Optimize)
    }

    #[test]
    fn fuse_local_get2() {
        let ops = compile_ops(vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Add,
        ]);
        insta::assert_debug_snapshot!(&ops, @r#"
        [
            LocalGet2 {
                local_idx_a: 0,
                local_idx_b: 1,
            },
            I32Add,
            Return,
        ]
        "#);
    }

    #[test]
    fn fuse_i32_eq_jump_if() {
        let ops = compile_ops(vec![Instruction::Block(
            BlockType::Empty,
            vec![
                Instruction::LocalGet(0),
                Instruction::LocalGet(1),
                Instruction::I32Eq,
                Instruction::BrIf(0),
            ],
        )]);
        insta::assert_debug_snapshot!(&ops, @r#"
        [
            LocalGet2 {
                local_idx_a: 0,
                local_idx_b: 1,
            },
            I32EqJumpIf {
                target: 2,
                keep: 0,
                drop: 0,
            },
            Return,
        ]
        "#);
    }

    #[test]
    fn fuse_i32_lt_signed_jump_if() {
        let ops = compile_ops(vec![Instruction::Block(
            BlockType::Empty,
            vec![
                Instruction::LocalGet(0),
                Instruction::LocalGet(1),
                Instruction::I32LtSigned,
                Instruction::BrIf(0),
            ],
        )]);
        insta::assert_debug_snapshot!(&ops, @r#"
        [
            LocalGet2 {
                local_idx_a: 0,
                local_idx_b: 1,
            },
            I32LtSignedJumpIf {
                target: 2,
                keep: 0,
                drop: 0,
            },
            Return,
        ]
        "#);
    }

    #[test]
    fn fuse_i32_eqz_jump_if() {
        let ops = compile_ops(vec![Instruction::Block(
            BlockType::Empty,
            vec![
                Instruction::LocalGet(0),
                Instruction::I32EqZero,
                Instruction::BrIf(0),
            ],
        )]);
        insta::assert_debug_snapshot!(&ops, @r#"
        [
            LocalGet {
                local_idx: 0,
            },
            I32EqZeroJumpIf {
                target: 2,
                keep: 0,
                drop: 0,
            },
            Return,
        ]
        "#);
    }

    #[test]
    fn no_fuse_when_jump_target_between() {
        let ops = compile_ops(vec![Instruction::Block(
            BlockType::Empty,
            vec![
                Instruction::Block(
                    BlockType::SingleValue(ValueType::I32),
                    vec![
                        Instruction::Br(0),
                        Instruction::LocalGet(0),
                        Instruction::LocalGet(1),
                        Instruction::I32Eq,
                    ],
                ),
                Instruction::BrIf(0),
            ],
        )]);
        insta::assert_debug_snapshot!(&ops, @r#"
        [
            Jump {
                target: 1,
                keep: 1,
                drop: 65535,
            },
            JumpIf {
                target: 2,
                keep: 0,
                drop: 0,
            },
            Return,
        ]
        "#);
    }

    #[test]
    fn fuse_local_get_i32_load() {
        let ops = compile_ops(vec![
            Instruction::LocalGet(0),
            Instruction::I32Load(MemArg {
                align: 2,
                offset: 8,
                memory: 0,
            }),
        ]);
        insta::assert_debug_snapshot!(&ops, @r#"
        [
            LocalGetI32Load {
                local_idx: 0,
                offset: 8,
                memory: 0,
            },
            Return,
        ]
        "#);
    }

    #[test]
    fn fuse_local_get_i32_store() {
        let ops = compile_ops(vec![
            Instruction::I32Const(42),
            Instruction::LocalGet(0),
            Instruction::I32Store(MemArg {
                align: 2,
                offset: 4,
                memory: 0,
            }),
        ]);
        insta::assert_debug_snapshot!(&ops, @r#"
        [
            I32Const {
                value: 42,
            },
            LocalGetI32Store {
                local_idx: 0,
                offset: 4,
                memory: 0,
            },
            Return,
        ]
        "#);
    }

    #[test]
    fn fuse_local_get_local_set() {
        let ops = compile_ops(vec![Instruction::LocalGet(0), Instruction::LocalSet(1)]);
        insta::assert_debug_snapshot!(&ops, @r#"
        [
            LocalGetLocalSet {
                local_get_idx: 0,
                local_set_idx: 1,
            },
            Return,
        ]
        "#);
    }

    #[test]
    fn fuse_local_get_return() {
        let ops = compile_ops(vec![Instruction::LocalGet(0)]);
        insta::assert_debug_snapshot!(&ops, @r#"
        [
            LocalGetReturn {
                local_idx: 0,
            },
        ]
        "#);
    }
}
