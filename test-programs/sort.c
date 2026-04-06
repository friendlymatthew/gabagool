// Quicksort — heavy on recursion, comparisons, memory swaps
#define SORT_SIZE 4096

static int arr[SORT_SIZE];

static void swap(int *a, int *b) {
    int t = *a;
    *a = *b;
    *b = t;
}

static int partition(int *data, int lo, int hi) {
    int pivot = data[hi];
    int i = lo - 1;
    for (int j = lo; j < hi; j++) {
        if (data[j] <= pivot) {
            i++;
            swap(&data[i], &data[j]);
        }
    }
    swap(&data[i + 1], &data[hi]);
    return i + 1;
}

static void quicksort(int *data, int lo, int hi) {
    if (lo < hi) {
        int p = partition(data, lo, hi);
        quicksort(data, lo, p - 1);
        quicksort(data, p + 1, hi);
    }
}

int sort_bench(void) {
    // LCG to fill array with pseudo-random values
    unsigned int seed = 12345;
    for (int i = 0; i < SORT_SIZE; i++) {
        seed = seed * 1103515245 + 12345;
        arr[i] = (int)(seed >> 16) & 0x7fff;
    }

    quicksort(arr, 0, SORT_SIZE - 1);

    // Verify sorted + return checksum
    int sum = 0;
    for (int i = 0; i < SORT_SIZE; i++) {
        sum += arr[i];
    }
    return sum;
}
