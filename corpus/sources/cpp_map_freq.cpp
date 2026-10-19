#include <cstdio>
#include <map>
#include <string>
#include <unordered_map>

// Associative containers: std::map (red-black tree) and unordered_map
// (hash table) over the same key stream.
int main() {
    std::map<int, int> ordered;
    std::unordered_map<std::string, unsigned> hashed;
    const char *words[] = {"foo", "bar", "baz", "quux", "wibble", "wobble"};
    unsigned s = 8080;
    for (int i = 0; i < 4000; i++) {
        s = s * 1103515245u + 12345u;
        int k = (int)(s % 512);
        ordered[k]++;
        hashed[words[s % 6]] += (s >> 8) & 1;
    }
    long acc = 0;
    for (const auto &kv : ordered) acc += (long)kv.first * kv.second;
    unsigned h = 0;
    for (const auto &kv : hashed) h = h * 33u + kv.second;
    std::printf("mapfreq acc=%ld h=%u maps=%zu\n", acc, h, ordered.size());
    return 0;
}
