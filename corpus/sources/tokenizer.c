#include <stdio.h>
#include <string.h>

/* Manual tokenizer: split on delimiters, no strtok, count token classes. */
static int classify_token(const char *tok, size_t len) {
    int digits = 0, alpha = 0, other = 0;
    for (size_t i = 0; i < len; i++) {
        char c = tok[i];
        if (c >= '0' && c <= '9') digits++;
        else if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')) alpha++;
        else other++;
    }
    if (digits && !alpha && !other) return 1;
    if (alpha && !digits && !other) return 2;
    if (other) return 3;
    return 0;
}

int main(void) {
    const char *line = "id=42,  name:seven  ;code  0x1F,  mixed7x;tail";
    unsigned counts[4] = {0, 0, 0, 0};
    unsigned acc = 0;
    for (int pass = 0; pass < 6; pass++) {
        char buf[128];
        snprintf(buf, sizeof buf, "%s|pass%d", line, pass);
        const char *p = buf;
        while (*p) {
            while (*p && (*p == ' ' || *p == ',' || *p == ';' ||
                          *p == ':' || *p == '|' || *p == '=')) p++;
            const char *start = p;
            while (*p && !(*p == ' ' || *p == ',' || *p == ';' ||
                           *p == ':' || *p == '|' || *p == '=')) p++;
            size_t len = (size_t)(p - start);
            if (len) counts[classify_token(start, len)]++;
        }
    }
    for (int i = 0; i < 4; i++) acc = acc * 10u + counts[i];
    printf("tokens acc=%u classes=%u,%u,%u,%u\n", acc,
           counts[0], counts[1], counts[2], counts[3]);
    return 0;
}
