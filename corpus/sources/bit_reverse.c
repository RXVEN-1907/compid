#include <stdio.h>
#include <stdint.h>

/* Reverse all 32 bits, plus byte-order swaps. */
static uint32_t rev_bits(uint32_t v) {
    v = ((v & 0x55555555u) << 1) | ((v >> 1) & 0x55555555u);
    v = ((v & 0x33333333u) << 2) | ((v >> 2) & 0x33333333u);
    v = ((v & 0x0F0F0F0Fu) << 4) | ((v >> 4) & 0x0F0F0F0Fu);
    v = ((v & 0x00FF00FFu) << 8) | ((v >> 8) & 0x00FF00FFu);
    return (v << 16) | (v >> 16);
}

static int is_palindrome32(uint32_t v) { return rev_bits(v) == v; }

int main(void) {
    long acc = 0;
    for (uint32_t i = 0; i < 200000; i++) {
        uint32_t r = rev_bits(i);
        acc += (r >> 24) ^ (i & 0xFF);
        if (is_palindrome32(i)) acc += 7;
        if ((i & 0xFFFF) == 0x00AA) acc += __builtin_bswap32(i) & 0xFFFF;
    }
    printf("bitrev acc=%ld\n", acc);
    return 0;
}
