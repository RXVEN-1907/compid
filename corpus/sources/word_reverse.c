#include <stdio.h>
#include <string.h>

/* Reverse the characters of each word in place, preserving order. */
static void reverse_range(char *s, size_t lo, size_t hi) {
    while (lo < hi) {
        char t = s[lo];
        s[lo++] = s[hi];
        s[hi--] = t;
    }
}

static void reverse_words(char *s) {
    size_t n = strlen(s), i = 0;
    while (i < n) {
        while (i < n && s[i] == ' ') i++;
        size_t start = i;
        while (i < n && s[i] != ' ') i++;
        reverse_range(s, start, i - 1);
    }
}

int main(void) {
    char buf[512];
    const char *seeds[] = {"one two three", "compact spaces  here",
                           "a", "palindrome never odd or even", "x y z"};
    unsigned acc = 0;
    for (int r = 0; r < 12; r++) {
        snprintf(buf, sizeof buf, "%s %d %s", seeds[r % 5], r, seeds[(r + 2) % 5]);
        reverse_words(buf);
        for (const char *p = buf; *p; p++) acc = acc * 33u + (unsigned char)*p;
    }
    printf("wordrev acc=%u\n", acc);
    return 0;
}
