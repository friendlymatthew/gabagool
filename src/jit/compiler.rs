use crate::{bail_jit, bail_trap, ensure_jit, ir::Op, prelude::*, Error, Result, Trap};

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
        Op::I32Sub => "_i32_sub",
        Op::I32Mul => "_i32_mul",
        Op::I32DivSigned => "_i32_div_s",
        Op::I32DivUnsigned => "_i32_div_u",
        Op::I32RemainderSigned => "_i32_rem_s",
        Op::I32RemainderUnsigned => "_i32_rem_u",
        Op::I32EqZero => "_i32_eqz",
        Op::I32Eq => "_i32_eq",
        Op::I32Ne => "_i32_ne",
        Op::I32LtSigned => "_i32_lt_s",
        Op::I32LtUnsigned => "_i32_lt_u",
        Op::I32GtSigned => "_i32_gt_s",
        Op::I32GtUnsigned => "_i32_gt_u",
        Op::I32LeSigned => "_i32_le_s",
        Op::I32LeUnsigned => "_i32_le_u",
        Op::I32GeSigned => "_i32_ge_s",
        Op::I32GeUnsigned => "_i32_ge_u",
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
    trap: u64,
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
            trap: 0,
        };

        type Entry = unsafe extern "C" fn(*mut VMContext);

        unsafe {
            let entry: Entry = core::mem::transmute(self.code.as_ptr());
            entry(&mut context);
        }

        match context.trap {
            0 => {}
            1 => bail_trap!(Trap::IntegerDivideByZero),
            2 => bail_trap!(Trap::IntegerOverflow),
            code => bail_jit!("unknown trap code {code}"),
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
            Op::I32EqZero => {
                ensure_jit!(
                    stack_height >= 1,
                    "stack underflow at IR {pc}: I32EqZero needs one value"
                );

                Vec::new()
            }
            Op::I32Add
            | Op::I32Sub
            | Op::I32Mul
            | Op::I32DivSigned
            | Op::I32DivUnsigned
            | Op::I32RemainderSigned
            | Op::I32RemainderUnsigned
            | Op::I32Eq
            | Op::I32Ne
            | Op::I32LtSigned
            | Op::I32LtUnsigned
            | Op::I32GtSigned
            | Op::I32GtUnsigned
            | Op::I32LeSigned
            | Op::I32LeUnsigned
            | Op::I32GeSigned
            | Op::I32GeUnsigned => {
                ensure_jit!(
                    stack_height >= 2,
                    "stack underflow at IR {pc}: {op:?} needs two values"
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
    fn executes_i32_comparisons() {
        let comparisons: &[(Op, fn(i32, i32) -> bool)] = &[
            (Op::I32Eq, |a, b| a == b),
            (Op::I32Ne, |a, b| a != b),
            (Op::I32LtSigned, |a, b| a < b),
            (Op::I32LtUnsigned, |a, b| (a as u32) < (b as u32)),
            (Op::I32GtSigned, |a, b| a > b),
            (Op::I32GtUnsigned, |a, b| (a as u32) > (b as u32)),
            (Op::I32LeSigned, |a, b| a <= b),
            (Op::I32LeUnsigned, |a, b| (a as u32) <= (b as u32)),
            (Op::I32GeSigned, |a, b| a >= b),
            (Op::I32GeUnsigned, |a, b| (a as u32) >= (b as u32)),
        ];

        for &(op, expected) in comparisons {
            let ops = [
                Op::LocalGet2 {
                    local_i_a: 0,
                    local_i_b: 1,
                },
                op,
                Op::Return,
            ];

            assert!(supports(&ops));

            let function = compile(&ops).unwrap();

            assert_eq!(function.result_count(), 1);

            for (a, b) in [(0, 0), (1, 2), (2, 1), (-1, 1), (i32::MIN, i32::MAX)] {
                let mut locals = [u64::from(a as u32), u64::from(b as u32)];
                let mut stack = [0; 2];

                assert_eq!(function.execute(&mut locals, &mut stack).unwrap(), 1);
                assert_eq!(stack[0], u64::from(expected(a, b)), "{op:?}: {a}, {b}");
            }
        }
    }

    #[test]
    fn executes_i32_eqz() {
        let ops = [Op::LocalGet { local_i: 0 }, Op::I32EqZero, Op::Return];

        assert!(supports(&ops));

        let function = compile(&ops).unwrap();

        assert_eq!(function.max_stack_height, 1);

        for value in [0, 1, -1, i32::MIN] {
            let mut locals = [u64::from(value as u32)];
            let mut stack = [0];

            assert_eq!(function.execute(&mut locals, &mut stack).unwrap(), 1);
            assert_eq!(stack[0], u64::from(value == 0));
        }
    }

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
