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
