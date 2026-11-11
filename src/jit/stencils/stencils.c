#include "shared.h"

// each stencil decides waht its numbered params mean
// for example, local_get uses param 0 as an index, i32_const uses it as a value
extern unsigned char _JIT_PARAM_0;
extern unsigned char _JIT_PARAM_1;

void return_value(VMContext *ctx) { (void)ctx; }

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

void local_set(VMContext *ctx) {
  uintptr_t local_index = (uintptr_t)&_JIT_PARAM_0;
  uint64_t value = ctx->stack[--ctx->stack_height];

  ctx->locals[local_index] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void local_tee(VMContext *ctx) {
  uintptr_t local_index = (uintptr_t)&_JIT_PARAM_0;
  uint64_t value = ctx->stack[ctx->stack_height - 1];

  ctx->locals[local_index] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_add(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a + b;
  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_const(VMContext *ctx) {
  uint32_t value = (uint32_t)(uintptr_t)&_JIT_PARAM_0;
  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_sub(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  ctx->stack[ctx->stack_height++] = a - b;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_mul(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = (uint32_t)(a * b);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_div_s(VMContext *ctx) {
  int32_t b = (int32_t)ctx->stack[--ctx->stack_height];
  int32_t a = (int32_t)ctx->stack[--ctx->stack_height];

  if (b == 0) {
    ctx->trap = JIT_TRAP_DIVIDE_BY_ZERO;
    return;
  }

  if (a == INT32_MIN && b == -1) {
    ctx->trap = JIT_TRAP_INTEGER_OVERFLOW;
    return;
  }

  uint32_t result = (uint32_t)(a / b);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_div_u(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  if (b == 0) {
    ctx->trap = JIT_TRAP_DIVIDE_BY_ZERO;
    return;
  }

  uint32_t result = (uint32_t)(a / b);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_rem_s(VMContext *ctx) {
  int32_t b = (int32_t)ctx->stack[--ctx->stack_height];
  int32_t a = (int32_t)ctx->stack[--ctx->stack_height];

  if (b == 0) {
    ctx->trap = JIT_TRAP_DIVIDE_BY_ZERO;
    return;
  }

  // c makes INT32_MIN % -1 undefined but wasm requires 0
  uint32_t result = b == -1 ? 0 : (uint32_t)(a % b);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_rem_u(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  if (b == 0) {
    ctx->trap = JIT_TRAP_DIVIDE_BY_ZERO;
    return;
  }

  uint32_t result = (uint32_t)(a % b);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_eqz(VMContext *ctx) {
  uint32_t value = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = value == 0;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_eq(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a == b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_ne(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a != b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_lt_s(VMContext *ctx) {
  int32_t b = (int32_t)ctx->stack[--ctx->stack_height];
  int32_t a = (int32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a < b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_lt_u(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a < b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_gt_s(VMContext *ctx) {
  int32_t b = (int32_t)ctx->stack[--ctx->stack_height];
  int32_t a = (int32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a > b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_gt_u(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a > b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_le_s(VMContext *ctx) {
  int32_t b = (int32_t)ctx->stack[--ctx->stack_height];
  int32_t a = (int32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a <= b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_le_u(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a <= b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_ge_s(VMContext *ctx) {
  int32_t b = (int32_t)ctx->stack[--ctx->stack_height];
  int32_t a = (int32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a >= b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_ge_u(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a >= b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}
