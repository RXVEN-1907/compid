#include <stdio.h>
#include <stdint.h>

/* Table-driven CRC-32: the 256-entry table is generated at startup. */
static uint32_t table[256];

static void crc_init(void) {
    for (uint32_t i = 0; i < 256; i++) {
        uint32_t c = i;
        for (int k = 0; k < 8; k++)
            c = (c & 1u) ? 0xEDB88320u ^ (c >> 1) : c >> 1;
        table[i] = c;
    }
}

static uint32_t crc32(const unsigned char *buf, unsigned len) {
    uint32_t crc = 0xFFFFFFFFu;
    for (unsigned i = 0; i < len; i++)
        crc = table[(crc ^ buf[i]) & 0xFFu] ^ (crc >> 8);
    return ~crc;
}

int main(void) {
    crc_init();
    unsigned char data[1024];
    for (unsigned i = 0; i < 1024; i++) data[i] = (unsigned char)(i ^ (i >> 3));
    uint32_t acc = 0;
    for (unsigned off = 0; off < 512; off += 9)
        acc ^= crc32(data + off, 512);
    printf("crc32-tbl acc=%u t0=%u t255=%u\n", acc, table[0], table[255]);
    return 0;
}
