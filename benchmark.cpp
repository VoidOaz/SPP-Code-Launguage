#include <chrono>
#include <iostream>

int main() {
    constexpr long long N = 5'000'000;
    constexpr int runs = 5;
    double total_ms = 0.0;
    long long checksum = 0;

    for (int r = 0; r < runs; ++r) {
        auto start = std::chrono::steady_clock::now();
        long long sum = 0;
        for (long long i = 0; i < N; ++i) {
            sum += (i % 97) * 3;
        }
        checksum = sum;
        auto end = std::chrono::steady_clock::now();
        total_ms += std::chrono::duration<double, std::milli>(end - start).count();
    }

    std::cout << "C++ avg: " << total_ms / runs << " ms\n";
    std::cout << "checksum: " << checksum << "\n";
}
