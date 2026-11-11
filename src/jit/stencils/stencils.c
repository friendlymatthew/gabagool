#include "shared.h"

// each stencil decides waht its numbered params mean
// for example, local_get uses param 0 as an index, i32_const uses it as a value
extern unsigned char _JIT_PARAM_0;
extern unsigned char _JIT_PARAM_1;

void i32_add(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a + b;
  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void local_get(VMContext *ctx) {
  uintptr_t local_index = (uintptr_t)&_JIT_PARAM_0;
  uint64_t value = ctx->locals[local_index];
  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void local_get2(VMContext *ctx) {
  uintptr_t a = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t b = (uintptr_t)&_JIT_PARAM_1;

  ctx->stack[ctx->stack_height++] = ctx->locals[a];
  ctx->stack[ctx->stack_height++] = ctx->locals[b];

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void return_value(VMContext *ctx) { (void)ctx; }

void i32_const(VMContext *ctx) {
  uint32_t value = (uint32_t)(uintptr_t)&_JIT_PARAM_0;
  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void local_set(VMContext *ctx) {
  uintptr_t local_index = (uintptr_t)&_JIT_PARAM_0;
  uint64_t value = ctx->stack[--ctx->stack_height];

  ctx->locals[local_index] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}
