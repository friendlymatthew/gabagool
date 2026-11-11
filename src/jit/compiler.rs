use crate::{ensure_jit, ir::Op, prelude::*, Error, Result};

use super::{code_buffer::CodeBuffer, code_layout::CodeLayout, StencilInstance, STENCILS};

const fn find_stencil(op: Op) -> Option<&'static str> {
    Some(match op {
        Op::I32Const { .. } => "_i32_const",
        Op::LocalGet { .. } => "_local_get",
        Op::LocalSet { .. } => "_local_set",
        Op::LocalTee { .. } => "_local_tee",
        Op::LocalGet2 { .. } => "_local_get2",
        Op::Return => "_return_value",
        Op::I32Add => "_i32_add",
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
        let name = find_stencil(op)
            .ok_or_else(|| Error::jit(format!("unsupported operation at IR {pc}: {op:?}")))?;

        let parameters = match op {
            Op::I32Const { value } => {
                stack_height += 1;

                vec![u64::from(value as u32)]
            }
            Op::LocalGet { local_i } => {
                required_locals = required_locals.max(u64::from(local_i) + 1);
                stack_height += 1;

                vec![u64::from(local_i)]
            }
            Op::LocalSet { local_i } => {
                ensure_jit!(
                    stack_height >= 1,
                    "stack underflow at IR {pc}: LocalSet needs one value"
                );

                required_locals = required_locals.max(u64::from(local_i) + 1);
                stack_height -= 1;

                vec![u64::from(local_i)]
            }
            Op::LocalTee { local_i } => {
                ensure_jit!(
                    stack_height >= 1,
                    "stack underflow at IR {pc}: LocalTee needs one value"
                );

                required_locals = required_locals.max(u64::from(local_i) + 1);

                vec![u64::from(local_i)]
            }
            Op::LocalGet2 {
                local_i_a,
                local_i_b,
            } => {
                required_locals = required_locals.max(u64::from(local_i_a.max(local_i_b)) + 1);
                stack_height += 2;
                vec![u64::from(local_i_a), u64::from(local_i_b)]
            }
            Op::I32Add => {
                ensure_jit!(
                    stack_height >= 2,
                    "stack underflow at IR {pc}: I32Add needs two values"
                );

                stack_height -= 1;
                Vec::new()
            }
            Op::Return => {
                ensure_jit!(
                    pc + 1 == ops.len(),
                    "instructions after Return at IR {pc} are not supported"
                );
                Vec::new()
            }
            _ => unreachable!("unsupported operations were rejected above"),
        };

        max_stack_height = max_stack_height.max(stack_height);

        let stencil = STENCILS
            .iter()
            .find(|stencil| stencil.name == name)
            .ok_or_else(|| Error::jit(format!("missing stencil {name} at IR {pc}")))?;

        instances.push(StencilInstance {
            stencil,
            parameters,
        });
    }

    let layout = CodeLayout::try_new(&instances)?;

    Ok(JitCompiledFunction {
        code: CodeBuffer::new(&layout)?,
        required_locals,
        max_stack_height,
        result_count: stack_height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_and_reads_local() {
        let ops = [
            Op::I32Const { value: 42 },
            Op::LocalSet { local_i: 3 },
            Op::LocalGet { local_i: 3 },
            Op::Return,
        ];
        assert!(supports(&ops));

        let function = compile(&ops).unwrap();

        assert_eq!(function.required_locals, 4);
        assert_eq!(function.max_stack_height, 1);

        let mut locals = [0; 4];
        let mut stack = [0; 1];

        assert_eq!(function.execute(&mut locals, &mut stack).unwrap(), 1);
        assert_eq!(locals, [0, 0, 0, 42]);
        assert_eq!(stack[0], 42);
    }

    #[test]
    fn compiles_and_executes_i32_constants() {
        for (a, b, expected) in [(20, 22, 42_u64), (-1, 1, 0), (i32::MIN, 0, 0x8000_0000)] {
            let ops = [
                Op::I32Const { value: a },
                Op::I32Const { value: b },
                Op::I32Add,
                Op::Return,
            ];
            assert!(supports(&ops));
            let function = compile(&ops).unwrap();
            let mut stack = [0_u64; 2];
            assert_eq!(function.execute(&mut [], &mut stack).unwrap(), 1);
            assert_eq!(stack[0], expected);
        }
    }
}
