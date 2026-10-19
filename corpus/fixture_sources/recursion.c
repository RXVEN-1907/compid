#include <stdio.h>
static unsigned long fib(unsigned n) {
    return n < 2 ? n : fib(n - 1) + fib(n - 2);
}
int main(void) {
    printf("fixture recursion fib=%lu\n", fib(18));
    return 0;
}
