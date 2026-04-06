// SHA-256 — heavy on bitwise ops (and, xor, rotl, shr)
typedef unsigned int uint32;

static uint32 k[64] = {
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
};

static inline uint32 rotr(uint32 x, int n) {
    return (x >> n) | (x << (32 - n));
}

static inline uint32 ch(uint32 x, uint32 y, uint32 z) {
    return (x & y) ^ (~x & z);
}

static inline uint32 maj(uint32 x, uint32 y, uint32 z) {
    return (x & y) ^ (x & z) ^ (y & z);
}

static inline uint32 sigma0(uint32 x) {
    return rotr(x, 2) ^ rotr(x, 13) ^ rotr(x, 22);
}

static inline uint32 sigma1(uint32 x) {
    return rotr(x, 6) ^ rotr(x, 11) ^ rotr(x, 25);
}

static inline uint32 gamma0(uint32 x) {
    return rotr(x, 7) ^ rotr(x, 18) ^ (x >> 3);
}

static inline uint32 gamma1(uint32 x) {
    return rotr(x, 17) ^ rotr(x, 19) ^ (x >> 10);
}

static void sha256_block(uint32 state[8], const unsigned char block[64]) {
    uint32 w[64];
    int i;

    for (i = 0; i < 16; i++) {
        w[i] = ((uint32)block[i*4] << 24) |
               ((uint32)block[i*4+1] << 16) |
               ((uint32)block[i*4+2] << 8) |
               ((uint32)block[i*4+3]);
    }
    for (i = 16; i < 64; i++) {
        w[i] = gamma1(w[i-2]) + w[i-7] + gamma0(w[i-15]) + w[i-16];
    }

    uint32 a = state[0], b = state[1], c = state[2], d = state[3];
    uint32 e = state[4], f = state[5], g = state[6], h = state[7];

    for (i = 0; i < 64; i++) {
        uint32 t1 = h + sigma1(e) + ch(e, f, g) + k[i] + w[i];
        uint32 t2 = sigma0(a) + maj(a, b, c);
        h = g; g = f; f = e; e = d + t1;
        d = c; c = b; b = a; a = t1 + t2;
    }

    state[0] += a; state[1] += b; state[2] += c; state[3] += d;
    state[4] += e; state[5] += f; state[6] += g; state[7] += h;
}

// Hash 'len' bytes of data, return first 4 bytes of digest as int
static int sha256(const unsigned char *data, int len) {
    uint32 state[8] = {
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19
    };

    unsigned char block[64];
    int i, pos = 0;

    // Process full blocks
    while (pos + 64 <= len) {
        sha256_block(state, data + pos);
        pos += 64;
    }

    // Pad last block
    int remaining = len - pos;
    for (i = 0; i < remaining; i++) block[i] = data[pos + i];
    block[remaining] = 0x80;
    for (i = remaining + 1; i < 64; i++) block[i] = 0;

    if (remaining >= 56) {
        sha256_block(state, block);
        for (i = 0; i < 64; i++) block[i] = 0;
    }

    // Length in bits (big endian)
    long long bitlen = (long long)len * 8;
    block[56] = (unsigned char)(bitlen >> 56);
    block[57] = (unsigned char)(bitlen >> 48);
    block[58] = (unsigned char)(bitlen >> 40);
    block[59] = (unsigned char)(bitlen >> 32);
    block[60] = (unsigned char)(bitlen >> 24);
    block[61] = (unsigned char)(bitlen >> 16);
    block[62] = (unsigned char)(bitlen >> 8);
    block[63] = (unsigned char)(bitlen);

    sha256_block(state, block);

    return (int)state[0];
}

// Hash 1KB of pseudo-random data 1000 times
int sha256_bench(void) {
    unsigned char data[1024];
    unsigned int seed = 42;
    for (int i = 0; i < 1024; i++) {
        seed = seed * 1103515245 + 12345;
        data[i] = (unsigned char)(seed >> 16);
    }

    int result = 0;
    for (int iter = 0; iter < 1000; iter++) {
        result = sha256(data, 1024);
        // Feed result back into data to chain iterations
        data[0] = (unsigned char)(result >> 24);
        data[1] = (unsigned char)(result >> 16);
        data[2] = (unsigned char)(result >> 8);
        data[3] = (unsigned char)(result);
    }

    return result;
}
