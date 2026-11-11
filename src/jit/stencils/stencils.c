#include "shared.h"

extern unsigned char _JIT_LOCAL_A;
extern unsigned char _JIT_LOCAL_B;

void i32_add(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a + b;
  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void local_get(VMContext *ctx) {
  uintptr_t local_index = (uintptr_t)&_JIT_LOCAL_A;
  uint64_t value = ctx->locals[local_index];
  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void local_get2(VMContext *ctx) {
  uintptr_t a = (uintptr_t)&_JIT_LOCAL_A;
  uintptr_t b = (uintptr_t)&_JIT_LOCAL_B;

  ctx->stack[ctx->stack_height++] = ctx->locals[a];
  ctx->stack[ctx->stack_height++] = ctx->locals[b];

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void return_value(VMContext *ctx) { (void)ctx; }
