// Bulk memory operations — memcpy/memset throughput in linear memory
#define BUF_SIZE 65536
#define ITERATIONS 500

static unsigned char src[BUF_SIZE];
static unsigned char dst[BUF_SIZE];

int bulk_memory_bench(void) {
    // Initialize source buffer
    unsigned int seed = 77777;
    for (int i = 0; i < BUF_SIZE; i++) {
        seed = seed * 1103515245 + 12345;
        src[i] = (unsigned char)(seed >> 16);
    }

    // Repeatedly copy src -> dst with XOR modifications
    for (int iter = 0; iter < ITERATIONS; iter++) {
        // Copy
        for (int i = 0; i < BUF_SIZE; i++) {
            dst[i] = src[i];
        }

        // Modify dst and feed back into src
        for (int i = 0; i < BUF_SIZE - 1; i++) {
            src[i] = dst[i] ^ dst[i + 1];
        }
        src[BUF_SIZE - 1] = dst[BUF_SIZE - 1] ^ dst[0];
    }

    // Checksum
    int sum = 0;
    for (int i = 0; i < BUF_SIZE; i++) {
        sum += dst[i];
    }
    return sum;
}
