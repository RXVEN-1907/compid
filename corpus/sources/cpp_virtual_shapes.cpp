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
