// Matrix chain multiplication — triangular DP with diagonal fill pattern
#define N 200

// dp[i][j] = minimum cost to multiply matrices i..j
// dims[i] has dimension dims[i] x dims[i+1]
static int dp[N][N];
static int dims[N + 1];

int matrix_chain_bench(void) {
    // Generate pseudo-random matrix dimensions (10..110)
    unsigned int seed = 271828;
    for (int i = 0; i <= N; i++) {
        seed = seed * 1103515245 + 12345;
        dims[i] = 10 + ((seed >> 16) % 100);
    }

    // Initialize diagonal (single matrices cost 0)
    for (int i = 0; i < N; i++) {
        dp[i][i] = 0;
    }

    // Fill diagonals: chain length 2, 3, ..., N
    for (int len = 2; len <= N; len++) {
        for (int i = 0; i <= N - len; i++) {
            int j = i + len - 1;
            dp[i][j] = 2147483647; // INT_MAX
            for (int k = i; k < j; k++) {
                int cost = dp[i][k] + dp[k + 1][j] + dims[i] * dims[k + 1] * dims[j + 1];
                if (cost < dp[i][j]) {
                    dp[i][j] = cost;
                }
            }
        }
    }

    return dp[0][N - 1];
}
