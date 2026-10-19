#include <stdio.h>
#include <string.h>

/* Hand-rolled strlen/strcpy/strcmp next to libc calls on the same data. */
static size_t my_strlen(const char *s) {
    const char *p = s;
    while (*p) p++;
    return (size_t)(p - s);
}

static char *my_strcpy(char *dst, const char *src) {
    char *d = dst;
    while ((*d++ = *src++)) { }
    return dst;
}

static int my_strcmp(const char *a, const char *b) {
    while (*a && *a == *b) { a++; b++; }
    return (unsigned char)*a - (unsigned char)*b;
}

int main(void) {
    const char *sample = "the quick brown fox jumps over the lazy dog";
    char buf[128], buf2[128];
    unsigned acc = 0;
    for (int i = 0; i < 40; i++) {
        my_strcpy(buf, sample);
        buf[my_strlen(sample)] = '\0';
        snprintf(buf2, sizeof buf2, "%.*s#%d", i + 1, sample, i);
        acc += (unsigned)my_strlen(buf2);
        acc ^= (unsigned)strlen(buf);
        acc += (unsigned)(my_strcmp(buf, sample) == 0);
        acc += (unsigned)(strcmp(buf, sample) == 0);
    }
    printf("strops acc=%u len=%zu\n", acc, strlen(buf2));
    return 0;
}
