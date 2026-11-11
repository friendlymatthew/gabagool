#ifndef GABAGOOL_STENCIL_H
#define GABAGOOL_STENCIL_H

#include <stdint.h>

enum { JIT_TRAP_NONE = 0, JIT_TRAP_DIVIDE_BY_ZERO = 1, JIT_TRAP_INTEGER_OVERFLOW = 2 };

typedef struct VMContext {
    uint64_t *locals;
    uint64_t *stack;
    uint64_t stack_height;

    // note: zero means success
    uint64_t trap;
} VMContext;

extern void _JIT_CONTINUE(VMContext *ctx);

#endif
