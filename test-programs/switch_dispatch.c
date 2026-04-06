// Switch dispatch — exercises br_table via a big switch in a loop
#define NUM_OPS 1000000

static int registers[16];

int switch_bench(void) {
    // Initialize registers
    for (int i = 0; i < 16; i++) {
        registers[i] = i + 1;
    }

    unsigned int pc = 0;
    unsigned int seed = 0xDEADBEEF;

    for (int i = 0; i < NUM_OPS; i++) {
        // LCG to generate pseudo-random opcodes
        seed = seed * 1664525 + 1013904223;
        int opcode = (seed >> 16) % 20;
        int ra = (seed >> 8) & 0xF;
        int rb = (seed >> 4) & 0xF;
        int rc = seed & 0xF;

        switch (opcode) {
            case 0:  registers[ra] = registers[rb] + registers[rc]; break;
            case 1:  registers[ra] = registers[rb] - registers[rc]; break;
            case 2:  registers[ra] = registers[rb] * registers[rc]; break;
            case 3:  registers[ra] = registers[rb] & registers[rc]; break;
            case 4:  registers[ra] = registers[rb] | registers[rc]; break;
            case 5:  registers[ra] = registers[rb] ^ registers[rc]; break;
            case 6:  registers[ra] = registers[rb] << (registers[rc] & 31); break;
            case 7:  registers[ra] = registers[rb] >> (registers[rc] & 31); break;
            case 8:  registers[ra] = ~registers[rb]; break;
            case 9:  registers[ra] = -registers[rb]; break;
            case 10: registers[ra] = registers[rb] + 1; break;
            case 11: registers[ra] = registers[rb] - 1; break;
            case 12: registers[ra] = (registers[rb] < registers[rc]) ? 1 : 0; break;
            case 13: registers[ra] = (registers[rb] == registers[rc]) ? 1 : 0; break;
            case 14: registers[ra] = (registers[rb] != registers[rc]) ? 1 : 0; break;
            case 15: registers[ra] = (registers[rb] > registers[rc]) ? 1 : 0; break;
            case 16: registers[ra] = registers[rb] + registers[rc] + 1; break;
            case 17: registers[ra] = (registers[rb] ^ registers[rc]) + registers[ra]; break;
            case 18: registers[ra] = (registers[rb] & 0xFF) | (registers[rc] << 8); break;
            case 19: registers[ra] = ((unsigned int)registers[rb] >> 16) | (registers[rc] << 16); break;
        }
    }

    // Checksum all registers
    int sum = 0;
    for (int i = 0; i < 16; i++) {
        sum ^= registers[i];
    }
    return sum;
}
