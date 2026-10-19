#include <stdio.h>

/* Manual string-to-integer parsing with sign, whitespace and overflow
   clamping, applied to a table of tricky inputs. */
static const char *inputs[] = {
    "0", "42", "-42", "  +123abc", "2147483647", "-2147483648",
    "2147483648", "  -9999999999", "000123", "+", "-", "",
};

static long parse_int(const char *s, const char **end) {
    while (*s == ' ' || *s == '\t') s++;
    int neg = 0;
    if (*s == '+' || *s == '-') neg = (*s++ == '-');
    long acc = 0;
    int any = 0;
    while (*s >= '0' && *s <= '9') {
        any = 1;
        if (acc > (2147483647L - (*s - '0')) / 10L) {
            acc = neg ? (-2147483647L - 1L) : 2147483647L;
            while (*s >= '0' && *s <= '9') s++;
            break;
        }
        acc = acc * 10 + (*s++ - '0');
    }
    if (!any) acc = 0;
    *end = s;
    return neg ? -acc : acc;
}

int main(void) {
    long acc = 0;
    for (unsigned i = 0; i < sizeof inputs / sizeof inputs[0]; i++) {
        const char *end;
        long v = parse_int(inputs[i], &end);
        acc += v + (long)(end - inputs[i]);
    }
    printf("atoi acc=%ld\n", acc);
    return 0;
}
