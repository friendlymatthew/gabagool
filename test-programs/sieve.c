// Sieve of Eratosthenes — heavy on memory access patterns, conditionals
#define SIEVE_SIZE 100000

static unsigned char sieve[SIEVE_SIZE];

int count_primes(void) {
    for (int i = 0; i < SIEVE_SIZE; i++) {
        sieve[i] = 1;
    }
    sieve[0] = 0;
    sieve[1] = 0;

    for (int i = 2; i * i < SIEVE_SIZE; i++) {
        if (sieve[i]) {
            for (int j = i * i; j < SIEVE_SIZE; j += i) {
                sieve[j] = 0;
            }
        }
    }

    int count = 0;
    for (int i = 0; i < SIEVE_SIZE; i++) {
        count += sieve[i];
    }
    return count;
}
