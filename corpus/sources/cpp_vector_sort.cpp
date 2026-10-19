#include <algorithm>
#include <cstdio>
#include <vector>

// std::vector plus std::sort: template instantiation over a comparator.
struct point { int x, y; };

int main() {
    std::vector<int> a;
    a.reserve(1024);
    unsigned s = 24680;
    for (int i = 0; i < 1024; i++) {
        s = s * 1103515245u + 12345u;
        a.push_back((int)(s >> 16) % 999);
    }
    std::sort(a.begin(), a.end());
    std::vector<point> pts;
    for (int i = 0; i < 256; i++)
        pts.push_back({a[i], a[(i * 7 + 3) % 1024]});
    std::sort(pts.begin(), pts.end(), [](const point &p, const point &q) {
        return p.x * p.x + p.y * p.y < q.x * q.x + q.y * q.y;
    });
    long acc = 0;
    for (int i = 0; i < 1024; i += 11) acc += a[i];
    for (const auto &p : pts) acc += p.x - p.y;
    std::printf("vecsort acc=%ld n=%zu\n", acc, pts.size());
    return 0;
}
