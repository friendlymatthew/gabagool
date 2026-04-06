// Ackermann function — extreme recursion depth, stack pressure
int ackermann(int m, int n) {
    if (m == 0) return n + 1;
    if (n == 0) return ackermann(m - 1, 1);
    return ackermann(m - 1, ackermann(m, n - 1));
}

int ackermann_bench(void) {
    // ack(3,5) = 253; deep recursion that fits within typical wasm stack limits
    return ackermann(3, 5);
}
