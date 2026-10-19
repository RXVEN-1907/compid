#include <stdio.h>
#include <stdint.h>
#include <string.h>

/* Bitwise CRC-32 (IEEE), no table: one branch per bit. */
static uint32_t crc32_bits(const unsigned char *buf, size_t len) {
    uint32_t crc = 0xFFFFFFFFu;
    for (size_t i = 0; i < len; i++) {
        crc ^= buf[i];
        for (int b = 0; b < 8; b++) {
            uint32_t mask = -(crc & 1u);
            crc = (crc >> 1) ^ (0xEDB88320u & mask);
        }
    }
    return ~crc;
}

int main(void) {
    unsigned char msg[256];
    for (int i = 0; i < 256; i++) msg[i] = (unsigned char)(i * 7 + 3);
    uint32_t acc = 0;
    for (size_t len = 1; len <= sizeof msg; len += 7)
        acc = acc * 31u + crc32_bits(msg, len);
    printf("crc32-bit acc=%u\n", acc);
    return 0;
}
