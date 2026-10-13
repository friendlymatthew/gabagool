use crate::{
    ensure_validation, validation_err, Error, Function, Instruction, Local, ParsedModule, Result,
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

            validation_err!("expected value on the stack but the block has none")
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
            _ => validation_err!("type mismatch, expected: {:?}, got: {:?}", expected, actual),
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
}
