use crate::{ensure_jit, ir::Op, prelude::*, Error, Result};

use super::{
    codebuffer::CodeBuffer, code_layout::CodeLayout, StencilInstance, StencilOperands, STENCILS,
};

const fn find_stencil(op: Op) -> Option<(&'static str, StencilOperands)> {
    Some(match op {
        Op::LocalGet { local_i } => ("_local_get", StencilOperands::LocalGet { local_i }),
        Op::LocalGet2 {
            local_i_a,
            local_i_b,
        } => (
            "_local_get2",
            StencilOperands::LocalGet2 {
                local_i_a,
                local_i_b,
            },
        ),
        Op::Return => ("_return_value", StencilOperands::None),
        Op::I32Add => ("_i32_add", StencilOperands::None),
        _ => return None,
    })
}

pub struct JitCompiledFunction {
    code: CodeBuffer,
    required_locals: u64,
    max_stack_height: usize,
    result_count: usize,
}

#[repr(C)]
struct VMContext {
    locals: *mut u64,
    stack: *mut u64,
    stack_height: u64,
}

impl JitCompiledFunction {
    pub const fn result_count(&self) -> usize {
        self.result_count
    }

    pub(crate) fn execute_vm(
        &self,
        locals: &mut [crate::RawValue],
        stack: &mut [crate::RawValue],
    ) -> Result<usize> {
        unsafe {
            let locals =
                core::slice::from_raw_parts_mut(locals.as_mut_ptr().cast::<u64>(), locals.len());

            let stack =
                core::slice::from_raw_parts_mut(stack.as_mut_ptr().cast::<u64>(), stack.len());

            self.execute(locals, stack)
        }
    }

    pub(super) fn execute(&self, locals: &mut [u64], stack: &mut [u64]) -> Result<usize> {
        ensure_jit!(
            locals.len() as u64 >= self.required_locals,
            "not enough locals"
        );

        ensure_jit!(
            stack.len() >= self.max_stack_height,
            "operand stack is too small"
        );

        let mut context = VMContext {
            locals: locals.as_mut_ptr(),
            stack: stack.as_mut_ptr(),
            stack_height: 0,
        };

        type Entry = unsafe extern "C" fn(*mut VMContext);

        unsafe {
            let entry: Entry = core::mem::transmute(self.code.as_ptr());
            entry(&mut context);
        }

        ensure_jit!(
            context.stack_height as usize == self.result_count,
            "unexpected result stack height"
        );

        Ok(self.result_count)
    }
}

pub fn supports(ops: &[Op]) -> bool {
    ops.iter().all(|&op| find_stencil(op).is_some())
}

pub fn compile(ops: &[Op]) -> Result<JitCompiledFunction> {
    ensure_jit!(
        matches!(ops.last(), Some(Op::Return)),
        "function must end with Return"
    );

    let mut instances = Vec::with_capacity(ops.len());

    let mut stack_height = 0_usize;
    let mut max_stack_height = 0;
    let mut required_locals = 0_u64;

    for (pc, &op) in ops.iter().enumerate() {
        let (name, operands) = find_stencil(op)
            .ok_or_else(|| Error::jit(format!("unsupported operation at IR {pc}: {op:?}")))?;

        match op {
            Op::LocalGet { local_i } => {
                required_locals = required_locals.max(u64::from(local_i) + 1);
                stack_height += 1;
            }
            Op::LocalGet2 {
                local_i_a,
                local_i_b,
            } => {
                required_locals = required_locals.max(u64::from(local_i_a.max(local_i_b)) + 1);
                stack_height += 2;
            }
            Op::I32Add => {
                ensure_jit!(
                    stack_height >= 2,
                    "stack underflow at IR {pc}: I32Add needs two values"
                );

                stack_height -= 1;
            }
            Op::Return => {
                ensure_jit!(
                    pc + 1 == ops.len(),
                    "instructions after Return at IR {pc} are not supported"
                );
            }
            _ => unreachable!("unsupported operations were rejected above"),
        }

        max_stack_height = max_stack_height.max(stack_height);

        let stencil = STENCILS
            .iter()
            .find(|stencil| stencil.name == name)
            .ok_or_else(|| Error::jit(format!("missing stencil {name} at IR {pc}")))?;

        instances.push(StencilInstance { stencil, operands });
    }

    let layout = CodeLayout::try_new(&instances)?;

    Ok(JitCompiledFunction {
        code: CodeBuffer::new(&layout)?,
        required_locals,
        max_stack_height,
        result_count: stack_height,
    })
}
