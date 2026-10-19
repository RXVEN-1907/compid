#include <cstdio>

// Templates: recursive instantiation and constexpr evaluation, then a
// runtime loop that consumes the generated tables.
template <unsigned N>
struct fact {
    static const unsigned long value = N * fact<N - 1>::value;
};
template <>
struct fact<0> {
    static const unsigned long value = 1;
};

template <unsigned N>
struct fib {
    static const unsigned value = fib<N - 1>::value + fib<N - 2>::value;
};
template <>
struct fib<0> { static const unsigned value = 0; };
template <>
struct fib<1> { static const unsigned value = 1; };

template <typename T, unsigned N>
struct buffer {
    T data[N];
    T &operator[](unsigned i) { return data[i % N]; }
    unsigned size() const { return N; }
};

int main() {
    unsigned long f = fact<12>::value + fact<10>::value;
    unsigned fi = fib<20>::value + fib<17>::value;
    buffer<unsigned, 256> buf;
    unsigned s = 1357;
    for (unsigned i = 0; i < 4096; i++) {
        s = s * 1664525u + 1013904223u;
        buf[i] = s ^ (i * 31u);
    }
    unsigned acc = 0;
    for (unsigned i = 0; i < buf.size(); i++) acc = acc * 3u + buf[i];
    std::printf("tmpl f=%lu fi=%u acc=%u\n", f, fi, acc);
    return 0;
}
