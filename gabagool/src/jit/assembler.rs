use std::mem;

use crate::{
    ir::Op,
    jit::{stencil_for_op, CodeBuffer, OpImmediate, StencilFn},
};

#[derive(Debug)]
pub struct JitFunction {
    pub code: CodeBuffer,
    pub fn_table: Vec<StencilFn>,
    pub imm_table: Vec<OpImmediate>,
}

fn immediate_for_op(op: Op) -> OpImmediate {
    match op {
        Op::Nop => OpImmediate::default(),
        Op::I32Const { value } => OpImmediate {
            imm0: value as u64,
            imm1: 0,
        },
        Op::LocalGet { local_i: local_i_a }
        | Op::LocalSet { local_i: local_i_a }
        | Op::LocalTee { local_i: local_i_a } => OpImmediate {
            imm0: local_i_a as u64,
            imm1: 0,
        },
        Op::LocalGet2 {
            local_i_a,
            local_i_b,
        } => OpImmediate {
            imm0: local_i_a as u64,
            imm1: local_i_b as u64,
        },
        Op::Jump { target, keep, drop }
        | Op::JumpIf { target, keep, drop }
        | Op::JumpIfNot { target, keep, drop } => OpImmediate {
            imm0: target as u64,
            imm1: ((keep as u64) << 32) | (drop as u64),
        },
        Op::Return => OpImmediate::default(),
        _ => OpImmediate::default(),
    }
}

pub fn assemble(ops: &[Op]) -> Option<JitFunction> {
    // todo: get rid of me when i implement it all
    if !ops.iter().all(|op| stencil_for_op(op).is_some()) {
        let unsupported_ops = ops.iter().filter(|op| stencil_for_op(op).is_none());

        panic!(
            "not all ops have stencils. unsupported ops: {:?}",
            unsupported_ops
        );
    }

    let op_stencil_bytes = ops
        .iter()
        .map(|op| stencil_for_op(op).expect("validation above"))
        .collect::<Vec<_>>();

    let mut total_size = 0;
    let pc_to_offset = op_stencil_bytes
        .iter()
        .map(|op| {
            let offset = total_size;
            total_size += op.len();
            offset
        })
        .collect::<Vec<_>>();

    let mut code = CodeBuffer::new(total_size.max(4_096));
    for stencil_op in &op_stencil_bytes {
        code.emit(stencil_op);
    }

    code.make_executable();

    let fn_table = pc_to_offset
        .iter()
        .map(|&offset| unsafe { mem::transmute(code.as_ptr().add(offset)) })
        .collect::<Vec<_>>();

    let imm_table = ops.iter().cloned().map(immediate_for_op).collect();

    Some(JitFunction {
        code,
        fn_table,
        imm_table,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_simple_program() {
        let ops = vec![Op::I32Const { value: 42 }, Op::Return];
        let jit_fn = assemble(&ops);

        assert!(jit_fn.is_some());
    }
}
