
#include "stencil_context.h"

void nop(StencilContext *ctx) {
  ctx->pc += 1;

  CHECK_SNAPSHOT(ctx);

  __attribute__((musttail)) return ctx->fn_table[ctx->pc](ctx);
}

void i32_const(StencilContext *ctx) {
  ctx->stack[ctx->stack_pointer] = (uint32_t)ctx->imm_table[ctx->pc].imm0;

  ctx->stack_pointer += 1;
  ctx->pc += 1;

  CHECK_SNAPSHOT(ctx);

  __attribute__((musttail)) return ctx->fn_table[ctx->pc](ctx);
}

void local_get(StencilContext *ctx) {
  uint32_t idx = (uint32_t)ctx->imm_table[ctx->pc].imm0;
  ctx->stack[ctx->stack_pointer] = ctx->locals[idx];

  ctx->stack_pointer += 1;
  ctx->pc += 1;

  CHECK_SNAPSHOT(ctx);

  __attribute__((musttail)) return ctx->fn_table[ctx->pc](ctx);
}

void local_set(StencilContext *ctx) {
  uint32_t idx = (uint32_t)ctx->imm_table[ctx->pc].imm0;
  ctx->stack_pointer -= 1;
  ctx->locals[idx] = ctx->stack[ctx->stack_pointer];

  ctx->pc += 1;

  CHECK_SNAPSHOT(ctx);

  __attribute__((musttail)) return ctx->fn_table[ctx->pc](ctx);
}

#define I32_BINOP(name, op)                                                    \
  void name(StencilContext *ctx) {                                             \
    ctx->stack_pointer -= 1;                                                   \
                                                                               \
    uint32_t b = (uint32_t)ctx->stack[ctx->stack_pointer];                     \
    uint32_t a = (uint32_t)ctx->stack[ctx->stack_pointer - 1];                 \
                                                                               \
    ctx->stack[ctx->stack_pointer - 1] = (uint32_t)(a op b);                   \
    ctx->pc += 1;                                                              \
                                                                               \
    CHECK_SNAPSHOT(ctx);                                                       \
    __attribute__((musttail)) return ctx->fn_table[ctx->pc](ctx);              \
  }

I32_BINOP(i32_add, +)
I32_BINOP(i32_sub, -)
I32_BINOP(i32_mul, *)

#define I32_CMP(name, op)                                                      \
  void name(StencilContext *ctx) {                                             \
    ctx->stack_pointer -= 1;                                                   \
                                                                               \
    int32_t b = (int32_t)ctx->stack[ctx->stack_pointer];                       \
    int32_t a = (int32_t)ctx->stack[ctx->stack_pointer - 1];                   \
                                                                               \
    ctx->stack[ctx->stack_pointer - 1] = (a op b) ? 1 : 0;                     \
    ctx->pc += 1;                                                              \
                                                                               \
    CHECK_SNAPSHOT(ctx);                                                       \
    __attribute__((musttail)) return ctx->fn_table[ctx->pc](ctx);              \
  }

I32_CMP(i32_lt_signed, <)
I32_CMP(i32_gt_signed, >)
I32_CMP(i32_le_signed, <=)
I32_CMP(i32_ge_signed, >=)
I32_CMP(i32_eq, ==)
I32_CMP(i32_ne, !=)

#define U32_CMP(name, op)                                                      \
  void name(StencilContext *ctx) {                                             \
    ctx->stack_pointer -= 1;                                                   \
                                                                               \
    uint32_t b = (uint32_t)ctx->stack[ctx->stack_pointer];                     \
    uint32_t a = (uint32_t)ctx->stack[ctx->stack_pointer - 1];                 \
                                                                               \
    ctx->stack[ctx->stack_pointer - 1] = (a op b) ? 1 : 0;                     \
    ctx->pc += 1;                                                              \
                                                                               \
    CHECK_SNAPSHOT(ctx);                                                       \
    __attribute__((musttail)) return ctx->fn_table[ctx->pc](ctx);              \
  }

U32_CMP(i32_lt_unsigned, <)
U32_CMP(i32_gt_unsigned, >)
U32_CMP(i32_le_unsigned, <=)
U32_CMP(i32_ge_unsigned, >=)

void i32_eq_zero(StencilContext *ctx) {
  uint32_t a = (uint32_t)ctx->stack[ctx->stack_pointer - 1];
  ctx->stack[ctx->stack_pointer - 1] = (a == 0) ? 1 : 0;
  ctx->pc += 1;

  CHECK_SNAPSHOT(ctx);
  __attribute__((musttail)) return ctx->fn_table[ctx->pc](ctx);
}

void jump(StencilContext *ctx) {
  uint32_t target = (uint32_t)ctx->imm_table[ctx->pc].imm0;
  uint32_t keep = (uint32_t)(ctx->imm_table[ctx->pc].imm1 >> 32);
  uint32_t drop = (uint32_t)(ctx->imm_table[ctx->pc].imm1);

  STACK_KEEP_DROP(ctx, keep, drop);
  ctx->pc = target;

  CHECK_SNAPSHOT(ctx);
  __attribute__((musttail)) return ctx->fn_table[ctx->pc](ctx);
}

void jump_if(StencilContext *ctx) {
  ctx->stack_pointer -= 1;
  uint32_t cond = (uint32_t)ctx->stack[ctx->stack_pointer];

  if (cond == 0) {
    ctx->pc += 1;
  } else {
    uint32_t target = (uint32_t)ctx->imm_table[ctx->pc].imm0;
    uint32_t keep = (uint32_t)(ctx->imm_table[ctx->pc].imm1 >> 32);
    uint32_t drop = (uint32_t)(ctx->imm_table[ctx->pc].imm1);

    STACK_KEEP_DROP(ctx, keep, drop);
    ctx->pc = target;
  }

  CHECK_SNAPSHOT(ctx);
  __attribute__((musttail)) return ctx->fn_table[ctx->pc](ctx);
}

void jump_if_not(StencilContext *ctx) {
  ctx->stack_pointer -= 1;
  uint32_t cond = (uint32_t)ctx->stack[ctx->stack_pointer];

  if (cond == 0) {
    uint32_t target = (uint32_t)ctx->imm_table[ctx->pc].imm0;
    uint32_t keep = (uint32_t)(ctx->imm_table[ctx->pc].imm1 >> 32);
    uint32_t drop = (uint32_t)(ctx->imm_table[ctx->pc].imm1);

    STACK_KEEP_DROP(ctx, keep, drop);
    ctx->pc = target;
  } else {
    ctx->pc += 1;
  }

  CHECK_SNAPSHOT(ctx);
  __attribute__((musttail)) return ctx->fn_table[ctx->pc](ctx);
}

void return_(StencilContext *ctx) { ctx->exit_reason = EXIT_RETURN; }
