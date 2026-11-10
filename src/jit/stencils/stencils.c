
#include "shared.h"
#include <stdint.h>


void local_get2(VMContext *ctx, uint64_t cached0, uint64_t cached1);
void i32_add(VMContext *ctx, uint64_t cached0, uint64_t cached1);
void return_value(VMContext *ctx, uint64_t cached0, uint64_t cached1);


extern unsigned char _JIT_LOCAL_A;
extern unsigned char _JIT_LOCAL_B;

void local_get2(VMContext *ctx, uint64_t cached0, uint64_t cached1) {
    cached0 = ctx->locals[(uintptr_t)&_JIT_LOCAL_A];
    cached1 = ctx->locals[(uintptr_t)&_JIT_LOCAL_B];

    __attribute__((musttail))
    return _JIT_CONTINUE(ctx, cached0, cached1);
}


void i32_add(VMContext *ctx, uint64_t cached0, uint64_t cached1) {
    uint64_t out = (uint32_t)cached0 + (uint32_t)cached1;

    __attribute__((musttail))
    return _JIT_CONTINUE(ctx, out, 0);
}

void return_value(VMContext *ctx, uint64_t cached0, uint64_t cached1) {
    (void)cached1;
     ctx->result = cached0;
}
