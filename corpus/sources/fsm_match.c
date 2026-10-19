#include <stdio.h>
#include <string.h>

/* Tiny wildcard matcher compiled to a transition table: patterns of
   literals, '?' (any one char) and '*' (any run). */
enum { FAIL = -1 };
static int trans[32][3];  /* inputs: literal-in-pattern, '?', '*' handled apart */

static int build(const char *pat, int (*tab)[3], char *lits) {
    int n = 0;
    for (const char *p = pat; *p; p++, n++) {
        lits[n] = *p;
        for (int c = 0; c < 3; c++) tab[n][c] = FAIL;
    }
    for (int i = 0; i < n; i++) {
        if (lits[i] == '*') { tab[i][0] = i + 1; tab[i][1] = i + 1; }
        else if (lits[i] == '?') { tab[i][0] = i + 1; tab[i][1] = i + 1; }
        else { tab[i][0] = i + 1; tab[i][1] = FAIL; }
    }
    return n;
}

static int match(const char *s, const char *pat) {
    char lits[32];
    int (*tab)[3] = trans;
    int n = build(pat, tab, lits);
    int state = 0;
    for (const char *p = s; *p; p++) {
        if (state == n) return 0;
        int c = (*p == lits[state]) ? 0 : 1;
        int ns = tab[state][c];
        if (lits[state] == '*') {
            /* '*' may swallow the character and stay, or advance */
            ns = (tab[state][0] == FAIL) ? state : tab[state][0];
            if (ns < n && lits[ns] == (char)*p) { /* try to advance further */ }
        }
        if (ns == FAIL) return 0;
        state = ns;
    }
    return state == n || (state < n && lits[state] == '*');
}

int main(void) {
    const char *texts[] = {"hello.txt", "hello.c", "a/b/c", "", "aaaa",
                           "x123y", "notes", "a"};
    const char *pats[] = {"*.txt", "hello.?", "a*b*c", "*", "a?a?a",
                          "x?*y", "n?tes", "a*"};
    unsigned acc = 0;
    for (unsigned i = 0; i < 8; i++)
        for (unsigned j = 0; j < 8; j++)
            acc = acc * 2u + (unsigned)match(texts[i], pats[j]);
    printf("fsmatch acc=%u\n", acc);
    return 0;
}
