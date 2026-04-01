
#ifndef STENCIL_CONTEXT_H
#define STENCIL_CONTEXT_H

#include <stdint.h>

typedef struct StencilContext StencilContext;

// every stencil must be a funciton with this signature
typedef void (*StencilFn)(StencilContext *ctx);

typedef struct {
  uint64_t imm0;
  uint64_t imm1;
} OpImmediate;

// this is the entire execution state, passed to every stencil
struct StencilContext {
  uint64_t *stack;
  uint64_t stack_pointer;
  uint64_t *locals;
  uint64_t *mem_base;
  uint64_t mem_len;
  uint64_t *globals;
  const OpImmediate *imm_table;
  const StencilFn *fn_table;
  uint32_t pc;
  uint8_t snapshot_flag;
  uint8_t exit_reason;
  uint32_t exit_value;
};

#define EXIT_SNAPSHOT 0
#define EXIT_RETURN 1

#define CHECK_SNAPSHOT(ctx) \
  if ((ctx)->snapshot_flag) { \
    (ctx)->exit_reason = EXIT_SNAPSHOT; \
    return; \
  }

#endif
