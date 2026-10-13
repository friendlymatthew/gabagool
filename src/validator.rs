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
            // unop: nt -> nt
            Instruction::I32CountLeadingZeros
            | Instruction::I32CountTrailingZeros
            | Instruction::I32PopCount
            | Instruction::I32Extend8Signed
            | Instruction::I32Extend16Signed => self.ctx.unop(ValueType::I32)?,
            Instruction::I64CountLeadingZeros
            | Instruction::I64CountTrailingZeros
            | Instruction::I64PopCount
            | Instruction::I64Extend8Signed
            | Instruction::I64Extend16Signed
            | Instruction::I64Extend32Signed => self.ctx.unop(ValueType::I64)?,
            Instruction::F32Abs
            | Instruction::F32Neg
            | Instruction::F32Ceil
            | Instruction::F32Floor
            | Instruction::F32Trunc
            | Instruction::F32Nearest
            | Instruction::F32Sqrt => self.ctx.unop(ValueType::F32)?,
            Instruction::F64Abs
            | Instruction::F64Neg
            | Instruction::F64Ceil
            | Instruction::F64Floor
            | Instruction::F64Trunc
            | Instruction::F64Nearest
            | Instruction::F64Sqrt => self.ctx.unop(ValueType::F64)?,
            // binop: nt nt -> nt
            Instruction::I32Add
            | Instruction::I32Sub
            | Instruction::I32Mul
            | Instruction::I32DivSigned
            | Instruction::I32DivUnsigned
            | Instruction::I32RemainderSigned
            | Instruction::I32RemainderUnsigned
            | Instruction::I32And
            | Instruction::I32Or
            | Instruction::I32Xor
            | Instruction::I32Shl
            | Instruction::I32ShrSigned
            | Instruction::I32ShrUnsigned
            | Instruction::I32RotateLeft
            | Instruction::I32RotateRight => self.ctx.binop(ValueType::I32)?,
            Instruction::I64Add
            | Instruction::I64Sub
            | Instruction::I64Mul
            | Instruction::I64DivSigned
            | Instruction::I64DivUnsigned
            | Instruction::I64RemainderSigned
            | Instruction::I64RemainderUnsigned
            | Instruction::I64And
            | Instruction::I64Or
            | Instruction::I64Xor
            | Instruction::I64Shl
            | Instruction::I64ShrSigned
            | Instruction::I64ShrUnsigned
            | Instruction::I64RotateLeft
            | Instruction::I64RotateRight => self.ctx.binop(ValueType::I64)?,
            Instruction::F32Add
            | Instruction::F32Sub
            | Instruction::F32Mul
            | Instruction::F32Div
            | Instruction::F32Min
            | Instruction::F32Max
            | Instruction::F32CopySign => self.ctx.binop(ValueType::F32)?,
            Instruction::F64Add
            | Instruction::F64Sub
            | Instruction::F64Mul
            | Instruction::F64Div
            | Instruction::F64Min
            | Instruction::F64Max
            | Instruction::F64CopySign => self.ctx.binop(ValueType::F64)?,
            // cvtop: nt1 -> nt2
            Instruction::I32WrapI64 => self.ctx.cvtop(ValueType::I64, ValueType::I32)?,
            Instruction::I32TruncF32Signed
            | Instruction::I32TruncF32Unsigned
            | Instruction::I32TruncSaturatedF32Signed
            | Instruction::I32TruncSaturatedF32Unsigned
            | Instruction::I32ReinterpretF32 => self.ctx.cvtop(ValueType::F32, ValueType::I32)?,
            Instruction::I32TruncF64Signed
            | Instruction::I32TruncF64Unsigned
            | Instruction::I32TruncSaturatedF64Signed
            | Instruction::I32TruncSaturatedF64Unsigned => {
                self.ctx.cvtop(ValueType::F64, ValueType::I32)?
            }
            Instruction::I64ExtendI32Signed
            | Instruction::I64ExtendI32Unsigned => {
                self.ctx.cvtop(ValueType::I32, ValueType::I64)?
            }
            Instruction::I64TruncF32Signed
            | Instruction::I64TruncF32Unsigned
            | Instruction::I64TruncSaturatedF32Signed
            | Instruction::I64TruncSaturatedF32Unsigned => {
                self.ctx.cvtop(ValueType::F32, ValueType::I64)?
            }
            Instruction::I64TruncF64Signed
            | Instruction::I64TruncF64Unsigned
            | Instruction::I64TruncSaturatedF64Signed
            | Instruction::I64TruncSaturatedF64Unsigned
            | Instruction::I64ReinterpretF64 => self.ctx.cvtop(ValueType::F64, ValueType::I64)?,
            Instruction::F32ConvertI32Signed
            | Instruction::F32ConvertI32Unsigned
            | Instruction::F32ReinterpretI32 => self.ctx.cvtop(ValueType::I32, ValueType::F32)?,
            Instruction::F32ConvertI64Signed | Instruction::F32ConvertI64Unsigned => {
                self.ctx.cvtop(ValueType::I64, ValueType::F32)?
            }
            Instruction::F32DemoteF64 => self.ctx.cvtop(ValueType::F64, ValueType::F32)?,
            Instruction::F64ConvertI32Signed | Instruction::F64ConvertI32Unsigned => {
                self.ctx.cvtop(ValueType::I32, ValueType::F64)?
            }
            Instruction::F64ConvertI64Signed
            | Instruction::F64ConvertI64Unsigned
            | Instruction::F64ReinterpretI64 => self.ctx.cvtop(ValueType::I64, ValueType::F64)?,
            Instruction::F64PromoteF32 => self.ctx.cvtop(ValueType::F32, ValueType::F64)?,
            Instruction::I32EqZero => self.ctx.testop(ValueType::I32)?,
            Instruction::I64EqZero => self.ctx.testop(ValueType::I64)?,
            Instruction::I32Eq
            | Instruction::I32Ne
            | Instruction::I32LtSigned
            | Instruction::I32LtUnsigned
            | Instruction::I32GtSigned
            | Instruction::I32GtUnsigned
            | Instruction::I32LeSigned
            | Instruction::I32LeUnsigned
            | Instruction::I32GeSigned
            | Instruction::I32GeUnsigned => self.ctx.relop(ValueType::I32)?,
            Instruction::I64Eq
            | Instruction::I64Ne
            | Instruction::I64LtSigned
            | Instruction::I64LtUnsigned
            | Instruction::I64GtSigned
            | Instruction::I64GtUnsigned
            | Instruction::I64LeSigned
            | Instruction::I64LeUnsigned
            | Instruction::I64GeSigned
            | Instruction::I64GeUnsigned => self.ctx.relop(ValueType::I64)?,
            Instruction::F32Eq
            | Instruction::F32Ne
            | Instruction::F32Lt
            | Instruction::F32Gt
            | Instruction::F32Le
            | Instruction::F32Ge => self.ctx.relop(ValueType::F32)?,
            Instruction::F64Eq
            | Instruction::F64Ne
            | Instruction::F64Lt
            | Instruction::F64Gt
            | Instruction::F64Le
            | Instruction::F64Ge => self.ctx.relop(ValueType::F64)?,
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

    fn constant(t: ValueType) -> Instruction {
        match t {
            ValueType::I32 => Instruction::I32Const(1),
            ValueType::I64 => Instruction::I64Const(1),
            ValueType::F32 => Instruction::F32Const(1.0),
            ValueType::F64 => Instruction::F64Const(1.0),
            _ => unreachable!("no constant for {t:?}"),
        }
    }

    #[test]
    fn testop_takes_one_operand_and_returns_i32() {
        for (t, eqz) in [
            (ValueType::I32, Instruction::I32EqZero),
            (ValueType::I64, Instruction::I64EqZero),
        ] {
            let body = vec![constant(t), eqz.clone()];

            assert!(validate_body(NONE, I32, &[], body.clone()).is_ok());

            // the result is i32 even for i64.eqz
            assert!(validate_body(NONE, I64, &[], body).is_err());
        }
    }

    #[test]
    fn testop_rejects_the_wrong_operand_type() {
        let body = vec![Instruction::I32Const(1), Instruction::I64EqZero];
        assert!(validate_body(NONE, I32, &[], body).is_err());

        let body = vec![Instruction::I64Const(1), Instruction::I32EqZero];
        assert!(validate_body(NONE, I32, &[], body).is_err());
    }

    #[test]
    fn testop_with_empty_stack_errors() {
        assert!(validate_body(NONE, I32, &[], vec![Instruction::I32EqZero]).is_err());
    }

    #[test]
    fn every_relop_takes_two_operands_and_returns_i32() {
        use Instruction::*;

        let cases: [(ValueType, Vec<Instruction>); 4] = [
            (
                ValueType::I32,
                vec![
                    I32Eq,
                    I32Ne,
                    I32LtSigned,
                    I32LtUnsigned,
                    I32GtSigned,
                    I32GtUnsigned,
                    I32LeSigned,
                    I32LeUnsigned,
                    I32GeSigned,
                    I32GeUnsigned,
                ],
            ),
            (
                ValueType::I64,
                vec![
                    I64Eq,
                    I64Ne,
                    I64LtSigned,
                    I64LtUnsigned,
                    I64GtSigned,
                    I64GtUnsigned,
                    I64LeSigned,
                    I64LeUnsigned,
                    I64GeSigned,
                    I64GeUnsigned,
                ],
            ),
            (
                ValueType::F32,
                vec![F32Eq, F32Ne, F32Lt, F32Gt, F32Le, F32Ge],
            ),
            (
                ValueType::F64,
                vec![F64Eq, F64Ne, F64Lt, F64Gt, F64Le, F64Ge],
            ),
        ];

        for (t, relops) in cases {
            for relop in relops {
                let body = vec![constant(t), constant(t), relop.clone()];

                assert!(
                    validate_body(NONE, I32, &[], body.clone()).is_ok(),
                    "{relop:?} should be i32"
                );

                // never returns the operand type, except where that is i32
                if t != ValueType::I32 {
                    assert!(
                        validate_body(&[], &[t], &[], body).is_err(),
                        "{relop:?} should not return {t:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn relop_consumes_both_operands() {
        let body = vec![
            Instruction::I64Const(1),
            Instruction::I64Const(2),
            Instruction::I64Eq,
        ];

        // nothing but the i32 result is left
        assert!(validate_body(NONE, NONE, &[], body).is_err());
    }

    #[test]
    fn relop_rejects_mismatched_operand_types() {
        let body = vec![
            Instruction::I32Const(1),
            Instruction::I64Const(2),
            Instruction::I32Eq,
        ];
        assert!(validate_body(NONE, I32, &[], body).is_err());

        let body = vec![
            Instruction::I64Const(1),
            Instruction::I64Const(2),
            Instruction::F64Eq,
        ];
        assert!(validate_body(NONE, I32, &[], body).is_err());
    }

    #[test]
    fn relop_with_too_few_operands_errors() {
        let body = vec![Instruction::F32Const(1.0), Instruction::F32Lt];
        assert!(validate_body(NONE, I32, &[], body).is_err());

        assert!(validate_body(NONE, I32, &[], vec![Instruction::F32Lt]).is_err());
    }

    #[test]
    fn relop_result_can_feed_other_instructions() {
        // (i64.eq a b) as an i32 condition, then i32.eqz of that
        let body = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(0),
            Instruction::I64Eq,
            Instruction::I32EqZero,
        ];

        assert!(validate_body(I64, I32, &[], body).is_ok());
    }

    #[test]
    fn relop_in_unreachable_code_accepts_missing_operands() {
        let body = vec![Instruction::Unreachable, Instruction::F64Gt];

        assert!(validate_body(NONE, I32, &[], body).is_ok());
    }

    /// (instruction, operand types in push order, result type)
    fn numeric_table() -> Vec<(Instruction, Vec<ValueType>, ValueType)> {
        use Instruction::*;

        let (i32t, i64t, f32t, f64t) = (
            ValueType::I32,
            ValueType::I64,
            ValueType::F32,
            ValueType::F64,
        );

        let mut table = Vec::new();
        let mut add = |instructions: Vec<Instruction>, params: Vec<ValueType>, result| {
            for instruction in instructions {
                table.push((instruction, params.clone(), result));
            }
        };

        // testop
        add(vec![I32EqZero], vec![i32t], i32t);
        add(vec![I64EqZero], vec![i64t], i32t);

        // relop
        add(
            vec![
                I32Eq, I32Ne, I32LtSigned, I32LtUnsigned, I32GtSigned, I32GtUnsigned,
                I32LeSigned, I32LeUnsigned, I32GeSigned, I32GeUnsigned,
            ],
            vec![i32t, i32t],
            i32t,
        );
        add(
            vec![
                I64Eq, I64Ne, I64LtSigned, I64LtUnsigned, I64GtSigned, I64GtUnsigned,
                I64LeSigned, I64LeUnsigned, I64GeSigned, I64GeUnsigned,
            ],
            vec![i64t, i64t],
            i32t,
        );
        add(
            vec![F32Eq, F32Ne, F32Lt, F32Gt, F32Le, F32Ge],
            vec![f32t, f32t],
            i32t,
        );
        add(
            vec![F64Eq, F64Ne, F64Lt, F64Gt, F64Le, F64Ge],
            vec![f64t, f64t],
            i32t,
        );

        // unop
        add(
            vec![
                I32CountLeadingZeros, I32CountTrailingZeros, I32PopCount, I32Extend8Signed,
                I32Extend16Signed,
            ],
            vec![i32t],
            i32t,
        );
        add(
            vec![
                I64CountLeadingZeros, I64CountTrailingZeros, I64PopCount, I64Extend8Signed,
                I64Extend16Signed, I64Extend32Signed,
            ],
            vec![i64t],
            i64t,
        );
        add(
            vec![F32Abs, F32Neg, F32Ceil, F32Floor, F32Trunc, F32Nearest, F32Sqrt],
            vec![f32t],
            f32t,
        );
        add(
            vec![F64Abs, F64Neg, F64Ceil, F64Floor, F64Trunc, F64Nearest, F64Sqrt],
            vec![f64t],
            f64t,
        );

        // binop
        add(
            vec![
                I32Add, I32Sub, I32Mul, I32DivSigned, I32DivUnsigned, I32RemainderSigned,
                I32RemainderUnsigned, I32And, I32Or, I32Xor, I32Shl, I32ShrSigned,
                I32ShrUnsigned, I32RotateLeft, I32RotateRight,
            ],
            vec![i32t, i32t],
            i32t,
        );
        add(
            vec![
                I64Add, I64Sub, I64Mul, I64DivSigned, I64DivUnsigned, I64RemainderSigned,
                I64RemainderUnsigned, I64And, I64Or, I64Xor, I64Shl, I64ShrSigned,
                I64ShrUnsigned, I64RotateLeft, I64RotateRight,
            ],
            vec![i64t, i64t],
            i64t,
        );
        add(
            vec![F32Add, F32Sub, F32Mul, F32Div, F32Min, F32Max, F32CopySign],
            vec![f32t, f32t],
            f32t,
        );
        add(
            vec![F64Add, F64Sub, F64Mul, F64Div, F64Min, F64Max, F64CopySign],
            vec![f64t, f64t],
            f64t,
        );

        // cvtop
        add(vec![I32WrapI64], vec![i64t], i32t);
        add(
            vec![
                I32TruncF32Signed, I32TruncF32Unsigned, I32TruncSaturatedF32Signed,
                I32TruncSaturatedF32Unsigned, I32ReinterpretF32,
            ],
            vec![f32t],
            i32t,
        );
        add(
            vec![
                I32TruncF64Signed, I32TruncF64Unsigned, I32TruncSaturatedF64Signed,
                I32TruncSaturatedF64Unsigned,
            ],
            vec![f64t],
            i32t,
        );
        add(vec![I64ExtendI32Signed, I64ExtendI32Unsigned], vec![i32t], i64t);
        add(
            vec![
                I64TruncF32Signed, I64TruncF32Unsigned, I64TruncSaturatedF32Signed,
                I64TruncSaturatedF32Unsigned,
            ],
            vec![f32t],
            i64t,
        );
        add(
            vec![
                I64TruncF64Signed, I64TruncF64Unsigned, I64TruncSaturatedF64Signed,
                I64TruncSaturatedF64Unsigned, I64ReinterpretF64,
            ],
            vec![f64t],
            i64t,
        );
        add(
            vec![F32ConvertI32Signed, F32ConvertI32Unsigned, F32ReinterpretI32],
            vec![i32t],
            f32t,
        );
        add(
            vec![F32ConvertI64Signed, F32ConvertI64Unsigned],
            vec![i64t],
            f32t,
        );
        add(vec![F32DemoteF64], vec![f64t], f32t);
        add(
            vec![F64ConvertI32Signed, F64ConvertI32Unsigned],
            vec![i32t],
            f64t,
        );
        add(
            vec![F64ConvertI64Signed, F64ConvertI64Unsigned, F64ReinterpretI64],
            vec![i64t],
            f64t,
        );
        add(vec![F64PromoteF32], vec![f32t], f64t);

        table
    }

    /// A type that is guaranteed to differ from `t`.
    fn other_than(t: ValueType) -> ValueType {
        if t == ValueType::I32 {
            ValueType::I64
        } else {
            ValueType::I32
        }
    }

    fn body_for(instruction: &Instruction, params: &[ValueType]) -> Vec<Instruction> {
        let mut body: Vec<Instruction> = params.iter().map(|&t| constant(t)).collect();
        body.push(instruction.clone());
        body
    }

    #[test]
    fn numeric_instructions_have_their_spec_type() {
        for (instruction, params, result) in numeric_table() {
            let body = body_for(&instruction, &params);

            assert!(
                validate_body(NONE, &[result], &[], body).is_ok(),
                "{instruction:?} should be {params:?} -> {result:?}"
            );
        }
    }

    #[test]
    fn numeric_instructions_reject_a_wrong_result_type() {
        for (instruction, params, result) in numeric_table() {
            let body = body_for(&instruction, &params);

            assert!(
                validate_body(NONE, &[other_than(result)], &[], body).is_err(),
                "{instruction:?} should not return {:?}",
                other_than(result)
            );
        }
    }

    #[test]
    fn numeric_instructions_consume_their_operands() {
        for (instruction, params, _) in numeric_table() {
            let body = body_for(&instruction, &params);

            // only the result is left, so a function returning nothing is invalid
            assert!(
                validate_body(NONE, NONE, &[], body).is_err(),
                "{instruction:?} should leave exactly one value"
            );
        }
    }

    #[test]
    fn numeric_instructions_reject_a_missing_operand() {
        for (instruction, params, result) in numeric_table() {
            let body = body_for(&instruction, &params[1..]);

            assert!(
                validate_body(NONE, &[result], &[], body).is_err(),
                "{instruction:?} should need {} operand(s)",
                params.len()
            );
        }
    }

    #[test]
    fn numeric_instructions_reject_a_wrong_operand_type() {
        for (instruction, params, result) in numeric_table() {
            for bad in 0..params.len() {
                let mut wrong = params.clone();
                wrong[bad] = other_than(params[bad]);

                let body = body_for(&instruction, &wrong);

                assert!(
                    validate_body(NONE, &[result], &[], body).is_err(),
                    "{instruction:?} should reject operand {bad} being {:?}",
                    wrong[bad]
                );
            }
        }
    }

    #[test]
    fn numeric_instructions_accept_missing_operands_in_unreachable_code() {
        for (instruction, _, result) in numeric_table() {
            let body = vec![Instruction::Unreachable, instruction.clone()];

            assert!(
                validate_body(NONE, &[result], &[], body).is_ok(),
                "{instruction:?} should type check after unreachable"
            );
        }
    }

    #[test]
    fn numeric_instructions_compose() {
        // ((a + b) as i64) * 2, wrapped back to i32, compared with a
        let body = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32Add,
            Instruction::I64ExtendI32Signed,
            Instruction::I64Const(2),
            Instruction::I64Mul,
            Instruction::I32WrapI64,
            Instruction::LocalGet(0),
            Instruction::I32Eq,
        ];

        assert!(validate_body(&[ValueType::I32, ValueType::I32], I32, &[], body).is_ok());
    }
}
