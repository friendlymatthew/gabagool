// Binary search — random-access memory reads with branching
#define ARRAY_SIZE 65536
#define NUM_SEARCHES 100000

static int sorted[ARRAY_SIZE];

static int binary_search(int target) {
    int lo = 0, hi = ARRAY_SIZE - 1;
    while (lo <= hi) {
        int mid = lo + (hi - lo) / 2;
        if (sorted[mid] == target) return mid;
        if (sorted[mid] < target) lo = mid + 1;
        else hi = mid - 1;
    }
    return -1;
}

int binary_search_bench(void) {
    // Fill with sorted values (each ~3 apart, with gaps)
    for (int i = 0; i < ARRAY_SIZE; i++) {
        sorted[i] = i * 3 + (i & 1);
    }

    int found = 0;
    unsigned int seed = 98765;

    for (int i = 0; i < NUM_SEARCHES; i++) {
        seed = seed * 1103515245 + 12345;
        int target = (int)((seed >> 16) % (ARRAY_SIZE * 3));
        if (binary_search(target) >= 0) {
            found++;
        }
    }

    return found;
}
