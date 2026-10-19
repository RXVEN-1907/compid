#include <cstdio>
#include <stdexcept>
#include <vector>

// Exception handling: throw paths, stack unwinding, catch dispatch.
static int depth_trigger = 0;

static int dive(int d) {
    if (d == depth_trigger) throw std::out_of_range("bottom");
    std::vector<int> frame(d + 1, d);
    int sub = 0;
    try {
        sub = dive(d + 1);
    } catch (const std::out_of_range &) {
        sub = -d;
    }
    return sub + (int)frame.size();
}

int main() {
    long acc = 0;
    for (int trigger = 0; trigger < 24; trigger++) {
        depth_trigger = trigger;
        try {
            acc += dive(0);
        } catch (const std::exception &) {
            acc += 1;
        }
    }
    depth_trigger = 48;
    try {
        (void)dive(0);  /* runs to depth 48, throws, unwinds all frames */
    } catch (const std::exception &e) {
        for (const char *p = e.what(); *p; p++) acc += *p;
    }
    std::printf("except acc=%ld\n", acc);
    return 0;
}
