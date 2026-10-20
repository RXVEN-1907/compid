#!/usr/bin/env python3
"""Write the deterministic C/C++ source corpus into corpus/sources/.

Every program is self-contained, deterministic (own PRNGs with fixed seeds,
no time() or rand()), uses nothing beyond libc/libm/libstdc++, and prints a
small checksum of its work so nothing can be optimized into thin air. The
programs deliberately span a wide range of idioms -- loops, string handling,
sorting, hashing, state machines, recursion, printf-heavy code, array math,
bit twiddling, floating point, linked structures, function pointers, varargs
and switch tables -- because build fingerprints are only as good as the
variety of code they were measured on.

The set of programs is fixed: rerunning this script overwrites sources/ with
byte-identical files. To add a program, add a (name, code) pair to SOURCES
or CXX_SOURCES and rerun; new builds pick it up automatically.

Usage:
    python3 corpus/generate_sources.py [--out corpus/sources] [--force]
"""

import argparse
import sys
from pathlib import Path


C_SOURCES = [
# ---------------------------------------------------------------------------
# Startup and printf-family idioms
# ---------------------------------------------------------------------------
("hello", r"""
#include <stdio.h>

/* Smallest possible program: a single call the optimizer may lower to puts. */
int main(void) {
    printf("compid hello corpus\n");
    return 0;
}
"""),

("printf_menu", r"""
#include <stdio.h>

/* printf-heavy: many distinct format specifiers in one hot function. */
static const char *items[] = {
    "encoder", "decoder", "mux", "demux", "filter", "sampler",
};

int main(void) {
    for (int i = 0; i < 6; i++) {
        printf("[%2d] %-8s rev=%u key=%#x gain=%.3g load=%g%%\n",
               i, items[i], 100u + (unsigned)i * 7u,
               0x5a00u | (unsigned)(i * 33), 0.25 * (i + 1) / 3.0,
               12.5 * i);
    }
    printf("done %d items\n", 6);
    return 0;
}
"""),

("snprintf_report", r"""
#include <stdio.h>
#include <string.h>

/* Builds a report with repeated snprintf calls and manual truncation. */
static void stat_line(char *dst, size_t cap, const char *name,
                      double mean, double var, int n) {
    int len = snprintf(dst, cap, "%-10s mean=%9.4f var=%9.4f n=%d",
                       name, mean, var, n);
    if (len < 0) dst[0] = '\0';
    if ((size_t)len >= cap) dst[cap - 1] = '\0';
}

int main(void) {
    char buf[12][96];
    double x[64];
    for (int i = 0; i < 64; i++) x[i] = (i * 37) % 101 * 0.125 - 4.0;
    for (int r = 0; r < 12; r++) {
        double mean = 0.0, var = 0.0;
        for (int i = 0; i < 64; i++) mean += x[(i + r * 5) % 64];
        mean /= 64.0;
        for (int i = 0; i < 64; i++) {
            double d = x[(i + r * 5) % 64] - mean;
            var += d * d;
        }
        stat_line(buf[r], sizeof buf[0] - 4, "series", mean, var / 63.0, 64);
        strcat(buf[r], "|ok");
    }
    unsigned long acc = 0;
    for (int r = 0; r < 12; r++)
        for (const char *p = buf[r]; *p; p++) acc = acc * 31u + (unsigned char)*p;
    printf("report acc=%lu len0=%zu\n", acc, strlen(buf[0]));
    return 0;
}
"""),

# ---------------------------------------------------------------------------
# Pseudo-random number generators (fixed seeds, no libc rand)
# ---------------------------------------------------------------------------
("lcg_rng", r"""
#include <stdio.h>

/* Classic 32-bit linear congruential generator, parameters from Numerical
   Recipes. Deterministic: the seed is a constant. */
static unsigned long lcg_state = 20240915u;

static unsigned lcg_next(void) {
    lcg_state = lcg_state * 6364136223846793005u >> 0; /* widened below */
    lcg_state = (unsigned long)(unsigned)lcg_state;
    lcg_state = lcg_state * 1664525u + 1013904223u;
    return (unsigned)(lcg_state >> 16);
}

int main(void) {
    unsigned acc = 0;
    int hist[16] = {0};
    for (int i = 0; i < 20000; i++) {
        unsigned v = lcg_next();
        acc ^= v + i;
        hist[v & 15]++;
    }
    for (int i = 0; i < 16; i++) acc += (unsigned)hist[i] * (unsigned)(i + 1);
    printf("lcg acc=%u\n", acc);
    return 0;
}
"""),

("xorshift64", r"""
#include <stdio.h>
#include <stdint.h>

/* xorshift64* (Vigna). Exercises 64-bit multiplies and shifts. */
static uint64_t xs = 0x9E3779B97F4A7C15ull;

static uint64_t next(void) {
    xs ^= xs >> 12;
    xs ^= xs << 25;
    xs ^= xs >> 27;
    return xs * 0x2545F4914F6CDD1Dull;
}

int main(void) {
    uint64_t acc = 0;
    for (int i = 0; i < 50000; i++) {
        uint64_t v = next();
        acc += (v >> 33) ^ (v & 0xff);
        if ((v & 7) == 0) acc = (acc << 1) | (acc >> 63);
    }
    printf("xorshift64 acc=%llu\n", (unsigned long long)acc);
    return 0;
}
"""),

# ---------------------------------------------------------------------------
# Hash functions
# ---------------------------------------------------------------------------
("fnv1a_hash", r"""
#include <stdio.h>
#include <stdint.h>
#include <string.h>

/* FNV-1a 64-bit over a table of strings. */
static const char *words[] = {
    "aurora", "boreal", "cirrus", "nimbus", "zephyr", "sirocco",
    "mistral", "chinook", "harmattan", "tramontana",
};

static uint64_t fnv1a(const char *s) {
    uint64_t h = 0xCBF29CE484222325ull;
    for (; *s; s++) {
        h ^= (unsigned char)*s;
        h *= 0x100000001B3ull;
    }
    return h;
}

int main(void) {
    uint64_t acc = 0;
    char tmp[64];
    for (int i = 0; i < 10; i++) {
        acc ^= fnv1a(words[i]);
        for (int j = 0; j < 10; j++) {
            snprintf(tmp, sizeof tmp, "%s/%d", words[j], i * j);
            acc += fnv1a(tmp) >> 13;
        }
    }
    printf("fnv1a acc=%llu\n", (unsigned long long)acc);
    return 0;
}
"""),

("djb2_hash", r"""
#include <stdio.h>
#include <stdint.h>

/* djb2 with a small open-addressing table and linear probing. */
#define SLOTS 128

static const char *keys[] = {
    "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf",
    "hotel", "india", "juliet", "kilo", "lima", "mike", "november",
};

static uint32_t djb2(const char *s) {
    uint32_t h = 5381u;
    while (*s) h = h * 33u ^ (unsigned char)*s++;
    return h;
}

int main(void) {
    uint32_t table[SLOTS];
    int used[SLOTS];
    for (int i = 0; i < SLOTS; i++) { table[i] = 0; used[i] = 0; }
    int collisions = 0;
    for (unsigned round = 0; round < 4; round++) {
        for (unsigned k = 0; k < sizeof keys / sizeof keys[0]; k++) {
            uint32_t h = djb2(keys[k]) ^ (round * 2654435761u);
            uint32_t slot = h & (SLOTS - 1);
            while (used[slot] && table[slot] != h) {
                slot = (slot + 1) & (SLOTS - 1);
                collisions++;
            }
            used[slot] = 1;
            table[slot] = h;
        }
    }
    uint32_t acc = 0;
    for (int i = 0; i < SLOTS; i++) acc = acc * 3u + table[i] + (uint32_t)used[i];
    printf("djb2 acc=%u collisions=%d\n", acc, collisions);
    return 0;
}
"""),

("crc32_bitwise", r"""
#include <stdio.h>
#include <stdint.h>
#include <string.h>

/* Bitwise CRC-32 (IEEE), no table: one branch per bit. */
static uint32_t crc32_bits(const unsigned char *buf, size_t len) {
    uint32_t crc = 0xFFFFFFFFu;
    for (size_t i = 0; i < len; i++) {
        crc ^= buf[i];
        for (int b = 0; b < 8; b++) {
            uint32_t mask = -(crc & 1u);
            crc = (crc >> 1) ^ (0xEDB88320u & mask);
        }
    }
    return ~crc;
}

int main(void) {
    unsigned char msg[256];
    for (int i = 0; i < 256; i++) msg[i] = (unsigned char)(i * 7 + 3);
    uint32_t acc = 0;
    for (size_t len = 1; len <= sizeof msg; len += 7)
        acc = acc * 31u + crc32_bits(msg, len);
    printf("crc32-bit acc=%u\n", acc);
    return 0;
}
"""),

("crc32_table", r"""
#include <stdio.h>
#include <stdint.h>

/* Table-driven CRC-32: the 256-entry table is generated at startup. */
static uint32_t table[256];

static void crc_init(void) {
    for (uint32_t i = 0; i < 256; i++) {
        uint32_t c = i;
        for (int k = 0; k < 8; k++)
            c = (c & 1u) ? 0xEDB88320u ^ (c >> 1) : c >> 1;
        table[i] = c;
    }
}

static uint32_t crc32(const unsigned char *buf, unsigned len) {
    uint32_t crc = 0xFFFFFFFFu;
    for (unsigned i = 0; i < len; i++)
        crc = table[(crc ^ buf[i]) & 0xFFu] ^ (crc >> 8);
    return ~crc;
}

int main(void) {
    crc_init();
    unsigned char data[1024];
    for (unsigned i = 0; i < 1024; i++) data[i] = (unsigned char)(i ^ (i >> 3));
    uint32_t acc = 0;
    for (unsigned off = 0; off < 512; off += 9)
        acc ^= crc32(data + off, 512);
    printf("crc32-tbl acc=%u t0=%u t255=%u\n", acc, table[0], table[255]);
    return 0;
}
"""),

("murmur3_mix", r"""
#include <stdio.h>
#include <stdint.h>

/* MurmurHash3's finalizer plus integer-block hashing. */
static inline uint32_t rotl32(uint32_t x, int r) {
    return (x << r) | (x >> (32 - r));
}

static uint32_t fmix32(uint32_t h) {
    h ^= h >> 16;
    h *= 0x85EBCA6Bu;
    h ^= h >> 13;
    h *= 0xC2B2AE35u;
    h ^= h >> 16;
    return h;
}

static uint32_t murmur3_int(uint32_t k) {
    k *= 0xCC9E2D51u;
    k = rotl32(k, 15);
    k *= 0x1B873593u;
    return fmix32(k);
}

int main(void) {
    uint32_t acc = 0;
    for (uint32_t i = 0; i < 65536; i++) {
        uint32_t h = murmur3_int(i ^ 0xDEADBEEFu);
        acc += rotl32(h, (int)(h & 31)) ^ i;
    }
    printf("murmur3 acc=%u\n", acc);
    return 0;
}
"""),

# ---------------------------------------------------------------------------
# Sorting
# ---------------------------------------------------------------------------
("bubble_sort", r"""
#include <stdio.h>

/* Bubble sort with the early-exit flag. */
static void bubble_sort(int *a, int n) {
    for (int pass = 0; pass < n - 1; pass++) {
        int swapped = 0;
        for (int i = 0; i < n - 1 - pass; i++) {
            if (a[i] > a[i + 1]) {
                int t = a[i];
                a[i] = a[i + 1];
                a[i + 1] = t;
                swapped = 1;
            }
        }
        if (!swapped) break;
    }
}

int main(void) {
    int a[256];
    unsigned s = 12345;
    for (int i = 0; i < 256; i++) {
        s = s * 1103515245u + 12345u;
        a[i] = (int)(s >> 17) % 1000;
    }
    bubble_sort(a, 256);
    unsigned acc = 0;
    for (int i = 0; i < 256; i++) acc = acc * 131u + (unsigned)a[i];
    printf("bubble acc=%u a0=%d a255=%d\n", acc, a[0], a[255]);
    return 0;
}
"""),

("insertion_sort", r"""
#include <stdio.h>

/* Insertion sort: short inner loop, branch-predictor friendly. */
static void insertion_sort(int *a, int n) {
    for (int i = 1; i < n; i++) {
        int key = a[i];
        int j = i - 1;
        while (j >= 0 && a[j] > key) {
            a[j + 1] = a[j];
            j--;
        }
        a[j + 1] = key;
    }
}

int main(void) {
    int a[512];
    unsigned s = 777;
    for (int i = 0; i < 512; i++) {
        s = s * 48271u % 2147483647u;
        a[i] = (int)(s % 4096);
    }
    insertion_sort(a, 512);
    long acc = 0;
    for (int i = 0; i < 512; i++) acc += (long)a[i] * (i + 1);
    printf("insertion acc=%ld med=%d\n", acc, a[256]);
    return 0;
}
"""),

("quicksort_rec", r"""
#include <stdio.h>

/* Recursive quicksort with median-of-three pivot selection. */
static void swap(int *a, int *b) { int t = *a; *a = *b; *b = t; }

static void quicksort(int *a, int lo, int hi) {
    while (lo < hi) {
        int mid = lo + (hi - lo) / 2;
        if (a[mid] < a[lo]) swap(&a[mid], &a[lo]);
        if (a[hi] < a[lo]) swap(&a[hi], &a[lo]);
        if (a[hi] < a[mid]) swap(&a[hi], &a[mid]);
        int pivot = a[mid];
        int i = lo, j = hi;
        while (i <= j) {
            while (a[i] < pivot) i++;
            while (a[j] > pivot) j--;
            if (i <= j) { swap(&a[i], &a[j]); i++; j--; }
        }
        if (j - lo < hi - i) { quicksort(a, lo, j); lo = i; }
        else { quicksort(a, i, hi); hi = j; }
    }
}

int main(void) {
    static int a[2048];
    unsigned s = 99173;
    for (int i = 0; i < 2048; i++) {
        s ^= s << 13; s ^= s >> 17; s ^= s << 5;
        a[i] = (int)(s % 100000);
    }
    quicksort(a, 0, 2047);
    unsigned acc = 0;
    for (int i = 0; i < 2048; i += 7) acc += (unsigned)a[i];
    printf("quick acc=%u min=%d max=%d\n", acc, a[0], a[2047]);
    return 0;
}
"""),

("merge_sort", r"""
#include <stdio.h>

/* Recursive top-down mergesort with a shared scratch buffer. */
static int tmp[1024];

static void merge(int *a, int lo, int mid, int hi) {
    int i = lo, j = mid, k = lo;
    while (i < mid && j < hi)
        tmp[k++] = (a[i] <= a[j]) ? a[i++] : a[j++];
    while (i < mid) tmp[k++] = a[i++];
    while (j < hi) tmp[k++] = a[j++];
    for (i = lo; i < hi; i++) a[i] = tmp[i];
}

static void merge_sort(int *a, int lo, int hi) {
    if (hi - lo < 2) return;
    int mid = lo + (hi - lo) / 2;
    merge_sort(a, lo, mid);
    merge_sort(a, mid, hi);
    merge(a, lo, mid, hi);
}

int main(void) {
    int a[1024];
    unsigned s = 20260101;
    for (int i = 0; i < 1024; i++) {
        s = s * 69069u + 1u;
        a[i] = (int)((s >> 8) % 2048) - 1024;
    }
    merge_sort(a, 0, 1024);
    long acc = 0;
    for (int i = 0; i < 1024; i++) acc += (long)a[i] * i;
    printf("merge acc=%ld inversions_check=%d\n", acc, a[0] <= a[1023]);
    return 0;
}
"""),

("heap_sort", r"""
#include <stdio.h>

/* In-place heapsort with iterative sift-down. */
static void sift_down(int *a, int start, int end) {
    int root = start;
    for (;;) {
        int child = 2 * root + 1;
        if (child > end) break;
        if (child + 1 <= end && a[child] < a[child + 1]) child++;
        if (a[root] < a[child]) {
            int t = a[root];
            a[root] = a[child];
            a[child] = t;
            root = child;
        } else break;
    }
}

static void heap_sort(int *a, int n) {
    for (int start = n / 2 - 1; start >= 0; start--) sift_down(a, start, n - 1);
    for (int end = n - 1; end > 0; end--) {
        int t = a[0];
        a[0] = a[end];
        a[end] = t;
        sift_down(a, 0, end - 1);
    }
}

int main(void) {
    int a[1024];
    unsigned s = 5150;
    for (int i = 0; i < 1024; i++) {
        s = s * 1664525u + 1013904223u;
        a[i] = (int)(s >> 20);
    }
    heap_sort(a, 1024);
    unsigned acc = 0;
    for (int i = 0; i < 1024; i++) acc ^= (unsigned)a[i] << (i & 7);
    printf("heap acc=%u top=%d\n", acc, a[0]);
    return 0;
}
"""),

("qsort_libc", r"""
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* libc qsort over a struct array with a real comparator: exercises the
   indirect calls qsort makes back into this binary. */
typedef struct {
    int key;
    char tag[8];
} rec;

static int cmp_rec(const void *pa, const void *pb) {
    const rec *a = pa, *b = pb;
    if (a->key != b->key) return (a->key < b->key) ? -1 : 1;
    return strcmp(a->tag, b->tag);
}

int main(void) {
    rec recs[512];
    unsigned s = 31337;
    for (int i = 0; i < 512; i++) {
        s = s * 1103515245u + 12345u;
        recs[i].key = (int)(s % 100) - 50;
        snprintf(recs[i].tag, sizeof recs[i].tag, "r%03d", i);
    }
    qsort(recs, 512, sizeof recs[0], cmp_rec);
    int sum = 0;
    for (int i = 0; i < 512; i += 13) sum += recs[i].key;
    printf("qsort sum=%d first=%s last=%s\n", sum, recs[0].tag, recs[511].tag);
    return 0;
}
"""),

# ---------------------------------------------------------------------------
# Searching
# ---------------------------------------------------------------------------
("binary_search", r"""
#include <stdio.h>

/* Lower-bound binary search plus membership checks. */
static int lower_bound(const int *a, int n, int key) {
    int lo = 0, hi = n;
    while (lo < hi) {
        int mid = lo + (hi - lo) / 2;
        if (a[mid] < key) lo = mid + 1;
        else hi = mid;
    }
    return lo;
}

int main(void) {
    int a[1024];
    for (int i = 0; i < 1024; i++) a[i] = i * 3 + 1;
    long acc = 0;
    for (int q = -5; q < 3100; q += 11) {
        int idx = lower_bound(a, 1024, q);
        acc += (idx < 1024 && a[idx] == q) ? idx : -idx;
    }
    for (int q = 0; q < 1024; q++) acc += lower_bound(a, 1024, a[q]) == q;
    printf("binsearch acc=%ld\n", acc);
    return 0;
}
"""),

("linear_scan", r"""
#include <stdio.h>

/* Linear scans: min/max in one pass, a sentinel search, and a running
   branchless-style update (compare-and-select). */
static void scan(const int *a, int n, int *out_min, int *out_max, int needle) {
    int mn = a[0], mx = a[0], found = -1;
    for (int i = 1; i < n; i++) {
        int v = a[i];
        mn = (v < mn) ? v : mn;
        mx = (v > mx) ? v : mx;
        if (found < 0 && v == needle) found = i;
    }
    *out_min = mn;
    *out_max = mx;
    (void)found;
}

int main(void) {
    int a[4096];
    unsigned s = 42;
    for (int i = 0; i < 4096; i++) {
        s = s * 6364136223846793005u + 1442695040888963407u;
        a[i] = (int)((s >> 33) % 100000);
    }
    int mn, mx;
    long acc = 0;
    for (int rep = 0; rep < 8; rep++) {
        scan(a, 4096, &mn, &mx, 4242 + rep);
        acc += mn + mx;
    }
    printf("linear acc=%ld min=%d max=%d\n", acc, mn, mx);
    return 0;
}
"""),

# ---------------------------------------------------------------------------
# String handling
# ---------------------------------------------------------------------------
("string_ops", r"""
#include <stdio.h>
#include <string.h>

/* Hand-rolled strlen/strcpy/strcmp next to libc calls on the same data. */
static size_t my_strlen(const char *s) {
    const char *p = s;
    while (*p) p++;
    return (size_t)(p - s);
}

static char *my_strcpy(char *dst, const char *src) {
    char *d = dst;
    while ((*d++ = *src++)) { }
    return dst;
}

static int my_strcmp(const char *a, const char *b) {
    while (*a && *a == *b) { a++; b++; }
    return (unsigned char)*a - (unsigned char)*b;
}

int main(void) {
    const char *sample = "the quick brown fox jumps over the lazy dog";
    char buf[128], buf2[128];
    unsigned acc = 0;
    for (int i = 0; i < 40; i++) {
        my_strcpy(buf, sample);
        buf[my_strlen(sample)] = '\0';
        snprintf(buf2, sizeof buf2, "%.*s#%d", i + 1, sample, i);
        acc += (unsigned)my_strlen(buf2);
        acc ^= (unsigned)strlen(buf);
        acc += (unsigned)(my_strcmp(buf, sample) == 0);
        acc += (unsigned)(strcmp(buf, sample) == 0);
    }
    printf("strops acc=%u len=%zu\n", acc, strlen(buf2));
    return 0;
}
"""),

("word_reverse", r"""
#include <stdio.h>
#include <string.h>

/* Reverse the characters of each word in place, preserving order. */
static void reverse_range(char *s, size_t lo, size_t hi) {
    while (lo < hi) {
        char t = s[lo];
        s[lo++] = s[hi];
        s[hi--] = t;
    }
}

static void reverse_words(char *s) {
    size_t n = strlen(s), i = 0;
    while (i < n) {
        while (i < n && s[i] == ' ') i++;
        size_t start = i;
        while (i < n && s[i] != ' ') i++;
        reverse_range(s, start, i - 1);
    }
}

int main(void) {
    char buf[512];
    const char *seeds[] = {"one two three", "compact spaces  here",
                           "a", "palindrome never odd or even", "x y z"};
    unsigned acc = 0;
    for (int r = 0; r < 12; r++) {
        snprintf(buf, sizeof buf, "%s %d %s", seeds[r % 5], r, seeds[(r + 2) % 5]);
        reverse_words(buf);
        for (const char *p = buf; *p; p++) acc = acc * 33u + (unsigned char)*p;
    }
    printf("wordrev acc=%u\n", acc);
    return 0;
}
"""),

("tokenizer", r"""
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
"""),

("atoi_manual", r"""
#include <stdio.h>

/* Manual string-to-integer parsing with sign, whitespace and overflow
   clamping, applied to a table of tricky inputs. */
static const char *inputs[] = {
    "0", "42", "-42", "  +123abc", "2147483647", "-2147483648",
    "2147483648", "  -9999999999", "000123", "+", "-", "",
};

static long parse_int(const char *s, const char **end) {
    while (*s == ' ' || *s == '\t') s++;
    int neg = 0;
    if (*s == '+' || *s == '-') neg = (*s++ == '-');
    long acc = 0;
    int any = 0;
    while (*s >= '0' && *s <= '9') {
        any = 1;
        if (acc > (2147483647L - (*s - '0')) / 10L) {
            acc = neg ? (-2147483647L - 1L) : 2147483647L;
            while (*s >= '0' && *s <= '9') s++;
            break;
        }
        acc = acc * 10 + (*s++ - '0');
    }
    if (!any) acc = 0;
    *end = s;
    return neg ? -acc : acc;
}

int main(void) {
    long acc = 0;
    for (unsigned i = 0; i < sizeof inputs / sizeof inputs[0]; i++) {
        const char *end;
        long v = parse_int(inputs[i], &end);
        acc += v + (long)(end - inputs[i]);
    }
    printf("atoi acc=%ld\n", acc);
    return 0;
}
"""),

("itoa_manual", r"""
#include <stdio.h>
#include <string.h>

/* Integer-to-string by hand for bases 10 and 16, into fixed buffers. */
static const char digits_lc[] = "0123456789abcdef";

static size_t utoa_buf(unsigned long v, unsigned base, char *dst, size_t cap) {
    char tmp[72];
    size_t n = 0;
    do {
        tmp[n++] = digits_lc[v % base];
        v /= base;
    } while (v && n < sizeof tmp);
    if (n >= cap) { dst[0] = '\0'; return 0; }
    for (size_t i = 0; i < n; i++) dst[i] = tmp[n - 1 - i];
    dst[n] = '\0';
    return n;
}

int main(void) {
    char a[80], b[80];
    unsigned acc = 0;
    unsigned long vals[] = {0, 1, 15, 16, 255, 4096, 65535, 0xCAFEBABE,
                            0xFFFFFFFFul, 1234567890};
    for (unsigned i = 0; i < sizeof vals / sizeof vals[0]; i++) {
        size_t la = utoa_buf(vals[i], 10, a, sizeof a);
        size_t lb = utoa_buf(vals[i] ^ 0xA5A5A5A5ul, 16, b, sizeof b);
        acc += (unsigned)(la * 100 + lb);
        acc ^= (unsigned)strcmp(a, b);
    }
    printf("itoa acc=%u last=%s\n", acc, b);
    return 0;
}
"""),

# ---------------------------------------------------------------------------
# Array math / vectorizable loops
# ---------------------------------------------------------------------------
("vec_add", r"""
#include <stdio.h>

/* Vectorizable elementwise loops over float arrays. */
#define N 1024

static float x[N], y[N], z[N];

int main(void) {
    unsigned s = 2024;
    for (int i = 0; i < N; i++) {
        s = s * 1664525u + 1013904223u;
        x[i] = (float)((int)(s >> 16) % 256) / 16.0f - 8.0f;
        y[i] = (float)((i * i) % 97) * 0.25f;
    }
    for (int rep = 0; rep < 16; rep++) {
        for (int i = 0; i < N; i++) z[i] = x[i] + 3.0f * y[i];
        for (int i = 0; i < N; i++) x[i] = 0.5f * (z[i] - x[i]);
    }
    double acc = 0;
    for (int i = 0; i < N; i += 3) acc += z[i] - x[i];
    printf("vecadd acc=%.6f\n", acc);
    return 0;
}
"""),

("dot_product", r"""
#include <stdio.h>

/* Reduction loops: a plain dot product and a strided variant. */
#define N 2048

static double a[N], b[N];

static double dot(const double *u, const double *v, int n) {
    double s = 0.0;
    for (int i = 0; i < n; i++) s += u[i] * v[i];
    return s;
}

static double dot_strided(const double *u, const double *v, int n, int stride) {
    double s = 0.0;
    for (int i = 0; i < n; i += stride) s += u[i] * v[i];
    return s;
}

int main(void) {
    for (int i = 0; i < N; i++) {
        a[i] = (i % 64) * 0.015625 - 0.5;
        b[i] = ((i * 7919) % 128) / 128.0;
    }
    double d1 = 0, d2 = 0;
    for (int rep = 0; rep < 4; rep++) {
        d1 += dot(a, b, N);
        d2 += dot_strided(a, b, N, 2 + rep);
    }
    printf("dot d1=%.9f d2=%.9f\n", d1, d2);
    return 0;
}
"""),

("matmul_ijk", r"""
#include <stdio.h>

/* Plain i-j-k matrix multiply, the canonical vectorization target. */
#define D 64

static double A[D][D], B[D][D], C[D][D];

int main(void) {
    for (int i = 0; i < D; i++)
        for (int j = 0; j < D; j++) {
            A[i][j] = ((i * 3 + j) % 7) - 3.0;
            B[i][j] = ((i * 5 + j * 11) % 5) * 0.25;
        }
    for (int i = 0; i < D; i++)
        for (int j = 0; j < D; j++) {
            double s = 0.0;
            for (int k = 0; k < D; k++) s += A[i][k] * B[k][j];
            C[i][j] = s;
        }
    double acc = 0;
    for (int i = 0; i < D; i++)
        for (int j = 0; j < D; j++) acc += C[i][j] * (i - j);
    printf("matmul acc=%.6f\n", acc);
    return 0;
}
"""),

("matmul_blocked", r"""
#include <stdio.h>

/* Blocked (tiled) i-k-j matrix multiply: loop nest with different reuse. */
#define D 96
#define BS 24

static double A[D][D], B[D][D], C[D][D];

int main(void) {
    for (int i = 0; i < D; i++)
        for (int j = 0; j < D; j++) {
            A[i][j] = (i == j) ? 2.0 : ((i + j) % 3) * 0.5;
            B[i][j] = ((i ^ j) % 4) - 1.5;
        }
    for (int ii = 0; ii < D; ii += BS)
        for (int kk = 0; kk < D; kk += BS)
            for (int jj = 0; jj < D; jj += BS)
                for (int i = ii; i < ii + BS; i++)
                    for (int k = kk; k < kk + BS; k++) {
                        double aik = A[i][k];
                        for (int j = jj; j < jj + BS; j++)
                            C[i][j] += aik * B[k][j];
                    }
    double acc = 0;
    for (int i = 0; i < D; i += 5)
        for (int j = 0; j < D; j += 7) acc += C[i][j];
    printf("matmulblk acc=%.6f\n", acc);
    return 0;
}
"""),

("saxpy", r"""
#include <stdio.h>

/* SAXPY-style loops in two shapes: one fused, one split by destination. */
#define N 4096

static float x[N], y[N], w[N], v1[N], v2[N];

int main(void) {
    unsigned s = 90210;
    for (int i = 0; i < N; i++) {
        s = s * 22695477u + 1u;
        x[i] = (float)((int)s % 1024) / 64.0f;
        y[i] = (float)(i % 33) - 16.0f;
    }
    for (int rep = 0; rep < 10; rep++) {
        for (int i = 0; i < N; i++) w[i] = 2.5f * x[i] + y[i];
        for (int i = 0; i < N; i++) v1[i] = w[i] * 0.5f;
        for (int i = 0; i < N; i++) v2[i] = v1[i] + 0.125f * y[i];
        for (int i = 0; i < N; i++) x[i] = v2[i] - w[i];
    }
    double acc = 0;
    for (int i = 0; i < N; i += 17) acc += x[i] + w[i];
    printf("saxpy acc=%.6f\n", acc);
    return 0;
}
"""),

# ---------------------------------------------------------------------------
# Floating point math
# ---------------------------------------------------------------------------
("horner_poly", r"""
#include <stdio.h>

/* Horner's method for polynomials in float and double, plus a derivative. */
static double poly_eval(const double *c, int deg, double x) {
    double r = c[deg];
    for (int i = deg - 1; i >= 0; i--) r = r * x + c[i];
    return r;
}

static float poly_evalf(const float *c, int deg, float x) {
    float r = c[deg];
    for (int i = deg - 1; i >= 0; i--) r = r * x + c[i];
    return r;
}

int main(void) {
    double cd[17];
    float cf[17];
    for (int i = 0; i <= 16; i++) {
        cd[i] = 1.0 / (double)(i + 1);
        cf[i] = (float)cd[i];
    }
    double acc = 0;
    for (int k = 0; k <= 64; k++) {
        double x = -2.0 + k / 16.0;
        float xf = (float)x;
        acc += poly_eval(cd, 16, x) - poly_evalf(cf, 16, xf);
        acc += 0.5 * poly_eval(cd, 16, x * 0.5);
    }
    printf("horner acc=%.9f\n", acc);
    return 0;
}
"""),

("sin_lut", r"""
#include <stdio.h>

/* Sine lookup table built at startup, then linear interpolation. */
#define LUT_SIZE 1024

static float lut[LUT_SIZE + 1];

int main(void) {
    for (int i = 0; i <= LUT_SIZE; i++)
        lut[i] = (float)__builtin_sin(6.283185307179586 * i / LUT_SIZE);
    double acc = 0;
    for (int i = 0; i < 20000; i++) {
        double t = (i * 13) % 4096;
        double frac = t / 4096.0 * LUT_SIZE;
        int idx = (int)frac;
        double f = frac - idx;
        double s = lut[idx] + f * (lut[idx + 1] - lut[idx]);
        acc += s * s;
    }
    printf("sinlut acc=%.9f lutmax=%.6f\n", acc, lut[LUT_SIZE / 4]);
    return 0;
}
"""),

("fixed_point", r"""
#include <stdio.h>
#include <stdint.h>

/* Q16.16 fixed-point multiply and a Newton-Raphson integer sqrt. */
typedef int32_t fp16;

static fp16 fp_mul(fp16 a, fp16 b) {
    return (fp16)(((int64_t)a * b) >> 16);
}

static uint32_t isqrt(uint32_t n) {
    uint32_t x = n, y = (x + 1) / 2;
    while (y < x) {
        x = y;
        y = (x + n / x) / 2;
    }
    return x;
}

int main(void) {
    fp16 a = 3 * 65536 + 14159 / 10000;  /* ~3.14 */
    fp16 b = 2 * 65536 + 71828 / 10000;  /* ~2.72 */
    fp16 prod = fp_mul(a, b);
    unsigned acc = 0;
    for (uint32_t i = 2; i < 100000; i += 3) acc ^= isqrt(i * 97);
    for (int i = 0; i < 32; i++) acc += (unsigned)fp_mul((fp16)(1 << i), prod);
    printf("fixed prod=%d acc=%u\n", prod, acc);
    return 0;
}
"""),

# ---------------------------------------------------------------------------
# Bit twiddling
# ---------------------------------------------------------------------------
("popcount", r"""
#include <stdio.h>
#include <stdint.h>

/* Population count three ways, cross-checked against the builtin. */
static int pop_naive(uint32_t v) {
    int c = 0;
    while (v) { c += v & 1u; v >>= 1; }
    return c;
}

static int pop_swar(uint32_t v) {
    v = v - ((v >> 1) & 0x55555555u);
    v = (v & 0x33333333u) + ((v >> 2) & 0x33333333u);
    v = (v + (v >> 4)) & 0x0F0F0F0Fu;
    return (int)((v * 0x01010101u) >> 24);
}

int main(void) {
    long acc = 0;
    uint32_t s = 0xC0FFEE11u;
    for (int i = 0; i < 65536; i++) {
        s ^= s << 7; s ^= s >> 3; s ^= s << 12;
        int a = pop_naive(s), b = pop_swar(s), c = __builtin_popcount(s);
        acc += a + b + c;
        if (a != b || b != c) acc += 1000;
    }
    printf("popcount acc=%ld\n", acc);
    return 0;
}
"""),

("bit_reverse", r"""
#include <stdio.h>
#include <stdint.h>

/* Reverse all 32 bits, plus byte-order swaps. */
static uint32_t rev_bits(uint32_t v) {
    v = ((v & 0x55555555u) << 1) | ((v >> 1) & 0x55555555u);
    v = ((v & 0x33333333u) << 2) | ((v >> 2) & 0x33333333u);
    v = ((v & 0x0F0F0F0Fu) << 4) | ((v >> 4) & 0x0F0F0F0Fu);
    v = ((v & 0x00FF00FFu) << 8) | ((v >> 8) & 0x00FF00FFu);
    return (v << 16) | (v >> 16);
}

static int is_palindrome32(uint32_t v) { return rev_bits(v) == v; }

int main(void) {
    long acc = 0;
    for (uint32_t i = 0; i < 200000; i++) {
        uint32_t r = rev_bits(i);
        acc += (r >> 24) ^ (i & 0xFF);
        if (is_palindrome32(i)) acc += 7;
        if ((i & 0xFFFF) == 0x00AA) acc += __builtin_bswap32(i) & 0xFFFF;
    }
    printf("bitrev acc=%ld\n", acc);
    return 0;
}
"""),

("gray_code", r"""
#include <stdio.h>
#include <stdint.h>

/* Gray-code encode/decode and a Gray-reflected walk over 10 bits. */
static uint32_t gray_encode(uint32_t n) { return n ^ (n >> 1); }

static uint32_t gray_decode(uint32_t g) {
    uint32_t n = g;
    n ^= n >> 16;
    n ^= n >> 8;
    n ^= n >> 4;
    n ^= n >> 2;
    n ^= n >> 1;
    return n;
}

int main(void) {
    long acc = 0;
    for (uint32_t n = 0; n < 1u << 16; n++) {
        uint32_t g = gray_encode(n);
        if (gray_decode(g) != n) acc += 999;
        acc += __builtin_popcount(g ^ gray_encode(n + 1)) - 1; /* Hamming 1 */
    }
    uint32_t g = 0;
    for (int step = 0; step < 1023; step++) {
        g = gray_encode(g + 1);
        acc ^= g & 0x3FF;
    }
    printf("gray acc=%ld\n", acc);
    return 0;
}
"""),

# ---------------------------------------------------------------------------
# State machines
# ---------------------------------------------------------------------------
("fsm_traffic", r"""
#include <stdio.h>

/* Traffic-light controller: explicit enum states with tick timing. */
enum { RED, GREEN, AMBER, RED_AMBER, STATES };
static const char *names[STATES] = { "red", "green", "amber", "red+amber" };
static const int dwell[STATES] = { 50, 40, 8, 10 };

int main(void) {
    int state = RED, left = dwell[RED];
    unsigned long acc = 0;
    for (int tick = 0; tick < 5000; tick++) {
        if (--left <= 0) {
            switch (state) {
                case RED:       state = GREEN; break;
                case GREEN:     state = AMBER; break;
                case AMBER:     state = RED_AMBER; break;
                case RED_AMBER: state = RED; break;
                default:        state = RED; break;
            }
            left = dwell[state];
            acc += (unsigned long)state * 1000003u;
        } else {
            acc += (unsigned long)left;
        }
        if (tick % 997 == 0) acc ^= (unsigned long)names[state][0];
    }
    printf("traffic acc=%lu state=%s\n", acc, names[state]);
    return 0;
}
"""),

("fsm_lock", r"""
#include <stdio.h>

/* Combination-lock FSM over a fixed digit stream; wrong digits reset. */
static const int combo[4] = { 3, 7, 1, 9 };

static int step_lock(int state, int digit) {
    if (digit == combo[state]) return state + 1;
    return (digit == combo[0]) ? 1 : 0;
}

int main(void) {
    unsigned s = 8675309;
    long opens = 0, resets = 0;
    for (long i = 0; i < 100000; i++) {
        s = s * 1103515245u + 12345u;
        int digit = (int)((s >> 16) % 10);
        static int state = 0;
        int ns = step_lock(state, digit);
        if (ns == 4) { opens++; ns = 0; }
        if (ns == 0 && state != 0) resets++;
        state = ns;
        if ((i & 0xFFF) == 0) state = 0;
    }
    printf("lock opens=%ld resets=%ld\n", opens, resets);
    return 0;
}
"""),

("fsm_match", r"""
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
"""),

# ---------------------------------------------------------------------------
# Linked structures and recursion
# ---------------------------------------------------------------------------
("linked_list", r"""
#include <stdio.h>
#include <stdlib.h>

/* Singly linked list: sorted insert, remove, reverse, count. */
typedef struct node {
    int val;
    struct node *next;
} node;

static node *insert_sorted(node *head, int v) {
    node *n = malloc(sizeof *n);
    n->val = v;
    node **pp = &head;
    while (*pp && (*pp)->val < v) pp = &(*pp)->next;
    n->next = *pp;
    *pp = n;
    return head;
}

static node *remove_val(node *head, int v) {
    node **pp = &head;
    while (*pp && (*pp)->val != v) pp = &(*pp)->next;
    if (*pp) {
        node *dead = *pp;
        *pp = dead->next;
        free(dead);
    }
    return head;
}

static node *reverse(node *head) {
    node *prev = NULL;
    while (head) {
        node *nx = head->next;
        head->next = prev;
        prev = head;
        head = nx;
    }
    return prev;
}

int main(void) {
    node *head = NULL;
    unsigned s = 2718;
    for (int i = 0; i < 300; i++) {
        s = s * 1103515245u + 12345u;
        head = insert_sorted(head, (int)(s >> 16) % 500);
    }
    for (int v = 0; v < 500; v += 37) head = remove_val(head, v);
    head = reverse(head);
    long acc = 0;
    int count = 0;
    for (node *p = head; p; p = p->next) { acc += p->val; count++; }
    printf("list acc=%ld count=%d head=%d\n", acc, count, head ? head->val : -1);
    while (head) { node *n = head->next; free(head); head = n; }
    return 0;
}
"""),

("bst_ops", r"""
#include <stdio.h>
#include <stdlib.h>

/* Unbalanced binary search tree: insert, search, in-order walk, height. */
typedef struct btn {
    int key;
    struct btn *l, *r;
} btn;

static btn *btn_new(int key) {
    btn *n = malloc(sizeof *n);
    n->key = key;
    n->l = n->r = NULL;
    return n;
}

static btn *bst_insert(btn *root, int key) {
    if (!root) return btn_new(key);
    if (key < root->key) root->l = bst_insert(root->l, key);
    else if (key > root->key) root->r = bst_insert(root->r, key);
    return root;
}

static int bst_has(const btn *root, int key) {
    while (root) {
        if (key < root->key) root = root->l;
        else if (key > root->key) root = root->r;
        else return 1;
    }
    return 0;
}

static long walk(const btn *n, long acc) {
    if (!n) return acc;
    acc = walk(n->l, acc);
    return walk(n->r, acc + n->key);
}

static int height(const btn *n) {
    if (!n) return 0;
    int hl = height(n->l), hr = height(n->r);
    return 1 + (hl > hr ? hl : hr);
}

int main(void) {
    btn *root = NULL;
    unsigned s = 112358;
    for (int i = 0; i < 1000; i++) {
        s = s * 6364136223846793005u + 1442695040888963407u;
        root = bst_insert(root, (int)((s >> 33) % 4096));
    }
    long acc = walk(root, 0);
    int hits = 0;
    for (int k = 0; k < 4096; k += 61) hits += bst_has(root, k);
    printf("bst acc=%ld hits=%d height=%d\n", acc, hits, height(root));
    return 0;
}
"""),

("funcptr_table", r"""
#include <stdio.h>

/* Dispatch through a table of function pointers chosen by a data stream. */
typedef int (*op_fn)(int);

static int op_inc(int x) { return x + 3; }
static int op_sq(int x) { return x * x % 100003; }
static int op_fold(int x) { return (x >> 1) ^ (x << 1); }
static int op_rev(int x) {
    int r = 0;
    while (x) { r = r * 10 + x % 10; x /= 10; }
    return r % 100003;
}
static int op_mix(int x) { return op_sq(x) ^ op_fold(x); }

static const op_fn ops[] = { op_inc, op_sq, op_fold, op_rev, op_mix };
static const char *op_names[] = { "inc", "sq", "fold", "rev", "mix" };

int main(void) {
    unsigned s = 6060;
    long acc = 0;
    for (int i = 0; i < 20000; i++) {
        s = s * 48271u % 2147483647u;
        int which = (int)(s % 5);
        int v = (int)(s % 50000);
        acc += ops[which](v);
        if ((i & 4095) == 0) acc += op_names[which][0];
    }
    printf("funcptr acc=%ld\n", acc);
    return 0;
}
"""),

("varargs_sum", r"""
#include <stdio.h>
#include <stdarg.h>

/* Variadic functions: a sentinel-terminated int sum and a mixed-type fold. */
static int sum_all(int first, ...) {
    va_list ap;
    va_start(ap, first);
    int total = first;
    int v;
    while ((v = va_arg(ap, int)) != -1) total += v * 3 % 97;
    va_end(ap);
    return total;
}

static double mix_all(int n, ...) {
    va_list ap;
    va_start(ap, n);
    double acc = 0.0;
    for (int i = 0; i < n; i++) {
        if (i & 1) acc += va_arg(ap, double);
        else acc += va_arg(ap, int) * 0.125;
    }
    va_end(ap);
    return acc;
}

int main(void) {
    int t = 0;
    for (int i = 1; i <= 12; i++)
        t += sum_all(i, i + 1, i * 2, -1);
    double d = mix_all(10, 1, 2.5, 3, 4.5, 5, 6.5, 7, 8.5, 9, 10.5);
    printf("varargs t=%d d=%.6f\n", t, d);
    return 0;
}
"""),

("mini_vprintf", r"""
#include <stdio.h>
#include <stdarg.h>
#include <string.h>

/* A miniature printf supporting %d, %x and %s, built on va_arg. */
static void emit(char *out, size_t *n, size_t cap, const char *s) {
    while (*s && *n + 1 < cap) out[(*n)++] = *s++;
}

static void mini_vsprintf(char *out, size_t cap, const char *fmt, va_list ap) {
    size_t n = 0;
    for (const char *p = fmt; *p; p++) {
        if (*p != '%') { if (n + 1 < cap) out[n++] = *p; continue; }
        p++;
        char tmp[24];
        if (*p == 'd') {
            int v = va_arg(ap, int);
            unsigned u = (v < 0) ? (unsigned)(-(long)v) : (unsigned)v;
            int i = 0;
            do { tmp[i++] = (char)('0' + u % 10u); u /= 10u; } while (u);
            if (v < 0 && n + 1 < cap) out[n++] = '-';
            while (i && n + 1 < cap) out[n++] = tmp[--i];
        } else if (*p == 'x') {
            unsigned v = va_arg(ap, unsigned);
            const char *hex = "0123456789abcdef";
            int i = 0;
            do { tmp[i++] = hex[v & 15u]; v >>= 4; } while (v);
            while (i && n + 1 < cap) out[n++] = tmp[--i];
        } else if (*p == 's') {
            emit(out, &n, cap, va_arg(ap, const char *));
        } else {
            if (n + 1 < cap) out[n++] = *p ? *p : '%';
        }
    }
    out[n] = '\0';
}

static void mini_sprintf(char *out, size_t cap, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    mini_vsprintf(out, cap, fmt, ap);
    va_end(ap);
}

int main(void) {
    char buf[256];
    unsigned acc = 0;
    const char *tags[] = {"alpha", "beta", "gamma", "delta"};
    for (int i = 0; i < 24; i++) {
        mini_sprintf(buf, sizeof buf, "%s:%d:%x|", tags[i & 3], i * 13, i * 7 + 1);
        for (const char *p = buf; *p; p++) acc = acc * 31u + (unsigned char)*p;
    }
    printf("minivp acc=%u len=%zu\n", acc, strlen(buf));
    return 0;
}
"""),

# ---------------------------------------------------------------------------
# Switch tables
# ---------------------------------------------------------------------------
("switch_table", r"""
#include <stdio.h>

/* Dense switch over 0..15 in a loop: the classic jump-table shape. */
static int classify(int op, int v) {
    switch (op) {
        case 0:  return v + 1;
        case 1:  return v - 1;
        case 2:  return v * 2;
        case 3:  return v / 2;
        case 4:  return v ^ 0x55;
        case 5:  return v << 3;
        case 6:  return v >> 1;
        case 7:  return ~v;
        case 8:  return v | 0x100;
        case 9:  return v & 0x1FF;
        case 10: return -v;
        case 11: return v % 97;
        case 12: return v + 0x400;
        case 13: return v * 31 + 7;
        case 14: return v * 3 / 2;
        case 15: return v * v % 4093;
        default: return 0;
    }
}

int main(void) {
    unsigned s = 1;
    long acc = 0;
    for (int i = 0; i < 30000; i++) {
        s = s * 1664525u + 1013904223u;
        acc += classify((int)(s >> 28) & 15, (int)(s & 0xFFFF));
    }
    printf("switch acc=%ld\n", acc);
    return 0;
}
"""),

("switch_sparse", r"""
#include <stdio.h>

/* Sparse switch: widely separated cases, no compact jump table. */
static long handle(unsigned code, long v) {
    switch (code) {
        case 1:       return v + 1000000000L;
        case 99:      return v * 3;
        case 500:     return v - 7;
        case 2500:    return v ^ 0x12345678L;
        case 10000:   return v >> 2;
        case 44444:   return v * v % 1000000007L;
        case 100000:  return ~v;
        case 500000:  return v + (v << 5);
        case 1000000: return (v << 3) - v;
        default:      return v % 13;
    }
}

int main(void) {
    unsigned s = 314159;
    long acc = 0;
    for (int i = 0; i < 40000; i++) {
        s = s * 69069u + 1u;
        acc += handle(s % 1000001, (long)(s >> 8) % 1000);
    }
    printf("sparse acc=%ld\n", acc);
    return 0;
}
"""),

# ---------------------------------------------------------------------------
# Algorithms and number theory
# ---------------------------------------------------------------------------
("sieve_primes", r"""
#include <stdio.h>
#include <string.h>

/* Sieve of Eratosthenes to 8192, then a few prime-gap statistics. */
#define LIMIT 8192

int main(void) {
    static unsigned char is_comp[LIMIT + 1];
    memset(is_comp, 0, sizeof is_comp);
    int count = 0, last = 2;
    long gap_sum = 0, gap_max = 0;
    for (int i = 2; i <= LIMIT; i++) {
        if (!is_comp[i]) {
            count++;
            long gap = i - last;
            gap_sum += gap;
            if (gap > gap_max) gap_max = gap;
            last = i;
            for (int j = i * i; j <= LIMIT; j += i) is_comp[j] = 1;
        }
    }
    unsigned acc = 0;
    for (int i = 2; i <= LIMIT; i++) if (!is_comp[i]) acc ^= (unsigned)i;
    printf("sieve count=%d gapsum=%ld gapmax=%ld acc=%u\n",
           count, gap_sum, gap_max, acc);
    return 0;
}
"""),

("collatz_gcd", r"""
#include <stdio.h>

/* Collatz lengths plus Euclid gcd and modular exponentiation. */
static int collatz_len(unsigned long n) {
    int steps = 0;
    while (n != 1) {
        n = (n & 1) ? 3 * n + 1 : n / 2;
        steps++;
    }
    return steps;
}

static unsigned long gcd(unsigned long a, unsigned long b) {
    while (b) { unsigned long t = a % b; a = b; b = t; }
    return a;
}

static unsigned long modpow(unsigned long b, unsigned long e, unsigned long m) {
    unsigned long r = 1;
    b %= m;
    while (e) {
        if (e & 1) r = r * b % m;
        b = b * b % m;
        e >>= 1;
    }
    return r;
}

int main(void) {
    long acc = 0;
    for (unsigned long n = 1; n <= 5000; n++) acc += collatz_len(n);
    for (unsigned long i = 1; i < 500; i++)
        acc += (long)gcd(2197 * i + 1, 3861 * i + 2);
    for (unsigned long i = 1; i < 200; i++)
        acc += (long)modpow(7 + i, 101, 1009 * 3);
    printf("collatz acc=%ld\n", acc);
    return 0;
}
"""),

("life_1d", r"""
#include <stdio.h>
#include <stdint.h>

/* Rule-30 one-dimensional cellular automaton, 96 cells, 64 generations. */
#define CELLS 96
#define GENS 64

static uint8_t row_a[CELLS + 2], row_b[CELLS + 2];

int main(void) {
    row_a[CELLS / 2] = 1;
    uint8_t *cur = row_a, *nxt = row_b;
    unsigned acc = 0;
    for (int g = 0; g < GENS; g++) {
        for (int i = 0; i < CELLS; i++) {
            uint8_t l = cur[i], c = cur[i + 1], r = cur[i + 2];
            uint8_t pattern = (uint8_t)((l << 2) | (c << 1) | r);
            nxt[i + 1] = (pattern == 1 || pattern == 2 ||
                          pattern == 3 || pattern == 4) ? 1 : 0;
            acc = (acc << 1) | nxt[i + 1];
        }
        uint8_t *t = cur;
        cur = nxt;
        nxt = t;
    }
    printf("life acc=%u live=%u\n", acc, (unsigned)(cur[CELLS / 2]));
    return 0;
}
"""),

("bresenham", r"""
#include <stdio.h>
#include <stdint.h>

/* Bresenham line rasterization over a 128x128 grid, many angles. */
#define W 128
#define H 128

static uint8_t grid[H][W];

static void line(int x0, int y0, int x1, int y1) {
    int dx = (x1 > x0) ? x1 - x0 : x0 - x1;
    int dy = (y1 > y0) ? y1 - y0 : y0 - y1;
    int sx = (x0 < x1) ? 1 : -1;
    int sy = (y0 < y1) ? 1 : -1;
    int err = dx - dy;
    for (;;) {
        if (x0 >= 0 && x0 < W && y0 >= 0 && y0 < H) grid[y0][x0]++;
        if (x0 == x1 && y0 == y1) break;
        int e2 = 2 * err;
        if (e2 > -dy) { err -= dy; x0 += sx; }
        if (e2 < dx) { err += dx; y0 += sy; }
    }
}

int main(void) {
    for (int a = 0; a < 360; a += 3) {
        int cx = W / 2, cy = H / 2;
        int r = 60;
        line(cx, cy, cx + r * (100 + a) / 141, cy + r * (141 - a) / 141);
    }
    unsigned acc = 0;
    for (int y = 0; y < H; y++)
        for (int x = 0; x < W; x++) acc = acc * 3u + grid[y][x];
    printf("bresenham acc=%u\n", acc);
    return 0;
}
"""),

("stack_machine", r"""
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
"""),
("export_demo", r"""
/* Export-table probe: four small pure functions behind a portable export
 * macro, plus a main that folds their results into one checksum.
 *
 * On the windows targets EXPORT expands to __declspec(dllexport), so those
 * builds carry an export directory and `compid functions` can enumerate the
 * four functions from it; the source also builds as a windows DLL
 * (export_demo's -dll matrix rows). On ELF and Mach-O builds EXPORT is
 * empty and the same functions appear as regular symbol-table entries.
 * Bodies are deliberately tiny and deterministic: fixed inputs, own
 * arithmetic, no libc beyond printf, and a printed checksum so -O2 cannot
 * delete the work. */
#include <stdio.h>

#if defined(_WIN32)
#define EXPORT __declspec(dllexport)
#else
#define EXPORT
#endif

EXPORT unsigned export_add3(unsigned a, unsigned b, unsigned c) {
    return a + b + c;
}

EXPORT unsigned export_djb2(const char *s) {
    unsigned h = 5381u;
    while (*s) {
        h = h * 33u ^ (unsigned char)*s++;
    }
    return h;
}

EXPORT unsigned export_mix(unsigned x) {
    x ^= x >> 15;
    x *= 0x2c1b3c6du;
    x ^= x >> 12;
    x *= 0x297a2d39u;
    x ^= x >> 15;
    return x;
}

EXPORT long export_scale(long v, long factor) {
    return v * factor - (v >> 3);
}

int main(void) {
    const char *words[3] = {"export", "demo", "corpus"};
    unsigned acc = export_add3(7u, 11u, 13u);
    for (int i = 0; i < 3; i++) {
        acc = export_mix(acc ^ export_djb2(words[i]));
    }
    acc ^= (unsigned)export_scale(12345L, -3L);
    printf("export demo acc=%u\n", acc);
    return 0;
}
"""),
]

