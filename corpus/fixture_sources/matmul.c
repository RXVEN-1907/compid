#include <stdio.h>
int main(void) {
    double a[2][2] = {{1, 2}, {3, 4}}, b[2][2] = {{2, 0}, {0, 2}}, c[2][2];
    for (int i = 0; i < 2; i++)
        for (int j = 0; j < 2; j++) {
            double s = 0;
            for (int k = 0; k < 2; k++) s += a[i][k] * b[k][j];
            c[i][j] = s;
        }
    printf("fixture matmul %.0f %.0f %.0f %.0f\n", c[0][0], c[0][1], c[1][0], c[1][1]);
    return 0;
}
