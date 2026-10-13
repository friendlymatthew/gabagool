use crate::{
    bail_validation, ensure_validation, Error, Function, Instruction, Local, ParsedModule, Result,
    ValueType,
};

#[derive(Debug, PartialEq, Eq)]
enum StackType {
    Known(ValueType),
    Bot,
}

impl From<ValueType> for StackType {
    fn from(vt: ValueType) -> Self {
        Self::Known(vt)
    }
}

struct Frame<'a> {
    start_types: &'a [ValueType],
    end_types: &'a [ValueType],
    value_height: usize,
    unreachable: bool,
    is_loop: bool,
}

impl<'a> Frame<'a> {
    const fn label_types(&self) -> &'a [ValueType] {
        if self.is_loop {
            self.start_types
        } else {
            self.end_types
        }
    }
}

pub struct Validator<'a> {
    module: &'a ParsedModule,
    ctx: FunctionContext<'a>,
}

impl<'a> Validator<'a> {
    pub fn new(module: &'a ParsedModule) -> Self {
        Self {
            module,
            ctx: FunctionContext::default(),
        }
    }

    pub fn validate(&mut self) -> Result<()> {
        for function in &self.module.functions {
            self.validate_function(function)?;
        }

        Ok(())
    }

    fn validate_function(&mut self, f: &'a Function) -> Result<()> {
        let ty = self
            .module
            .types
            .get(f.type_index as usize)
            .ok_or_else(|| Error::validation("oob"))?
            .try_as_function_type()
            .ok_or_else(|| Error::validation("does not have function type"))?;

        let mut locals = ty.params.clone();

        for &Local { count, value_type } in f.locals.iter() {
            locals.extend(std::iter::repeat_n(value_type, count as usize));
        }

        self.ctx = FunctionContext::new(locals);
        self.ctx.push_control(&[], &ty.results, false);

        for instruction in f.body.iter() {
            self.validate_instruction(instruction)?;
        }

        self.ctx.pop_control()?;

        Ok(())
    }

    fn validate_instruction(&mut self, instruction: &'a Instruction) -> Result<()> {
        match instruction {
            Instruction::Nop => {}
            Instruction::Unreachable => self.ctx.mark_unreachable()?,
            Instruction::Drop => {
                let _ = self.ctx.pop_value()?;
            }
            Instruction::I32Const(_) => {
                self.ctx.push_value(ValueType::I32);
            }
            Instruction::I64Const(_) => {
                self.ctx.push_value(ValueType::I64);
            }
            Instruction::F32Const(_) => {
                self.ctx.push_value(ValueType::F32);
            }
            Instruction::F64Const(_) => {
                self.ctx.push_value(ValueType::F64);
            }
            Instruction::LocalGet(x) => {
                let ty = self.ctx.local_type(*x)?;
                self.ctx.apply(&[], &[ty])?;
            }
            Instruction::LocalSet(x) => {
                let ty = self.ctx.local_type(*x)?;
                self.ctx.apply(&[ty], &[])?;
            }
            Instruction::LocalTee(x) => {
                let ty = self.ctx.local_type(*x)?;
                self.ctx.apply(&[ty], &[ty])?;
            }
            Instruction::I32Add | Instruction::I32Sub | Instruction::I32Mul => {
                self.ctx.binop(ValueType::I32)?;
            }
            _ => unimplemented!(),
        }

        Ok(())
    }
}

#[derive(Default)]
struct FunctionContext<'a> {
    locals: Vec<ValueType>,
    value_stack: Vec<StackType>,
    control_stack: Vec<Frame<'a>>,
}

impl<'a> FunctionContext<'a> {
    const fn new(locals: Vec<ValueType>) -> Self {
        Self {
            locals,
            value_stack: Vec::new(),
            control_stack: Vec::new(),
        }
    }

    fn local_type(&self, index: u32) -> Result<ValueType> {
        self.locals.get(index as usize).copied().ok_or_else(|| {
            Error::validation(format!(
                "unknown local {index}, function has {} locals",
                self.locals.len()
            ))
        })
    }

    fn apply(&mut self, params: &[ValueType], results: &[ValueType]) -> Result<()> {
        self.pop_values_expect(params)?;
        self.push_values(results.iter().cloned());

        Ok(())
    }

    fn unop(&mut self, t: ValueType) -> Result<()> {
        self.apply(&[t], &[t])
    }

    fn binop(&mut self, t: ValueType) -> Result<()> {
        self.apply(&[t, t], &[t])
    }

