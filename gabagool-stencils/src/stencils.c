
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

void return_(StencilContext *ctx) { ctx->exit_reason = EXIT_RETURN; }
