#include <stdio.h>
#include <string.h>
int main(void) {
    char buf[64] = "fixture string";
    size_t n = strlen(buf);
    for (size_t i = 0; i < n / 2; i++) {
        char t = buf[i];
        buf[i] = buf[n - 1 - i];
        buf[n - 1 - i] = t;
    }
    printf("%s\n", buf);
    return 0;
}
