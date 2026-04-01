
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

#define CHECK_SNAPSHOT(ctx)                                                    \
  if ((ctx)->snapshot_flag) {                                                  \
    (ctx)->exit_reason = EXIT_SNAPSHOT;                                        \
    return;                                                                    \
  }

#define STACK_KEEP_DROP(ctx, keep, drop)                                       \
  if ((drop) > 0) {                                                            \
    uint64_t _src = (ctx)->stack_pointer - (keep);                             \
    uint64_t _dst = _src - (drop);                                             \
                                                                               \
    for (uint32_t _i = 0; _i < (keep); _i++) {                                 \
      (ctx)->stack[_dst + _i] = (ctx)->stack[_src + _i];                       \
    }                                                                          \
                                                                               \
    (ctx)->stack_pointer -= (drop);                                            \
  }

#endif
