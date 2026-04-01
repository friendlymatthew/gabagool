use std::mem;

use crate::{
    ir::Op,
    jit::{CodeBuffer, OpImmediate, StencilFn, STENCIL_I32_CONST, STENCIL_NOP, STENCIL_RETURN_},
};

#[derive(Debug)]
pub struct JitFunction {
    pub code: CodeBuffer,
    pub fn_table: Vec<StencilFn>,
    pub imm_table: Vec<OpImmediate>,
}

const fn stencil_for_op(op: &Op) -> Option<&'static [u8]> {
    match op {
        Op::Nop => Some(STENCIL_NOP),
        Op::I32Const { .. } => Some(STENCIL_I32_CONST),
        Op::Return => Some(STENCIL_RETURN_),
        _ => None,
    }
}

fn immediate_for_op(op: &Op) -> OpImmediate {
    match op {
        Op::Nop => OpImmediate::default(),
        Op::I32Const { value } => OpImmediate {
            imm0: *value as u64,
            imm1: 0,
        },
        Op::Return => OpImmediate::default(),
        _ => OpImmediate::default(),
    }
}

pub fn assemble(ops: &[Op]) -> Option<JitFunction> {
    // todo: get rid of me when i implement it all
    if !ops.iter().all(|op| stencil_for_op(op).is_some()) {
        return None;
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

    let imm_table = ops.iter().map(immediate_for_op).collect();

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
