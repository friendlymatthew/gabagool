// Matrix multiplication — heavy on memory loads/stores, nested loops, integer arithmetic
#define N 64

static int a[N * N];
static int b[N * N];
static int c[N * N];

void matrix_init(void) {
    for (int i = 0; i < N * N; i++) {
        a[i] = i % 97;
        b[i] = (i * 31) % 101;
        c[i] = 0;
    }
}

void matrix_multiply(void) {
    for (int i = 0; i < N; i++) {
        for (int j = 0; j < N; j++) {
            int sum = 0;
            for (int k = 0; k < N; k++) {
                sum += a[i * N + k] * b[k * N + j];
            }
            c[i * N + j] = sum;
        }
    }
}

int matrix_checksum(void) {
    int sum = 0;
    for (int i = 0; i < N * N; i++) {
        sum += c[i];
    }
    return sum;
}

// Entry point: init, multiply, return checksum
int matrix_bench(void) {
    matrix_init();
    matrix_multiply();
    return matrix_checksum();
}
