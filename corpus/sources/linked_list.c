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
