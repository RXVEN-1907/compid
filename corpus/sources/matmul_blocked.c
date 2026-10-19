#include <stdio.h>

/* Blocked (tiled) i-k-j matrix multiply: loop nest with different reuse. */
#define D 96
#define BS 24

static double A[D][D], B[D][D], C[D][D];

int main(void) {
    for (int i = 0; i < D; i++)
        for (int j = 0; j < D; j++) {
            A[i][j] = (i == j) ? 2.0 : ((i + j) % 3) * 0.5;
            B[i][j] = ((i ^ j) % 4) - 1.5;
        }
    for (int ii = 0; ii < D; ii += BS)
        for (int kk = 0; kk < D; kk += BS)
            for (int jj = 0; jj < D; jj += BS)
                for (int i = ii; i < ii + BS; i++)
                    for (int k = kk; k < kk + BS; k++) {
                        double aik = A[i][k];
                        for (int j = jj; j < jj + BS; j++)
                            C[i][j] += aik * B[k][j];
                    }
    double acc = 0;
    for (int i = 0; i < D; i += 5)
        for (int j = 0; j < D; j += 7) acc += C[i][j];
    printf("matmulblk acc=%.6f\n", acc);
    return 0;
}
