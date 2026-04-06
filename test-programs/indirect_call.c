// Indirect calls — exercises call_indirect via function pointers
typedef int (*op_fn)(int, int);

__attribute__((noinline)) static int op_add(int a, int b) { return a + b; }
__attribute__((noinline)) static int op_sub(int a, int b) { return a - b; }
__attribute__((noinline)) static int op_mul(int a, int b) { return a * b; }
__attribute__((noinline)) static int op_and(int a, int b) { return a & b; }
__attribute__((noinline)) static int op_or(int a, int b)  { return a | b; }
__attribute__((noinline)) static int op_xor(int a, int b) { return a ^ b; }
__attribute__((noinline)) static int op_shl(int a, int b) { return a << (b & 31); }
__attribute__((noinline)) static int op_shr(int a, int b) { return a >> (b & 31); }
__attribute__((noinline)) static int op_min(int a, int b) { return a < b ? a : b; }
__attribute__((noinline)) static int op_max(int a, int b) { return a > b ? a : b; }

static op_fn dispatch_table[10] = {
    op_add, op_sub, op_mul, op_and, op_or,
    op_xor, op_shl, op_shr, op_min, op_max
};

int indirect_call_bench(void) {
    int acc = 1;
    unsigned int seed = 0x12345678;

    for (int i = 0; i < 1000000; i++) {
        seed = seed * 1103515245 + 12345;
        int idx = (seed >> 16) % 10;
        acc = dispatch_table[idx](acc, (int)(seed & 0xFF) + 1);
        // Keep acc bounded to avoid overflow issues
        if (acc == 0) acc = 1;
    }

    return acc;
}
