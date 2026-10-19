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