CXX_SOURCES = [
("cpp_iostream", r"""
#include <iostream>
#include <iomanip>

// Stream formatting: std::cout with manipulators, a modest amount of
// libstdc++ code in the binary.
int main() {
    double vals[8] = {1.5, 2.25, -3.125, 4.0, 0.5, -0.75, 8.125, 16.0};
    double acc = 0;
    for (int i = 0; i < 8; i++) {
        acc += vals[i];
        std::cout << std::setw(3) << i << ' '
                  << std::fixed << std::setprecision(4) << vals[i] << ' '
                  << std::hex << std::showbase
                  << (unsigned)(vals[i] * 16 + 64) << '\n';
    }
    std::cout << "acc=" << acc << std::endl;
    return 0;
}
"""),

("cpp_vector_sort", r"""
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
"""),

("cpp_string_ops", r"""
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
"""),

("cpp_virtual_shapes", r"""
#include <cstdio>
#include <memory>
#include <vector>

// Virtual dispatch: vtables and indirect calls through base pointers.
struct shape {
    virtual ~shape() = default;
    virtual double area() const = 0;
    virtual const char *kind() const = 0;
};

struct circle : shape {
    double r;
    explicit circle(double r) : r(r) {}
    double area() const override { return 3.14159265358979 * r * r; }
    const char *kind() const override { return "circle"; }
};

struct rect : shape {
    double w, h;
    rect(double w, double h) : w(w), h(h) {}
    double area() const override { return w * h; }
    const char *kind() const override { return "rect"; }
};

struct tri : shape {
    double b, h;
    tri(double b, double h) : b(b), h(h) {}
    double area() const override { return 0.5 * b * h; }
    const char *kind() const override { return "tri"; }
};

int main() {
    std::vector<std::unique_ptr<shape>> shapes;
    for (int i = 0; i < 90; i++) {
        double k = i + 1;
        switch (i % 3) {
            case 0: shapes.push_back(std::make_unique<circle>(k * 0.5)); break;
            case 1: shapes.push_back(std::make_unique<rect>(k, k * 0.25)); break;
            default: shapes.push_back(std::make_unique<tri>(k, k * 0.75)); break;
        }
    }
    double total = 0;
    long name_hash = 0;
    for (const auto &s : shapes) {
        total += s->area();
        for (const char *p = s->kind(); *p; p++) name_hash = name_hash * 31 + *p;
    }
    std::printf("shapes total=%.6f hash=%ld n=%zu\n", total, name_hash, shapes.size());
    return 0;
}
"""),

("cpp_template_meta", r"""
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
"""),

("cpp_lambda_algo", r"""
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
"""),

("cpp_map_freq", r"""
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
"""),

("cpp_exceptions", r"""
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
"""),
]


