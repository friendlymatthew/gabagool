// Call chain — measures function call/return overhead with many small functions
__attribute__((noinline)) static int f0(int x)  { return x + 1; }
__attribute__((noinline)) static int f1(int x)  { return f0(x) + 1; }
__attribute__((noinline)) static int f2(int x)  { return f1(x) + 1; }
__attribute__((noinline)) static int f3(int x)  { return f2(x) + 1; }
__attribute__((noinline)) static int f4(int x)  { return f3(x) + 1; }
__attribute__((noinline)) static int f5(int x)  { return f4(x) + 1; }
__attribute__((noinline)) static int f6(int x)  { return f5(x) + 1; }
__attribute__((noinline)) static int f7(int x)  { return f6(x) + 1; }
__attribute__((noinline)) static int f8(int x)  { return f7(x) + 1; }
__attribute__((noinline)) static int f9(int x)  { return f8(x) + 1; }
__attribute__((noinline)) static int f10(int x) { return f9(x) + 1; }
__attribute__((noinline)) static int f11(int x) { return f10(x) + 1; }
__attribute__((noinline)) static int f12(int x) { return f11(x) + 1; }
__attribute__((noinline)) static int f13(int x) { return f12(x) + 1; }
__attribute__((noinline)) static int f14(int x) { return f13(x) + 1; }
__attribute__((noinline)) static int f15(int x) { return f14(x) + 1; }
__attribute__((noinline)) static int f16(int x) { return f15(x) + 1; }
__attribute__((noinline)) static int f17(int x) { return f16(x) + 1; }
__attribute__((noinline)) static int f18(int x) { return f17(x) + 1; }
__attribute__((noinline)) static int f19(int x) { return f18(x) + 1; }

// Each call to f19 triggers a chain of 20 function calls
int call_chain_bench(void) {
    int sum = 0;
    for (int i = 0; i < 100000; i++) {
        sum += f19(i);
    }
    return sum;
}
