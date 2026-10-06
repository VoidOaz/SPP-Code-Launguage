use crate::{native, value::Value};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const BUILTIN_MODULES: &[&str] = &[
    "Math", "math",
    "Time", "time",
    "Random", "random",
    "Spp3D", "spp3d",
    "File", "file",
    "OS", "os",
];

static RNG_STATE: OnceLock<Mutex<u64>> = OnceLock::new();

fn rng_state() -> &'static Mutex<u64> {
    RNG_STATE.get_or_init(|| {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15);
        Mutex::new(seed ^ 0xA5A5_5A5A_DEAD_BEEF)
    })
}

pub fn canonical_module(name: &str) -> Option<&'static str> {
    BUILTIN_MODULES.iter().copied().find(|candidate| candidate.eq_ignore_ascii_case(name))
}

pub fn is_builtin_module(name: &str) -> bool {
    canonical_module(name).is_some()
}

pub fn is_allowed_member(module: &str, member: &str) -> bool {
    let m = module.to_ascii_lowercase();
    let f = member.to_ascii_lowercase();
    match m.as_str() {
        "math" => matches!(f.as_str(), "pi"|"e"|"tau"|"sqrt"|"cbrt"|"abs"|"floor"|"ceil"|"round"|"min"|"max"|"pow"|"sin"|"cos"|"tan"|"asin"|"acos"|"atan"|"atan2"|"log"|"ln"|"exp"),
        "time" => matches!(f.as_str(), "now"|"now_ms"|"unix_ms"|"sleep_ms"|"monotonic_ms"),
        "random" => matches!(f.as_str(), "int"|"float"|"bool"|"seed"),
        "spp3d" => matches!(f.as_str(), "version"|"vec3"|"add"|"sub"|"mul"|"dot"|"length"|"distance"|"normalize"),
        "file" => matches!(f.as_str(), "read"|"write"|"exists"|"delete"),
        "os" => matches!(f.as_str(), "cwd"|"env"),
        _ => false,
    }
}

fn expect(args: &[Value], n: usize, name: &str) -> Result<(), String> {
    if args.len() == n {
        Ok(())
    } else {
        Err(format!("SPP runtime: {name} expects {n} argument(s), got {}", args.len()))
    }
}

fn number(v: &Value, name: &str) -> Result<f64, String> {
    match v {
        Value::Int(x) => Ok(*x as f64),
        Value::Float(x) => Ok(*x),
        _ => Err(format!("SPP runtime: {name} expects a number")),
    }
}

fn int(v: &Value, name: &str) -> Result<i64, String> {
    v.as_int().ok_or_else(|| format!("SPP runtime: {name} expects an int"))
}

