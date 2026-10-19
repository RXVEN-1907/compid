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