def write_sources(out_dir, force=False):
    """Write every source file. Returns (written, skipped)."""
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    written = skipped = 0
    for sid, code in C_SOURCES:
        path = out / f"{sid}.c"
        if path.exists() and not force:
            skipped += 1
            continue
        path.write_text(code.lstrip("\n"))
        written += 1
    for sid, code in CXX_SOURCES:
        path = out / f"{sid}.cpp"
        if path.exists() and not force:
            skipped += 1
            continue
        path.write_text(code.lstrip("\n"))
        written += 1
    return written, skipped


def main():
    parser = argparse.ArgumentParser(
        description="Write the deterministic C/C++ corpus into corpus/sources/."
    )
    parser.add_argument("--out", default=str(Path(__file__).resolve().parent / "sources"),
                        help="output directory (default: corpus/sources)")
    parser.add_argument("--force", action="store_true",
                        help="overwrite existing source files")
    args = parser.parse_args()

    if not (40 <= len(C_SOURCES) <= 60):
        print(f"error: implausible C source count {len(C_SOURCES)}", file=sys.stderr)
        return 1
    if len(CXX_SOURCES) != 8:
        print(f"error: expected 8 C++ sources, found {len(CXX_SOURCES)}", file=sys.stderr)
        return 1

    names = [n for n, _ in C_SOURCES + CXX_SOURCES]
    if len(set(names)) != len(names):
        dupes = sorted({n for n in names if names.count(n) > 1})
        print(f"error: duplicate source names: {dupes}", file=sys.stderr)
        return 1

    written, skipped = write_sources(args.out, force=args.force)
    total = len(C_SOURCES) + len(CXX_SOURCES)
    print(f"sources: {total} programs ({len(C_SOURCES)} C, {len(CXX_SOURCES)} C++) "
          f"-> {args.out} ({written} written, {skipped} already present)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
