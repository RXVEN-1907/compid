#include <stdio.h>

/* RPN expression evaluator with an explicit operand stack. */
static long stack[64];
static int sp;

static void push(long v) { if (sp < 64) stack[sp++] = v; }
static long pop(void) { return sp > 0 ? stack[--sp] : 0; }

static void eval_token(const char *tok, size_t len) {
    if (len == 1 && (*tok == '+' || *tok == '-' || *tok == '*' || *tok == '/' ||
                     *tok == '%')) {
        long b = pop(), a = pop();
        switch (*tok) {
            case '+': push(a + b); break;
            case '-': push(a - b); break;
            case '*': push(a * b); break;
            case '/': push(b ? a / b : 0); break;
            case '%': push(b ? a % b : 0); break;
        }
    } else {
        long v = 0;
        int neg = 0;
        for (size_t i = 0; i < len; i++) {
            if (tok[i] == '_') neg = 1;
            else if (tok[i] >= '0' && tok[i] <= '9') v = v * 10 + (tok[i] - '0');
        }
        push(neg ? -v : v);
    }
}

int main(void) {
    const char *expr = "5_8+13_2*+21-4/ 100 7% + 3 4 5 + * +";
    unsigned acc = 0;
    for (int rep = 0; rep < 8; rep++) {
        sp = 0;
        const char *p = expr;
        while (*p) {
            while (*p == ' ') p++;
            const char *start = p;
            while (*p && *p != ' ') p++;
            if (p > start) eval_token(start, (size_t)(p - start));
        }
        while (sp) acc ^= (unsigned)pop() + (unsigned)rep;
    }
    printf("stackvm acc=%u\n", acc);
    return 0;
}
