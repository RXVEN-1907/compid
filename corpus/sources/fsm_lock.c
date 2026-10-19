#include <stdio.h>

/* Combination-lock FSM over a fixed digit stream; wrong digits reset. */
static const int combo[4] = { 3, 7, 1, 9 };

static int step_lock(int state, int digit) {
    if (digit == combo[state]) return state + 1;
    return (digit == combo[0]) ? 1 : 0;
}

int main(void) {
    unsigned s = 8675309;
    long opens = 0, resets = 0;
    for (long i = 0; i < 100000; i++) {
        s = s * 1103515245u + 12345u;
        int digit = (int)((s >> 16) % 10);
        static int state = 0;
        int ns = step_lock(state, digit);
        if (ns == 4) { opens++; ns = 0; }
        if (ns == 0 && state != 0) resets++;
        state = ns;
        if ((i & 0xFFF) == 0) state = 0;
    }
    printf("lock opens=%ld resets=%ld\n", opens, resets);
    return 0;
}
