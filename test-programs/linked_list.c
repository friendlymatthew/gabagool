// Linked list traversal — pointer chasing through linear memory
#define LIST_SIZE 16384
#define TRAVERSALS 200

// Node: [next_index (4 bytes), value (4 bytes)]
static int nodes[LIST_SIZE * 2];

int linked_list_bench(void) {
    // Build a shuffled linked list using Fisher-Yates
    int order[LIST_SIZE];
    for (int i = 0; i < LIST_SIZE; i++) {
        order[i] = i;
    }

    unsigned int seed = 54321;
    for (int i = LIST_SIZE - 1; i > 0; i--) {
        seed = seed * 1103515245 + 12345;
        int j = (int)((seed >> 16) % (unsigned)(i + 1));
        int tmp = order[i];
        order[i] = order[j];
        order[j] = tmp;
    }

    // Wire up the linked list in shuffled order
    for (int i = 0; i < LIST_SIZE - 1; i++) {
        int cur = order[i];
        int next = order[i + 1];
        nodes[cur * 2] = next;      // next pointer
        nodes[cur * 2 + 1] = cur;   // value
    }
    int last = order[LIST_SIZE - 1];
    nodes[last * 2] = -1;            // end of list
    nodes[last * 2 + 1] = last;

    int head = order[0];

    // Traverse the list multiple times, summing values
    int total = 0;
    for (int t = 0; t < TRAVERSALS; t++) {
        int current = head;
        while (current >= 0) {
            total += nodes[current * 2 + 1];
            current = nodes[current * 2];
        }
    }

    return total;
}
