unsafe extern "C" {
    fn spp_fast_sqrt(v: f64) -> f64;
    fn spp_fast_sin(v: f64) -> f64;
    fn spp_fast_cos(v: f64) -> f64;
    fn spp_fast_pow(a: f64, b: f64) -> f64;
    fn spp_vec3_length(x: f64, y: f64, z: f64) -> f64;
    fn spp_vec3_dot(ax: f64, ay: f64, az: f64, bx: f64, by: f64, bz: f64) -> f64;
}

pub fn sqrt(v: f64) -> f64 { unsafe { spp_fast_sqrt(v) } }
pub fn sin(v: f64) -> f64 { unsafe { spp_fast_sin(v) } }
pub fn cos(v: f64) -> f64 { unsafe { spp_fast_cos(v) } }
pub fn pow(a: f64, b: f64) -> f64 { unsafe { spp_fast_pow(a, b) } }
pub fn vec3_length(x: f64, y: f64, z: f64) -> f64 { unsafe { spp_vec3_length(x, y, z) } }
pub fn vec3_dot(ax: f64, ay: f64, az: f64, bx: f64, by: f64, bz: f64) -> f64 { unsafe { spp_vec3_dot(ax, ay, az, bx, by, bz) } }
