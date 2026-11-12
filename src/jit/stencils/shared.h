#ifndef GABAGOOL_STENCIL_H
#define GABAGOOL_STENCIL_H

#include <stdint.h>

enum {
    JIT_TRAP_NONE = 0,
    JIT_TRAP_DIVIDE_BY_ZERO = 1,
    JIT_TRAP_INTEGER_OVERFLOW = 2,
    JIT_TRAP_INVALID_CONVERSION = 3,
    JIT_TRAP_MEMORY_OUT_OF_BOUNDS = 4,
    JIT_TRAP_TABLE_OUT_OF_BOUNDS = 5,
    JIT_TRAP_UNREACHABLE = 6,
};

enum {
    JIT_EXIT_RETURN = 0,
    JIT_EXIT_CALL = 1,
};

enum {
    JIT_BULK_MEMORY_INIT = 0,
    JIT_BULK_DATA_DROP = 1,
    JIT_BULK_TABLE_INIT = 2,
    JIT_BULK_ELEM_DROP = 3,
    JIT_BULK_MEMORY_COPY = 4,
    JIT_BULK_MEMORY_FILL = 5,
};

typedef uint64_t (*BulkOperation)(void *runtime, uint32_t operation,
                                  uint32_t segment, uint32_t destination,
                                  uint64_t offset, uint64_t source, uint64_t length);

typedef struct MemoryView {
    uint8_t *bytes;
    uint64_t length;
    uint64_t address64;
} MemoryView;

typedef struct VMContext {
    uint64_t *locals;
    uint64_t *stack;
    uint64_t stack_height;

    // note: zero means success
    uint64_t trap;

    void *runtime;
    BulkOperation bulk_operation;
    const MemoryView *memories;
    uint64_t *const *globals;

    uint64_t exit_reason;
    uint64_t call_function;
    uint64_t resume_pc;
} VMContext;

static __attribute__((always_inline)) inline uint8_t *memory_address(
    VMContext *ctx, uintptr_t index, uint64_t base, uint64_t offset,
    uint64_t width) {
    MemoryView memory = ctx->memories[index];
    base = memory.address64 ? base : (uint32_t)base;

    if (base > memory.length || offset > memory.length - base ||
        width > memory.length - base - offset) {
        ctx->trap = JIT_TRAP_MEMORY_OUT_OF_BOUNDS;
        return 0;
    }

    return memory.bytes + base + offset;
}

static __attribute__((always_inline)) inline void branch_keep_top(
    VMContext *ctx, uintptr_t keep, uintptr_t drop) {
    uint64_t source = ctx->stack_height - keep;
    uint64_t destination = source - drop;

    for (uint64_t i = 0; i < keep; i++) {
        ctx->stack[destination + i] = ctx->stack[source + i];
    }

    ctx->stack_height -= drop;
}

// convert float bit patterns without changing their value or violating aliasing rules.
#define F32_FROM_BITS(value) __builtin_bit_cast(float, (uint32_t)(value))
#define F64_FROM_BITS(value) __builtin_bit_cast(double, (uint64_t)(value))
#define F32_TO_BITS(value) __builtin_bit_cast(uint32_t, (float)(value))
#define F64_TO_BITS(value) __builtin_bit_cast(uint64_t, (double)(value))

extern void _JIT_CONTINUE(VMContext *ctx);
extern void _JIT_TARGET(VMContext *ctx);

#endif
