#ifndef GABAGOOL_STENCIL_H
#define GABAGOOL_STENCIL_H

#include <stdint.h>

enum {
    JIT_TRAP_NONE = 0,
    JIT_TRAP_DIVIDE_BY_ZERO = 1,
    JIT_TRAP_INTEGER_OVERFLOW = 2,
    JIT_TRAP_INVALID_CONVERSION = 3,
};

typedef struct VMContext {
    uint64_t *locals;
    uint64_t *stack;
    uint64_t stack_height;

    // note: zero means success
    uint64_t trap;
} VMContext;

// convert float bit patterns without changing their value or violating aliasing rules.
#define F32_FROM_BITS(value) __builtin_bit_cast(float, (uint32_t)(value))
#define F64_FROM_BITS(value) __builtin_bit_cast(double, (uint64_t)(value))
#define F32_TO_BITS(value) __builtin_bit_cast(uint32_t, (float)(value))
#define F64_TO_BITS(value) __builtin_bit_cast(uint64_t, (double)(value))

extern void _JIT_CONTINUE(VMContext *ctx);

#endif
