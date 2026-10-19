#include <cstdio>
#include <string>

// std::string building, appending, substring search.
int main() {
    const char *parts[] = {"glyph", "-", "kern", ":", "hint", "/", "glyf"};
    std::string s;
    unsigned acc = 0;
    for (int rep = 0; rep < 24; rep++) {
        s.clear();
        for (int i = 0; i < 6; i++) {
            s += parts[i];
            s += std::to_string(rep * 6 + i);
        }
        acc += (unsigned)s.size();
        size_t hit = s.find("kern");
        acc ^= (unsigned)hit;
        std::string sub = s.substr(2, 9);
        acc += (unsigned)sub[0];
    }
    std::printf("strops acc=%u len=%zu\n", acc, s.size());
    return 0;
}