fn random_u64() -> u64 {
    let mut state = rng_state().lock().expect("rng mutex poisoned");
    let mut x = *state;
    if x == 0 { x = 0x9E3779B97F4A7C15; }
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

pub fn module_constant(module: &str, member: &str) -> Option<Value> {
    match (module.to_ascii_lowercase().as_str(), member.to_ascii_lowercase().as_str()) {
        ("math", "pi") => Some(Value::Float(std::f64::consts::PI)),
        ("math", "e") => Some(Value::Float(std::f64::consts::E)),
        ("math", "tau") => Some(Value::Float(std::f64::consts::TAU)),
        ("spp3d", "version") => Some(Value::String("0.1-native-math".into())),
        _ => None,
    }
}

pub fn module_call(module: &str, member: &str, args: &[Value], monotonic_ms: i64) -> Result<Value, String> {
    match (module.to_ascii_lowercase().as_str(), member.to_ascii_lowercase().as_str()) {
        ("math", "sqrt") => { expect(args, 1, "Math.sqrt")?; Ok(Value::Float(native::sqrt(number(&args[0], "Math.sqrt")?))) }
        ("math", "cbrt") => { expect(args, 1, "Math.cbrt")?; Ok(Value::Float(number(&args[0], "Math.cbrt")?.cbrt())) }
        ("math", "abs") => { expect(args, 1, "Math.abs")?; match &args[0] { Value::Int(v) => Ok(Value::Int(v.checked_abs().ok_or_else(|| "SPP runtime: integer overflow".to_string())?)), Value::Float(v) => Ok(Value::Float(v.abs())), _ => Err("SPP runtime: Math.abs expects a number".into()) } }
        ("math", "floor") => { expect(args, 1, "Math.floor")?; Ok(Value::Float(number(&args[0], "Math.floor")?.floor())) }
        ("math", "ceil") => { expect(args, 1, "Math.ceil")?; Ok(Value::Float(number(&args[0], "Math.ceil")?.ceil())) }
        ("math", "round") => { expect(args, 1, "Math.round")?; Ok(Value::Float(number(&args[0], "Math.round")?.round())) }
        ("math", "min") | ("math", "max") => {
            expect(args, 2, "Math.min/max")?;
            let a = number(&args[0], "Math.min/max")?;
            let b = number(&args[1], "Math.min/max")?;
            Ok(Value::Float(if member.eq_ignore_ascii_case("min") { a.min(b) } else { a.max(b) }))
        }
        ("math", "pow") => { expect(args, 2, "Math.pow")?; Ok(Value::Float(native::pow(number(&args[0], "Math.pow")?, number(&args[1], "Math.pow")?))) }
        ("math", "sin") => { expect(args, 1, "Math.sin")?; Ok(Value::Float(native::sin(number(&args[0], "Math.sin")?))) }
        ("math", "cos") => { expect(args, 1, "Math.cos")?; Ok(Value::Float(native::cos(number(&args[0], "Math.cos")?))) }
        ("math", "tan") => { expect(args, 1, "Math.tan")?; Ok(Value::Float(number(&args[0], "Math.tan")?.tan())) }
        ("math", "asin") => { expect(args, 1, "Math.asin")?; Ok(Value::Float(number(&args[0], "Math.asin")?.asin())) }
        ("math", "acos") => { expect(args, 1, "Math.acos")?; Ok(Value::Float(number(&args[0], "Math.acos")?.acos())) }
        ("math", "atan") => { expect(args, 1, "Math.atan")?; Ok(Value::Float(number(&args[0], "Math.atan")?.atan())) }
        ("math", "atan2") => { expect(args, 2, "Math.atan2")?; Ok(Value::Float(number(&args[0], "Math.atan2")?.atan2(number(&args[1], "Math.atan2")?))) }
        ("math", "log") | ("math", "ln") => { expect(args, 1, "Math.log")?; Ok(Value::Float(number(&args[0], "Math.log")?.ln())) }
        ("math", "exp") => { expect(args, 1, "Math.exp")?; Ok(Value::Float(number(&args[0], "Math.exp")?.exp())) }

        ("time", "now") | ("time", "now_ms") | ("time", "unix_ms") => {
            expect(args, 0, "Time.now")?;
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?;
            Ok(Value::Int(now.as_millis() as i64))
        }
        ("time", "monotonic_ms") => { expect(args, 0, "Time.monotonic_ms")?; Ok(Value::Int(monotonic_ms)) }
        ("time", "sleep_ms") => {
            expect(args, 1, "Time.sleep_ms")?;
            std::thread::sleep(std::time::Duration::from_millis(int(&args[0], "Time.sleep_ms")?.max(0) as u64));
            Ok(Value::Null)
        }

        ("random", "int") => {
            expect(args, 2, "random.int")?;
            let a = int(&args[0], "random.int")?;
            let b = int(&args[1], "random.int")?;
            if a > b { return Err("SPP runtime: random.int min cannot be greater than max".into()); }
            let span = (b as i128 - a as i128 + 1) as u128;
            let value = (random_u64() as u128 % span) as i64;
            Ok(Value::Int(a + value))
        }
        ("random", "float") => { expect(args, 0, "random.float")?; Ok(Value::Float((random_u64() as f64) / (u64::MAX as f64))) }
        ("random", "bool") => { expect(args, 0, "random.bool")?; Ok(Value::Bool((random_u64() & 1) == 1)) }
        ("random", "seed") => {
            expect(args, 1, "random.seed")?;
            let mut state = rng_state().lock().expect("rng mutex poisoned");
            *state = int(&args[0], "random.seed")? as u64;
            Ok(Value::Null)
        }

        ("spp3d", "version") => { expect(args, 0, "Spp3D.version")?; Ok(Value::String("0.1-native-math".into())) }
        ("spp3d", "vec3") => {
            expect(args, 3, "Spp3D.vec3")?;
            Ok(Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
                args[0].clone(), args[1].clone(), args[2].clone()
            ]))))
        }
        ("spp3d", "add") | ("spp3d", "sub") | ("spp3d", "distance") | ("spp3d", "dot") => {
            expect(args, 2, "Spp3D vector operation")?;
            let a = vector3(&args[0], "Spp3D vector operation")?;
            let b = vector3(&args[1], "Spp3D vector operation")?;
            match member.to_ascii_lowercase().as_str() {
                "add" => Ok(vector3_value([a[0] + b[0], a[1] + b[1], a[2] + b[2]])),
                "sub" => Ok(vector3_value([a[0] - b[0], a[1] - b[1], a[2] - b[2]])),
                "distance" => Ok(Value::Float(native::sqrt((a[0]-b[0]).powi(2)+(a[1]-b[1]).powi(2)+(a[2]-b[2]).powi(2)))),
                "dot" => Ok(Value::Float(native::vec3_dot(a[0], a[1], a[2], b[0], b[1], b[2]))),
                _ => unreachable!(),
            }
        }
        ("spp3d", "mul") => {
            expect(args, 2, "Spp3D.mul")?;
            let v = vector3(&args[0], "Spp3D.mul")?;
            let s = number(&args[1], "Spp3D.mul")?;
            Ok(vector3_value([v[0]*s, v[1]*s, v[2]*s]))
        }
        ("spp3d", "length") => {
            expect(args, 1, "Spp3D.length")?;
            let v = vector3(&args[0], "Spp3D.length")?;
            Ok(Value::Float(native::vec3_length(v[0], v[1], v[2])))
        }
        ("spp3d", "normalize") => {
            expect(args, 1, "Spp3D.normalize")?;
            let v = vector3(&args[0], "Spp3D.normalize")?;
            let len = native::vec3_length(v[0], v[1], v[2]);
            if len == 0.0 { return Ok(vector3_value([0.0,0.0,0.0])); }
            Ok(vector3_value([v[0]/len, v[1]/len, v[2]/len]))
        }

        ("file", "read") => { expect(args, 1, "File.read")?; let p = string(&args[0], "File.read")?; Ok(Value::String(std::fs::read_to_string(p).map_err(|e| e.to_string())?)) }
        ("file", "write") => { expect(args, 2, "File.write")?; let p = string(&args[0], "File.write")?; let d = string(&args[1], "File.write")?; std::fs::write(p, d).map_err(|e| e.to_string())?; Ok(Value::Null) }
        ("file", "exists") => { expect(args, 1, "File.exists")?; let p = string(&args[0], "File.exists")?; Ok(Value::Bool(std::path::Path::new(p).exists())) }
        ("file", "delete") => { expect(args, 1, "File.delete")?; let p = string(&args[0], "File.delete")?; std::fs::remove_file(p).map_err(|e| e.to_string())?; Ok(Value::Null) }
        ("os", "cwd") => { expect(args, 0, "OS.cwd")?; Ok(Value::String(std::env::current_dir().map_err(|e| e.to_string())?.to_string_lossy().into_owned())) }
        ("os", "env") => { expect(args, 1, "OS.env")?; let key = string(&args[0], "OS.env")?; Ok(std::env::var(key).map(Value::String).unwrap_or(Value::Null)) }

        _ => Err(format!("SPP runtime: unknown module member '{module}.{member}'")),
    }
}

fn string<'a>(v: &'a Value, name: &str) -> Result<&'a str, String> {
    match v { Value::String(s) => Ok(s), _ => Err(format!("SPP runtime: {name} expects a string")) }
}

fn vector3(v: &Value, name: &str) -> Result<[f64; 3], String> {
    match v {
        Value::Array(a) => {
            let values = a.borrow();
            if values.len() != 3 { return Err(format!("SPP runtime: {name} expects a 3-component vector")); }
            Ok([number(&values[0], name)?, number(&values[1], name)?, number(&values[2], name)?])
        }
        _ => Err(format!("SPP runtime: {name} expects a 3-component vector")),
    }
}

fn vector3_value(v: [f64; 3]) -> Value {
    Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
        Value::Float(v[0]), Value::Float(v[1]), Value::Float(v[2])
    ])))
}
