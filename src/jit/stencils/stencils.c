#include "shared.h"

// each stencil decides waht its numbered params mean
// for example, local_get uses param 0 as an index, i32_const uses it as a value
extern unsigned char _JIT_PARAM_0;
extern unsigned char _JIT_PARAM_1;
extern unsigned char _JIT_PARAM_2;

void jump(VMContext *ctx) { __attribute__((musttail)) return _JIT_TARGET(ctx); }

void jump_with_drop(VMContext *ctx) {
  uintptr_t keep = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t drop = (uintptr_t)&_JIT_PARAM_1;
  uint64_t source = ctx->stack_height - keep;
  uint64_t destination = source - drop;

  for (uint64_t i = 0; i < keep; i++) {
    ctx->stack[destination + i] = ctx->stack[source + i];
  }

  ctx->stack_height -= drop;

  __attribute__((musttail)) return _JIT_TARGET(ctx);
}

void jump_if(VMContext *ctx) {
  uint32_t condition = (uint32_t)ctx->stack[--ctx->stack_height];

  if (condition != 0) {
    __attribute__((musttail)) return _JIT_TARGET(ctx);
  }

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void jump_if_with_drop(VMContext *ctx) {
  uint32_t condition = (uint32_t)ctx->stack[--ctx->stack_height];

  if (condition != 0) {
    uintptr_t keep = (uintptr_t)&_JIT_PARAM_0;
    uintptr_t drop = (uintptr_t)&_JIT_PARAM_1;
    uint64_t source = ctx->stack_height - keep;
    uint64_t destination = source - drop;

    for (uint64_t i = 0; i < keep; i++) {
      ctx->stack[destination + i] = ctx->stack[source + i];
    }

    ctx->stack_height -= drop;

    __attribute__((musttail)) return _JIT_TARGET(ctx);
  }

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void jump_if_not(VMContext *ctx) {
  uint32_t condition = (uint32_t)ctx->stack[--ctx->stack_height];

  if (condition == 0) {
    __attribute__((musttail)) return _JIT_TARGET(ctx);
  }

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void jump_if_not_with_drop(VMContext *ctx) {
  uint32_t condition = (uint32_t)ctx->stack[--ctx->stack_height];

  if (condition == 0) {
    uintptr_t keep = (uintptr_t)&_JIT_PARAM_0;
    uintptr_t drop = (uintptr_t)&_JIT_PARAM_1;
    uint64_t source = ctx->stack_height - keep;
    uint64_t destination = source - drop;

    for (uint64_t i = 0; i < keep; i++) {
      ctx->stack[destination + i] = ctx->stack[source + i];
    }

    ctx->stack_height -= drop;

    __attribute__((musttail)) return _JIT_TARGET(ctx);
  }

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

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

void i32_clz(VMContext *ctx) {
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a == 0 ? 32 : __builtin_clz(a);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_ctz(VMContext *ctx) {
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a == 0 ? 32 : __builtin_ctz(a);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_popcnt(VMContext *ctx) {
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = __builtin_popcount(a);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_and(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a & b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_or(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a | b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_xor(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a ^ b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_shl(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a << (b & 31);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_shr_u(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a >> (b & 31);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_rotl(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = (a << (b & 31)) | (a >> ((-b) & 31));

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_rotr(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = (a >> (b & 31)) | (a << ((-b) & 31));

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_shr_s(VMContext *ctx) {
  int32_t b = (int32_t)ctx->stack[--ctx->stack_height];
  int32_t a = (int32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a >> ((uint32_t)b & 31);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_clz(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a == 0 ? 64 : __builtin_clzll(a);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_ctz(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a == 0 ? 64 : __builtin_ctzll(a);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_popcnt(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = __builtin_popcountll(a);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_and(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a & b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_or(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a | b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_xor(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a ^ b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_shl(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a << (b & 63);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_shr_u(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a >> (b & 63);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_rotl(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = (a << (b & 63)) | (a >> ((-b) & 63));

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_rotr(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = (a >> (b & 63)) | (a << ((-b) & 63));

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_shr_s(VMContext *ctx) {
  int64_t b = (int64_t)ctx->stack[--ctx->stack_height];
  int64_t a = (int64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a >> ((uint64_t)b & 63);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_add(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a + b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_sub(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a - b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_mul(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a * b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_div_s(VMContext *ctx) {
  int64_t b = (int64_t)ctx->stack[--ctx->stack_height];
  int64_t a = (int64_t)ctx->stack[--ctx->stack_height];

  if (b == 0) {
    ctx->trap = JIT_TRAP_DIVIDE_BY_ZERO;
    return;
  }

  if (a == INT64_MIN && b == -1) {
    ctx->trap = JIT_TRAP_INTEGER_OVERFLOW;
    return;
  }

  uint64_t result = (uint64_t)(a / b);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_div_u(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  if (b == 0) {
    ctx->trap = JIT_TRAP_DIVIDE_BY_ZERO;
    return;
  }

  uint64_t result = a / b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_rem_s(VMContext *ctx) {
  int64_t b = (int64_t)ctx->stack[--ctx->stack_height];
  int64_t a = (int64_t)ctx->stack[--ctx->stack_height];

  if (b == 0) {
    ctx->trap = JIT_TRAP_DIVIDE_BY_ZERO;
    return;
  }

  uint64_t result = b == -1 ? 0 : (uint64_t)(a % b);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_rem_u(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  if (b == 0) {
    ctx->trap = JIT_TRAP_DIVIDE_BY_ZERO;
    return;
  }

  uint64_t result = a % b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_eqz(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a == 0;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_eq(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a == b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_ne(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a != b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_lt_s(VMContext *ctx) {
  int64_t b = (int64_t)ctx->stack[--ctx->stack_height];
  int64_t a = (int64_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a < b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_lt_u(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a < b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_gt_s(VMContext *ctx) {
  int64_t b = (int64_t)ctx->stack[--ctx->stack_height];
  int64_t a = (int64_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a > b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_gt_u(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a > b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_le_s(VMContext *ctx) {
  int64_t b = (int64_t)ctx->stack[--ctx->stack_height];
  int64_t a = (int64_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a <= b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_le_u(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a <= b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_ge_s(VMContext *ctx) {
  int64_t b = (int64_t)ctx->stack[--ctx->stack_height];
  int64_t a = (int64_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a >= b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_ge_u(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a >= b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_abs(VMContext *ctx) {
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a & ~UINT32_C(0x80000000);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_neg(VMContext *ctx) {
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = a ^ UINT32_C(0x80000000);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_ceil(VMContext *ctx) {
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  float result = __builtin_ceilf(a);

  ctx->stack[ctx->stack_height++] = F32_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_floor(VMContext *ctx) {
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  float result = __builtin_floorf(a);

  ctx->stack[ctx->stack_height++] = F32_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_trunc(VMContext *ctx) {
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  float result = __builtin_truncf(a);

  ctx->stack[ctx->stack_height++] = F32_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_nearest(VMContext *ctx) {
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  float result = __builtin_roundevenf(a);

  ctx->stack[ctx->stack_height++] = F32_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_sqrt(VMContext *ctx) {
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  float result = __builtin_sqrtf(a);

  ctx->stack[ctx->stack_height++] = F32_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_add(VMContext *ctx) {
  float b = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  float result = a + b;

  ctx->stack[ctx->stack_height++] = F32_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_sub(VMContext *ctx) {
  float b = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  float result = a - b;

  ctx->stack[ctx->stack_height++] = F32_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_mul(VMContext *ctx) {
  float b = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  float result = a * b;

  ctx->stack[ctx->stack_height++] = F32_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_div(VMContext *ctx) {
  float b = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  float result = a / b;

  ctx->stack[ctx->stack_height++] = F32_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_eq(VMContext *ctx) {
  float b = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  uint32_t result = a == b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_ne(VMContext *ctx) {
  float b = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  uint32_t result = a != b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_lt(VMContext *ctx) {
  float b = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  uint32_t result = a < b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_gt(VMContext *ctx) {
  float b = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  uint32_t result = a > b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_le(VMContext *ctx) {
  float b = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  uint32_t result = a <= b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_ge(VMContext *ctx) {
  float b = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  uint32_t result = a >= b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_min(VMContext *ctx) {
  uint32_t b_bits = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a_bits = (uint32_t)ctx->stack[--ctx->stack_height];
  float a = F32_FROM_BITS(a_bits);
  float b = F32_FROM_BITS(b_bits);

  // Propagate NaNs and choose the correct sign when both operands are zero.
  uint32_t result;
  if (__builtin_isnan(a) || __builtin_isnan(b)) {
    result = UINT32_C(0x7fc00000);
  } else if (a == b) {
    result = a_bits | b_bits;
  } else {
    result = a < b ? a_bits : b_bits;
  }

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_max(VMContext *ctx) {
  uint32_t b_bits = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a_bits = (uint32_t)ctx->stack[--ctx->stack_height];
  float a = F32_FROM_BITS(a_bits);
  float b = F32_FROM_BITS(b_bits);

  // Propagate NaNs and choose the correct sign when both operands are zero.
  uint32_t result;
  if (__builtin_isnan(a) || __builtin_isnan(b)) {
    result = UINT32_C(0x7fc00000);
  } else if (a == b) {
    result = a_bits & b_bits;
  } else {
    result = a > b ? a_bits : b_bits;
  }

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_copysign(VMContext *ctx) {
  uint32_t b = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  uint32_t result = (a & ~UINT32_C(0x80000000)) | (b & UINT32_C(0x80000000));

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_abs(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a & ~UINT64_C(0x8000000000000000);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_neg(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a ^ UINT64_C(0x8000000000000000);

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_ceil(VMContext *ctx) {
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double result = __builtin_ceil(a);

  ctx->stack[ctx->stack_height++] = F64_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_floor(VMContext *ctx) {
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double result = __builtin_floor(a);

  ctx->stack[ctx->stack_height++] = F64_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_trunc(VMContext *ctx) {
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double result = __builtin_trunc(a);

  ctx->stack[ctx->stack_height++] = F64_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_nearest(VMContext *ctx) {
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double result = __builtin_roundeven(a);

  ctx->stack[ctx->stack_height++] = F64_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_sqrt(VMContext *ctx) {
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double result = __builtin_sqrt(a);

  ctx->stack[ctx->stack_height++] = F64_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_add(VMContext *ctx) {
  double b = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double result = a + b;

  ctx->stack[ctx->stack_height++] = F64_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_sub(VMContext *ctx) {
  double b = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double result = a - b;

  ctx->stack[ctx->stack_height++] = F64_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_mul(VMContext *ctx) {
  double b = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double result = a * b;

  ctx->stack[ctx->stack_height++] = F64_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_div(VMContext *ctx) {
  double b = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double result = a / b;

  ctx->stack[ctx->stack_height++] = F64_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_eq(VMContext *ctx) {
  double b = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  uint32_t result = a == b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_ne(VMContext *ctx) {
  double b = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  uint32_t result = a != b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_lt(VMContext *ctx) {
  double b = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  uint32_t result = a < b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_gt(VMContext *ctx) {
  double b = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  uint32_t result = a > b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_le(VMContext *ctx) {
  double b = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  uint32_t result = a <= b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_ge(VMContext *ctx) {
  double b = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  uint32_t result = a >= b;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_min(VMContext *ctx) {
  uint64_t b_bits = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a_bits = (uint64_t)ctx->stack[--ctx->stack_height];
  double a = F64_FROM_BITS(a_bits);
  double b = F64_FROM_BITS(b_bits);

  // Propagate NaNs and choose the correct sign when both operands are zero.
  uint64_t result;
  if (__builtin_isnan(a) || __builtin_isnan(b)) {
    result = UINT64_C(0x7ff8000000000000);
  } else if (a == b) {
    result = a_bits | b_bits;
  } else {
    result = a < b ? a_bits : b_bits;
  }

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_max(VMContext *ctx) {
  uint64_t b_bits = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a_bits = (uint64_t)ctx->stack[--ctx->stack_height];
  double a = F64_FROM_BITS(a_bits);
  double b = F64_FROM_BITS(b_bits);

  // Propagate NaNs and choose the correct sign when both operands are zero.
  uint64_t result;
  if (__builtin_isnan(a) || __builtin_isnan(b)) {
    result = UINT64_C(0x7ff8000000000000);
  } else if (a == b) {
    result = a_bits & b_bits;
  } else {
    result = a > b ? a_bits : b_bits;
  }

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_copysign(VMContext *ctx) {
  uint64_t b = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result =
      (a & ~UINT64_C(0x8000000000000000)) | (b & UINT64_C(0x8000000000000000));

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_const(VMContext *ctx) {
  uint64_t value = (uint64_t)(uintptr_t)&_JIT_PARAM_0;

  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_const(VMContext *ctx) {
  uint32_t value = (uint32_t)(uintptr_t)&_JIT_PARAM_0;

  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_const(VMContext *ctx) {
  uint64_t value = (uint64_t)(uintptr_t)&_JIT_PARAM_0;

  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_wrap_i64(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = (uint32_t)a;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_extend_i32_s(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = (uint64_t)(int64_t)(int32_t)a;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_extend_i32_u(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = (uint32_t)a;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_reinterpret_f32(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = (uint32_t)a;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_reinterpret_f64(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_reinterpret_i32(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = (uint32_t)a;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_reinterpret_i64(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = a;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_extend8_s(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = (uint32_t)(int32_t)(int8_t)a;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_extend16_s(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = (uint32_t)(int32_t)(int16_t)a;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_extend8_s(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = (uint64_t)(int64_t)(int8_t)a;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_extend16_s(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = (uint64_t)(int64_t)(int16_t)a;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_extend32_s(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  uint64_t result = (uint64_t)(int64_t)(int32_t)a;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_convert_i32_s(VMContext *ctx) {
  int32_t a = (int32_t)ctx->stack[--ctx->stack_height];

  float result = (float)a;

  ctx->stack[ctx->stack_height++] = F32_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_convert_i32_u(VMContext *ctx) {
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  float result = (float)a;

  ctx->stack[ctx->stack_height++] = F32_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_convert_i64_s(VMContext *ctx) {
  int64_t a = (int64_t)ctx->stack[--ctx->stack_height];

  float result = (float)a;

  ctx->stack[ctx->stack_height++] = F32_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_convert_i64_u(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  float result = (float)a;

  ctx->stack[ctx->stack_height++] = F32_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_convert_i32_s(VMContext *ctx) {
  int32_t a = (int32_t)ctx->stack[--ctx->stack_height];

  double result = (double)a;

  ctx->stack[ctx->stack_height++] = F64_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_convert_i32_u(VMContext *ctx) {
  uint32_t a = (uint32_t)ctx->stack[--ctx->stack_height];

  double result = (double)a;

  ctx->stack[ctx->stack_height++] = F64_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_convert_i64_s(VMContext *ctx) {
  int64_t a = (int64_t)ctx->stack[--ctx->stack_height];

  double result = (double)a;

  ctx->stack[ctx->stack_height++] = F64_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_convert_i64_u(VMContext *ctx) {
  uint64_t a = (uint64_t)ctx->stack[--ctx->stack_height];

  double result = (double)a;

  ctx->stack[ctx->stack_height++] = F64_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f32_demote_f64(VMContext *ctx) {
  double a = F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  float result = (float)a;

  ctx->stack[ctx->stack_height++] = F32_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void f64_promote_f32(VMContext *ctx) {
  float a = F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double result = (double)a;

  ctx->stack[ctx->stack_height++] = F64_TO_BITS(result);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_trunc_f32_s(VMContext *ctx) {
  double a = (double)F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  if (__builtin_isnan(t)) {
    ctx->trap = JIT_TRAP_INVALID_CONVERSION;
    return;
  }

  if (t < -0x1p31 || t >= 0x1p31) {
    ctx->trap = JIT_TRAP_INTEGER_OVERFLOW;
    return;
  }

  uint32_t result = (uint32_t)(int32_t)t;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_trunc_sat_f32_s(VMContext *ctx) {
  double a = (double)F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  uint32_t result;
  if (__builtin_isnan(t)) {
    result = 0;
  } else if (t <= -0x1p31) {
    result = (uint32_t)INT32_MIN;
  } else if (t >= 0x1p31) {
    result = (uint32_t)INT32_MAX;
  } else {
    result = (uint32_t)(int32_t)t;
  }

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_trunc_f32_u(VMContext *ctx) {
  double a = (double)F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  if (__builtin_isnan(t)) {
    ctx->trap = JIT_TRAP_INVALID_CONVERSION;
    return;
  }

  if (t < 0.0 || t >= 0x1p32) {
    ctx->trap = JIT_TRAP_INTEGER_OVERFLOW;
    return;
  }

  uint32_t result = (uint32_t)(uint32_t)t;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_trunc_sat_f32_u(VMContext *ctx) {
  double a = (double)F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  uint32_t result;
  if (__builtin_isnan(t)) {
    result = 0;
  } else if (t <= 0.0) {
    result = (uint32_t)0;
  } else if (t >= 0x1p32) {
    result = (uint32_t)UINT32_MAX;
  } else {
    result = (uint32_t)(uint32_t)t;
  }

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_trunc_f64_s(VMContext *ctx) {
  double a = (double)F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  if (__builtin_isnan(t)) {
    ctx->trap = JIT_TRAP_INVALID_CONVERSION;
    return;
  }

  if (t < -0x1p31 || t >= 0x1p31) {
    ctx->trap = JIT_TRAP_INTEGER_OVERFLOW;
    return;
  }

  uint32_t result = (uint32_t)(int32_t)t;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_trunc_sat_f64_s(VMContext *ctx) {
  double a = (double)F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  uint32_t result;
  if (__builtin_isnan(t)) {
    result = 0;
  } else if (t <= -0x1p31) {
    result = (uint32_t)INT32_MIN;
  } else if (t >= 0x1p31) {
    result = (uint32_t)INT32_MAX;
  } else {
    result = (uint32_t)(int32_t)t;
  }

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_trunc_f64_u(VMContext *ctx) {
  double a = (double)F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  if (__builtin_isnan(t)) {
    ctx->trap = JIT_TRAP_INVALID_CONVERSION;
    return;
  }

  if (t < 0.0 || t >= 0x1p32) {
    ctx->trap = JIT_TRAP_INTEGER_OVERFLOW;
    return;
  }

  uint32_t result = (uint32_t)(uint32_t)t;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_trunc_sat_f64_u(VMContext *ctx) {
  double a = (double)F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  uint32_t result;
  if (__builtin_isnan(t)) {
    result = 0;
  } else if (t <= 0.0) {
    result = (uint32_t)0;
  } else if (t >= 0x1p32) {
    result = (uint32_t)UINT32_MAX;
  } else {
    result = (uint32_t)(uint32_t)t;
  }

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_trunc_f32_s(VMContext *ctx) {
  double a = (double)F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  if (__builtin_isnan(t)) {
    ctx->trap = JIT_TRAP_INVALID_CONVERSION;
    return;
  }

  if (t < -0x1p63 || t >= 0x1p63) {
    ctx->trap = JIT_TRAP_INTEGER_OVERFLOW;
    return;
  }

  uint64_t result = (uint64_t)(int64_t)t;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_trunc_sat_f32_s(VMContext *ctx) {
  double a = (double)F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  uint64_t result;
  if (__builtin_isnan(t)) {
    result = 0;
  } else if (t <= -0x1p63) {
    result = (uint64_t)INT64_MIN;
  } else if (t >= 0x1p63) {
    result = (uint64_t)INT64_MAX;
  } else {
    result = (uint64_t)(int64_t)t;
  }

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_trunc_f32_u(VMContext *ctx) {
  double a = (double)F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  if (__builtin_isnan(t)) {
    ctx->trap = JIT_TRAP_INVALID_CONVERSION;
    return;
  }

  if (t < 0.0 || t >= 0x1p64) {
    ctx->trap = JIT_TRAP_INTEGER_OVERFLOW;
    return;
  }

  uint64_t result = (uint64_t)(uint64_t)t;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_trunc_sat_f32_u(VMContext *ctx) {
  double a = (double)F32_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  uint64_t result;
  if (__builtin_isnan(t)) {
    result = 0;
  } else if (t <= 0.0) {
    result = (uint64_t)0;
  } else if (t >= 0x1p64) {
    result = (uint64_t)UINT64_MAX;
  } else {
    result = (uint64_t)(uint64_t)t;
  }

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_trunc_f64_s(VMContext *ctx) {
  double a = (double)F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  if (__builtin_isnan(t)) {
    ctx->trap = JIT_TRAP_INVALID_CONVERSION;
    return;
  }

  if (t < -0x1p63 || t >= 0x1p63) {
    ctx->trap = JIT_TRAP_INTEGER_OVERFLOW;
    return;
  }

  uint64_t result = (uint64_t)(int64_t)t;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_trunc_sat_f64_s(VMContext *ctx) {
  double a = (double)F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  uint64_t result;
  if (__builtin_isnan(t)) {
    result = 0;
  } else if (t <= -0x1p63) {
    result = (uint64_t)INT64_MIN;
  } else if (t >= 0x1p63) {
    result = (uint64_t)INT64_MAX;
  } else {
    result = (uint64_t)(int64_t)t;
  }

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_trunc_f64_u(VMContext *ctx) {
  double a = (double)F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  if (__builtin_isnan(t)) {
    ctx->trap = JIT_TRAP_INVALID_CONVERSION;
    return;
  }

  if (t < 0.0 || t >= 0x1p64) {
    ctx->trap = JIT_TRAP_INTEGER_OVERFLOW;
    return;
  }

  uint64_t result = (uint64_t)(uint64_t)t;

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_trunc_sat_f64_u(VMContext *ctx) {
  double a = (double)F64_FROM_BITS(ctx->stack[--ctx->stack_height]);

  double t = __builtin_trunc(a);

  uint64_t result;
  if (__builtin_isnan(t)) {
    result = 0;
  } else if (t <= 0.0) {
    result = (uint64_t)0;
  } else if (t >= 0x1p64) {
    result = (uint64_t)UINT64_MAX;
  } else {
    result = (uint64_t)(uint64_t)t;
  }

  ctx->stack[ctx->stack_height++] = result;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void memory_init(VMContext *ctx) {
  uint32_t length = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t source = (uint32_t)ctx->stack[--ctx->stack_height];
  uint64_t offset = ctx->stack[--ctx->stack_height];

  uint32_t segment = (uint32_t)(uintptr_t)&_JIT_PARAM_0;
  uint32_t destination = (uint32_t)(uintptr_t)&_JIT_PARAM_1;

  ctx->trap = ctx->bulk_operation(ctx->runtime, JIT_BULK_MEMORY_INIT, segment,
                                  destination, offset, source, length);

  if (ctx->trap != JIT_TRAP_NONE) {
    return;
  }

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void data_drop(VMContext *ctx) {
  uint32_t segment = (uint32_t)(uintptr_t)&_JIT_PARAM_0;

  ctx->trap = ctx->bulk_operation(ctx->runtime, JIT_BULK_DATA_DROP, segment,
                                  0, 0, 0, 0);

  if (ctx->trap != JIT_TRAP_NONE) {
    return;
  }

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void table_init(VMContext *ctx) {
  uint32_t length = (uint32_t)ctx->stack[--ctx->stack_height];
  uint32_t source = (uint32_t)ctx->stack[--ctx->stack_height];
  uint64_t offset = ctx->stack[--ctx->stack_height];

  uint32_t segment = (uint32_t)(uintptr_t)&_JIT_PARAM_0;
  uint32_t destination = (uint32_t)(uintptr_t)&_JIT_PARAM_1;

  ctx->trap = ctx->bulk_operation(ctx->runtime, JIT_BULK_TABLE_INIT, segment,
                                  destination, offset, source, length);

  if (ctx->trap != JIT_TRAP_NONE) {
    return;
  }

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void elem_drop(VMContext *ctx) {
  uint32_t segment = (uint32_t)(uintptr_t)&_JIT_PARAM_0;

  ctx->trap = ctx->bulk_operation(ctx->runtime, JIT_BULK_ELEM_DROP, segment,
                                  0, 0, 0, 0);

  if (ctx->trap != JIT_TRAP_NONE) {
    return;
  }

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_load(VMContext *ctx) {
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 4);

  if (address == 0) {
    return;
  }

  uint32_t value;
  __builtin_memcpy(&value, address, 4);

  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_load8_s(VMContext *ctx) {
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 1);

  if (address == 0) {
    return;
  }

  int8_t value;
  __builtin_memcpy(&value, address, 1);

  ctx->stack[ctx->stack_height++] = (uint32_t)(int32_t)value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_load8_u(VMContext *ctx) {
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 1);

  if (address == 0) {
    return;
  }

  uint8_t value;
  __builtin_memcpy(&value, address, 1);

  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_load16_s(VMContext *ctx) {
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 2);

  if (address == 0) {
    return;
  }

  int16_t value;
  __builtin_memcpy(&value, address, 2);

  ctx->stack[ctx->stack_height++] = (uint32_t)(int32_t)value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_load16_u(VMContext *ctx) {
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 2);

  if (address == 0) {
    return;
  }

  uint16_t value;
  __builtin_memcpy(&value, address, 2);

  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_store(VMContext *ctx) {
  uint32_t value = (uint32_t)ctx->stack[--ctx->stack_height];
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 4);

  if (address == 0) {
    return;
  }

  __builtin_memcpy(address, &value, 4);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_store8(VMContext *ctx) {
  uint8_t value = (uint8_t)ctx->stack[--ctx->stack_height];
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 1);

  if (address == 0) {
    return;
  }

  __builtin_memcpy(address, &value, 1);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i32_store16(VMContext *ctx) {
  uint16_t value = (uint16_t)ctx->stack[--ctx->stack_height];
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 2);

  if (address == 0) {
    return;
  }

  __builtin_memcpy(address, &value, 2);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void local_get_i32_load(VMContext *ctx) {
  uint64_t base = ctx->locals[(uintptr_t)&_JIT_PARAM_2];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 4);

  if (address == 0) {
    return;
  }

  uint32_t value;
  __builtin_memcpy(&value, address, 4);

  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void local_get_i32_store(VMContext *ctx) {
  uint32_t value = (uint32_t)ctx->locals[(uintptr_t)&_JIT_PARAM_2];
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 4);

  if (address == 0) {
    return;
  }

  __builtin_memcpy(address, &value, 4);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_load(VMContext *ctx) {
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 8);

  if (address == 0) {
    return;
  }

  uint64_t value;
  __builtin_memcpy(&value, address, 8);

  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_load8_s(VMContext *ctx) {
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 1);

  if (address == 0) {
    return;
  }

  int8_t value;
  __builtin_memcpy(&value, address, 1);

  ctx->stack[ctx->stack_height++] = (uint64_t)(int64_t)value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_load8_u(VMContext *ctx) {
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 1);

  if (address == 0) {
    return;
  }

  uint8_t value;
  __builtin_memcpy(&value, address, 1);

  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_load16_s(VMContext *ctx) {
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 2);

  if (address == 0) {
    return;
  }

  int16_t value;
  __builtin_memcpy(&value, address, 2);

  ctx->stack[ctx->stack_height++] = (uint64_t)(int64_t)value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_load16_u(VMContext *ctx) {
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 2);

  if (address == 0) {
    return;
  }

  uint16_t value;
  __builtin_memcpy(&value, address, 2);

  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_load32_s(VMContext *ctx) {
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 4);

  if (address == 0) {
    return;
  }

  int32_t value;
  __builtin_memcpy(&value, address, 4);

  ctx->stack[ctx->stack_height++] = (uint64_t)(int64_t)value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_load32_u(VMContext *ctx) {
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 4);

  if (address == 0) {
    return;
  }

  uint32_t value;
  __builtin_memcpy(&value, address, 4);

  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_store(VMContext *ctx) {
  uint64_t value = (uint64_t)ctx->stack[--ctx->stack_height];
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 8);

  if (address == 0) {
    return;
  }

  __builtin_memcpy(address, &value, 8);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_store8(VMContext *ctx) {
  uint8_t value = (uint8_t)ctx->stack[--ctx->stack_height];
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 1);

  if (address == 0) {
    return;
  }

  __builtin_memcpy(address, &value, 1);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_store16(VMContext *ctx) {
  uint16_t value = (uint16_t)ctx->stack[--ctx->stack_height];
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 2);

  if (address == 0) {
    return;
  }

  __builtin_memcpy(address, &value, 2);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void i64_store32(VMContext *ctx) {
  uint32_t value = (uint32_t)ctx->stack[--ctx->stack_height];
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 4);

  if (address == 0) {
    return;
  }

  __builtin_memcpy(address, &value, 4);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void local_get_i64_load(VMContext *ctx) {
  uint64_t base = ctx->locals[(uintptr_t)&_JIT_PARAM_2];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 8);

  if (address == 0) {
    return;
  }

  uint64_t value;
  __builtin_memcpy(&value, address, 8);

  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void local_get_i64_store(VMContext *ctx) {
  uint64_t value = (uint64_t)ctx->locals[(uintptr_t)&_JIT_PARAM_2];
  uint64_t base = ctx->stack[--ctx->stack_height];
  uintptr_t memory = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t offset = (uintptr_t)&_JIT_PARAM_1;

  uint8_t *address = memory_address(ctx, memory, base, offset, 8);

  if (address == 0) {
    return;
  }

  __builtin_memcpy(address, &value, 8);

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void global_get(VMContext *ctx) {
  uintptr_t index = (uintptr_t)&_JIT_PARAM_0;
  uint64_t value = *ctx->globals[index];

  ctx->stack[ctx->stack_height++] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void global_set(VMContext *ctx) {
  uintptr_t index = (uintptr_t)&_JIT_PARAM_0;
  uint64_t value = ctx->stack[--ctx->stack_height];

  *ctx->globals[index] = value;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void unreachable(VMContext *ctx) {
  ctx->trap = JIT_TRAP_UNREACHABLE;
}

void call(VMContext *ctx) {
  ctx->call_function = (uintptr_t)&_JIT_PARAM_0;
  ctx->resume_pc = (uintptr_t)&_JIT_PARAM_1;
  ctx->exit_reason = JIT_EXIT_CALL;
}

void drop(VMContext *ctx) {
  ctx->stack_height--;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void select_value(VMContext *ctx) {
  uint32_t condition = (uint32_t)ctx->stack[--ctx->stack_height];
  uint64_t second = ctx->stack[--ctx->stack_height];
  uint64_t first = ctx->stack[--ctx->stack_height];

  ctx->stack[ctx->stack_height++] = condition != 0 ? first : second;

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void local_get_local_set(VMContext *ctx) {
  uintptr_t source = (uintptr_t)&_JIT_PARAM_0;
  uintptr_t destination = (uintptr_t)&_JIT_PARAM_1;

  ctx->locals[destination] = ctx->locals[source];

  __attribute__((musttail)) return _JIT_CONTINUE(ctx);
}

void local_get_return(VMContext *ctx) {
  uintptr_t index = (uintptr_t)&_JIT_PARAM_0;
  uint64_t value = ctx->locals[index];

  ctx->stack[ctx->stack_height++] = value;
}
