#ifndef GABAGOOL_STENCIL_H
#define GABAGOOL_STENCIL_H

#include <stdint.h>

typedef struct {
    uint64_t *locals;
    uint64_t result;
} VMContext;

extern void _JIT_CONTINUE(
    VMContext *ctx,
    uint64_t cached0,
    uint64_t cached1
);

#endif
