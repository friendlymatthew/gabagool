#ifndef GABAGOOL_STENCIL_H
#define GABAGOOL_STENCIL_H

#include <stdint.h>

typedef struct VMContext {
    uint64_t *locals;
    uint64_t *stack;
    uint64_t stack_height;
} VMContext;

extern void _JIT_CONTINUE(VMContext *ctx);

#endif