    fn testop(&mut self, t: ValueType) -> Result<()> {
        self.apply(&[t], &[ValueType::I32])
    }

    fn relop(&mut self, t: ValueType) -> Result<()> {
        self.apply(&[t, t], &[ValueType::I32])
    }

    fn cvtop(&mut self, from: ValueType, to: ValueType) -> Result<()> {
        self.apply(&[from], &[to])
    }

    fn push_value(&mut self, v: impl Into<StackType>) {
        self.value_stack.push(v.into());
    }

    fn pop_value(&mut self) -> Result<StackType> {
        let top_frame = self
            .control_stack
            .last()
            .ok_or_else(|| Error::validation("control stack empty while popping value"))?;

        if top_frame.value_height == self.value_stack.len() {
            if top_frame.unreachable {
                return Ok(StackType::Bot);
            }

            bail_validation!("expected value on the stack but the block has none")
        }

        let t = self
            .value_stack
            .pop()
            .expect("value stack empty, it's above the frame height lowk should never happen");

        Ok(t)
    }

    fn pop_value_expect(&mut self, expected: &ValueType) -> Result<StackType> {
        let actual = self.pop_value()?;

        match actual {
            StackType::Bot => Ok(actual),
            StackType::Known(value_type) if expected == &value_type => Ok(actual),
            _ => bail_validation!("type mismatch, expected: {:?}, got: {:?}", expected, actual),
        }
    }

    fn push_values<I>(&mut self, v: I)
    where
        I: IntoIterator,
        I::Item: Into<StackType>,
    {
        self.value_stack.extend(v.into_iter().map(Into::into));
    }

    fn pop_values_expect(&mut self, expected: &[ValueType]) -> Result<()> {
        for t in expected.iter().rev() {
            self.pop_value_expect(t)?;
        }

        Ok(())
    }

    fn mark_unreachable(&mut self) -> Result<()> {
        let top_frame = self
            .control_stack
            .last_mut()
            .ok_or_else(|| Error::validation("control stack empty in mark_unreachable"))?;

        self.value_stack.truncate(top_frame.value_height);
        top_frame.unreachable = true;

        Ok(())
    }

    fn push_control(
        &mut self,
        start_types: &'a [ValueType],
        end_types: &'a [ValueType],
        is_loop: bool,
    ) {
        let f = Frame {
            start_types,
            end_types,
            value_height: self.value_stack.len(),
            unreachable: false,
            is_loop,
        };

        self.control_stack.push(f);
        self.push_values(start_types.iter().cloned());
    }

    fn pop_control(&mut self) -> Result<Frame<'a>> {
        let (end_types, value_height) = {
            let top_frame = self
                .control_stack
                .last()
                .ok_or_else(|| Error::validation("empty stack"))?;

            (top_frame.end_types, top_frame.value_height)
        };

        self.pop_values_expect(end_types)?;

        ensure_validation!(self.value_stack.len() == value_height, "not matching");

        self.control_stack
            .pop()
            .ok_or_else(|| Error::validation("should not occur"))
    }
}

#[cfg(test)]
mod tests {
    use crate::{CompositeType, FunctionType, SubType};

    use super::*;

    const NONE: &[ValueType] = &[];
    const I32: &[ValueType] = &[ValueType::I32];
    const I64: &[ValueType] = &[ValueType::I64];
    const I32_I64: &[ValueType] = &[ValueType::I32, ValueType::I64];

