#include <stdio.h>
static unsigned djb2(const char *s) {
    unsigned h = 5381u;
    while (*s) h = h * 33u ^ (unsigned char)*s++;
    return h;
}
int main(void) {
    unsigned acc = 0;
    const char *w[] = {"alpha", "beta", "gamma"};
    for (int i = 0; i < 3; i++) acc ^= djb2(w[i]);
    printf("fixture hash acc=%u\n", acc);
    return 0;
}
