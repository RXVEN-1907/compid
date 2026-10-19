#include <algorithm>
#include <cstdio>
#include <numeric>
#include <vector>

// STL algorithms with lambdas: transform, accumulate, count_if, partitions.
int main() {
    std::vector<int> v(2048);
    unsigned s = 60606;
    for (auto &x : v) {
        s = s * 22695477u + 1u;
        x = (int)(s % 2000) - 1000;
    }
    std::transform(v.begin(), v.end(), v.begin(), [](int x) {
        return (x < 0) ? -x : x;
    });
    long sum = std::accumulate(v.begin(), v.end(), 0L);
    long big = std::count_if(v.begin(), v.end(), [](int x) { return x > 900; });
    auto mm = std::minmax_element(v.begin(), v.end());
    long uniq = 0;
    {
        std::vector<int> w = v;
        std::sort(w.begin(), w.end());
        uniq = std::unique(w.begin(), w.end()) - w.begin();
    }
    std::printf("lambda sum=%ld big=%ld uniq=%ld range=%d\n",
                sum, big, uniq, *mm.second - *mm.first);
    return 0;
}