    fn ctx() -> FunctionContext<'static> {
        let mut v = FunctionContext::default();
        // function-level frame, like the one validate_function pushes
        v.push_control(NONE, NONE, false);
        v
    }

    #[test]
    fn push_then_pop_expect_matches() {
        let mut v = ctx();

        v.push_value(ValueType::I32);

        assert_eq!(
            v.pop_value_expect(&ValueType::I32).unwrap(),
            StackType::Known(ValueType::I32)
        );
    }

    #[test]
    fn pop_expect_wrong_type_errors() {
        let mut v = ctx();

        v.push_value(ValueType::I32);

        assert!(v.pop_value_expect(&ValueType::I64).is_err());
    }

    #[test]
    fn pop_from_empty_reachable_frame_errors() {
        let mut v = ctx();

        assert!(v.pop_value().is_err());
    }

    #[test]
    fn block_cannot_pop_values_below_its_floor() {
        let mut v = ctx();

        v.push_value(ValueType::I32);
        v.push_control(NONE, NONE, false);

        assert!(v.pop_value().is_err());
    }

    #[test]
    fn pop_from_empty_unreachable_frame_gives_bot() {
        let mut v = ctx();

        v.mark_unreachable().unwrap();

        assert_eq!(v.pop_value().unwrap(), StackType::Bot);
        assert_eq!(v.pop_value_expect(&ValueType::I64).unwrap(), StackType::Bot);
    }

    // spec note: `(unreachable (i32.const) i64.add)` is invalid
    #[test]
    fn unreachable_then_push_i32_then_pop_i64_errors() {
        let mut v = ctx();

        v.mark_unreachable().unwrap();
        v.push_value(ValueType::I32);

        assert!(v.pop_value_expect(&ValueType::I64).is_err());
    }

    #[test]
    fn mark_unreachable_discards_values_above_floor() {
        let mut v = ctx();

        v.push_value(ValueType::I32);
        v.push_value(ValueType::I64);
        v.mark_unreachable().unwrap();

        assert!(v.value_stack.is_empty());
    }

    #[test]
    fn pop_values_expect_checks_in_reverse_order() {
        let mut v = ctx();

        // stack top is I64, so [I32, I64] matches
        v.push_values([ValueType::I32, ValueType::I64]);
        assert!(v.pop_values_expect(I32_I64).is_ok());

        // swapped order does not
        v.push_values([ValueType::I64, ValueType::I32]);
        assert!(v.pop_values_expect(I32_I64).is_err());
    }

    #[test]
    fn push_control_pushes_params() {
        let mut v = ctx();

        v.push_control(I32, I32, false);

        assert_eq!(v.value_stack, vec![StackType::Known(ValueType::I32)]);
    }

    #[test]
    fn pop_control_with_matching_results_succeeds() {
        let mut v = ctx();

        v.push_control(I32, I32, false); // param i32 is already on the stack

        assert!(v.pop_control().is_ok());
        assert!(v.value_stack.is_empty());
    }

    #[test]
    fn pop_control_with_leftover_values_errors() {
        let mut v = ctx();

        v.push_control(NONE, NONE, false);
        v.push_value(ValueType::I32);

        assert!(v.pop_control().is_err());
    }

    #[test]
    fn pop_control_with_missing_results_errors() {
        let mut v = ctx();

        v.push_control(NONE, I32, false);

        assert!(v.pop_control().is_err());
    }

    #[test]
    fn pop_control_with_wrong_result_type_errors() {
        let mut v = ctx();

        v.push_control(NONE, I64, false);
        v.push_value(ValueType::I32);

        assert!(v.pop_control().is_err());
    }

    #[test]
    fn pop_control_with_no_frame_errors() {
        let mut v = FunctionContext::default();

        assert!(v.pop_control().is_err());
    }

    #[test]
    fn pop_control_in_unreachable_frame_accepts_missing_results() {
        let mut v = ctx();

        v.push_control(NONE, I32, false);
        v.mark_unreachable().unwrap();

        assert!(v.pop_control().is_ok());
    }

    #[test]
    fn label_types_depends_on_loop() {
        let mut v = ctx();

        v.push_control(I32, I64, true);
        assert_eq!(v.control_stack.last().unwrap().label_types(), I32);

        v.push_control(I32, I64, false);
        assert_eq!(v.control_stack.last().unwrap().label_types(), I64);
    }

    fn func_type(params: &[ValueType], results: &[ValueType]) -> SubType {
        SubType {
            is_final: true,
            supertypes: vec![],
            composite_type: CompositeType::Func(FunctionType {
                params: params.to_vec(),
                results: results.to_vec(),
            }),
        }
    }

    fn function(type_index: u32, locals: &[(u32, ValueType)], body: Vec<Instruction>) -> Function {
        Function {
            type_index,
            locals: locals
                .iter()
                .map(|&(count, value_type)| Local { count, value_type })
                .collect(),
            body,
        }
    }

    fn validate_body(
        params: &[ValueType],
        results: &[ValueType],
        locals: &[(u32, ValueType)],
        body: Vec<Instruction>,
    ) -> Result<()> {
        let mut module = ParsedModule::default();
        module.types.push(func_type(params, results));
        module.functions.push(function(0, locals, body));

        Validator::new(&module).validate()
    }

    #[test]
    fn local_type_is_bounds_checked() {
        let ctx = FunctionContext::new(vec![ValueType::I32, ValueType::F64]);

        assert_eq!(ctx.local_type(0).unwrap(), ValueType::I32);
        assert_eq!(ctx.local_type(1).unwrap(), ValueType::F64);
        assert!(ctx.local_type(2).is_err());
    }

    #[test]
    fn local_get_reads_a_param() {
        let body = vec![Instruction::LocalGet(0)];

        assert!(validate_body(I32, I32, &[], body).is_ok());
    }

    #[test]
    fn local_get_has_the_locals_type() {
        let body = vec![Instruction::LocalGet(0)];

        // a param i32 does not satisfy an i64 result
        assert!(validate_body(I32, I64, &[], body).is_err());
    }

    #[test]
    fn local_indices_are_params_then_declared_locals() {
        let params = [ValueType::I32, ValueType::F32];
        let locals = [(1, ValueType::I64)];

        assert!(validate_body(&params, I64, &locals, vec![Instruction::LocalGet(2)]).is_ok());
        assert!(validate_body(
            &params,
            &[ValueType::F32],
            &locals,
            vec![Instruction::LocalGet(1)]
        )
        .is_ok());
        assert!(validate_body(&params, I32, &locals, vec![Instruction::LocalGet(2)]).is_err());
    }

    #[test]
    fn local_count_is_expanded() {
        let locals = [(3, ValueType::F32)];
        let f32 = &[ValueType::F32];

        // locals 0, 1, 2 exist, 3 does not
        assert!(validate_body(NONE, f32, &locals, vec![Instruction::LocalGet(2)]).is_ok());
        assert!(validate_body(NONE, f32, &locals, vec![Instruction::LocalGet(3)]).is_err());
    }

    #[test]
    fn local_get_out_of_range_errors() {
        assert!(validate_body(NONE, I32, &[], vec![Instruction::LocalGet(0)]).is_err());
        assert!(validate_body(I32, I32, &[], vec![Instruction::LocalGet(1)]).is_err());
    }

    #[test]
    fn local_set_pops_a_matching_value() {
        let body = vec![Instruction::I32Const(1), Instruction::LocalSet(0)];

        assert!(validate_body(I32, NONE, &[], body).is_ok());
    }

    #[test]
    fn local_set_wrong_type_errors() {
        let body = vec![Instruction::I64Const(1), Instruction::LocalSet(0)];

        assert!(validate_body(I32, NONE, &[], body).is_err());
    }

    #[test]
    fn local_set_with_empty_stack_errors() {
        assert!(validate_body(I32, NONE, &[], vec![Instruction::LocalSet(0)]).is_err());
    }

    #[test]
    fn local_set_out_of_range_errors() {
        let body = vec![Instruction::I32Const(1), Instruction::LocalSet(1)];

        assert!(validate_body(I32, NONE, &[], body).is_err());
    }

    #[test]
    fn local_set_leaves_nothing_on_the_stack() {
        // a function returning i32 would need the value that set consumed
        let body = vec![Instruction::I32Const(1), Instruction::LocalSet(0)];

        assert!(validate_body(I32, I32, &[], body).is_err());
    }

    #[test]
    fn local_tee_keeps_the_value() {
        let body = vec![Instruction::I32Const(1), Instruction::LocalTee(0)];

        assert!(validate_body(I32, I32, &[], body).is_ok());
    }

    #[test]
    fn local_tee_then_drop_leaves_an_empty_stack() {
        let body = vec![
            Instruction::I32Const(1),
            Instruction::LocalTee(0),
            Instruction::Drop,
        ];

        assert!(validate_body(I32, NONE, &[], body).is_ok());
    }

    #[test]
    fn local_tee_wrong_type_errors() {
        let body = vec![Instruction::I64Const(1), Instruction::LocalTee(0)];

        assert!(validate_body(I32, I64, &[], body).is_err());
    }

    #[test]
    fn local_tee_with_empty_stack_errors() {
        assert!(validate_body(I32, I32, &[], vec![Instruction::LocalTee(0)]).is_err());
    }

    #[test]
    fn local_set_in_unreachable_code_accepts_a_missing_operand() {
        let body = vec![Instruction::Unreachable, Instruction::LocalSet(0)];

        assert!(validate_body(I32, NONE, &[], body).is_ok());
    }

    #[test]
    fn locals_do_not_leak_between_functions() {
        let mut module = ParsedModule::default();
        module.types.push(func_type(I32, I32));
        module.types.push(func_type(NONE, I32));

        // function 0 has local 0; function 1 has none, so reading local 0 must fail
        module
            .functions
            .push(function(0, &[], vec![Instruction::LocalGet(0)]));

        module
            .functions
            .push(function(1, &[], vec![Instruction::LocalGet(0)]));

        assert!(Validator::new(&module).validate().is_err());
    }
}
