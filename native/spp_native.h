#pragma once
#include <cstdint>

extern "C" {
    double spp_fast_sqrt(double v) noexcept;
    double spp_fast_sin(double v) noexcept;
    double spp_fast_cos(double v) noexcept;
    double spp_fast_pow(double a, double b) noexcept;
    double spp_vec3_length(double x, double y, double z) noexcept;
    double spp_vec3_dot(double ax, double ay, double az, double bx, double by, double bz) noexcept;
}
