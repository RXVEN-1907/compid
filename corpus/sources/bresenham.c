#include <stdio.h>
#include <stdint.h>

/* Bresenham line rasterization over a 128x128 grid, many angles. */
#define W 128
#define H 128

static uint8_t grid[H][W];

static void line(int x0, int y0, int x1, int y1) {
    int dx = (x1 > x0) ? x1 - x0 : x0 - x1;
    int dy = (y1 > y0) ? y1 - y0 : y0 - y1;
    int sx = (x0 < x1) ? 1 : -1;
    int sy = (y0 < y1) ? 1 : -1;
    int err = dx - dy;
    for (;;) {
        if (x0 >= 0 && x0 < W && y0 >= 0 && y0 < H) grid[y0][x0]++;
        if (x0 == x1 && y0 == y1) break;
        int e2 = 2 * err;
        if (e2 > -dy) { err -= dy; x0 += sx; }
        if (e2 < dx) { err += dx; y0 += sy; }
    }
}

int main(void) {
    for (int a = 0; a < 360; a += 3) {
        int cx = W / 2, cy = H / 2;
        int r = 60;
        line(cx, cy, cx + r * (100 + a) / 141, cy + r * (141 - a) / 141);
    }
    unsigned acc = 0;
    for (int y = 0; y < H; y++)
        for (int x = 0; x < W; x++) acc = acc * 3u + grid[y][x];
    printf("bresenham acc=%u\n", acc);
    return 0;
}
