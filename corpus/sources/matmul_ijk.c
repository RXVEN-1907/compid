#include <stdio.h>

/* Plain i-j-k matrix multiply, the canonical vectorization target. */
#define D 64

static double A[D][D], B[D][D], C[D][D];

int main(void) {
    for (int i = 0; i < D; i++)
        for (int j = 0; j < D; j++) {
            A[i][j] = ((i * 3 + j) % 7) - 3.0;
            B[i][j] = ((i * 5 + j * 11) % 5) * 0.25;
        }
    for (int i = 0; i < D; i++)
        for (int j = 0; j < D; j++) {
            double s = 0.0;
            for (int k = 0; k < D; k++) s += A[i][k] * B[k][j];
            C[i][j] = s;
        }
    double acc = 0;
    for (int i = 0; i < D; i++)
        for (int j = 0; j < D; j++) acc += C[i][j] * (i - j);
    printf("matmul acc=%.6f\n", acc);
    return 0;
}
