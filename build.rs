use std::{env, fs, path::PathBuf, process::Command};

fn run(mut command: Command, label: &str) {
    let status = command.status().unwrap_or_else(|e| panic!("SPP build: failed to start {label}: {e}"));
    if !status.success() { panic!("SPP build: {label} failed with status {status}"); }
}

fn main() {
    println!("cargo:rerun-if-changed=native/spp_native.cpp");
    println!("cargo:rerun-if-changed=native/spp_native.c");
    println!("cargo:rerun-if-changed=native/spp_native.h");

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing"));
    fs::create_dir_all(&out).expect("cannot create OUT_DIR");
    let target = env::var("TARGET").unwrap_or_default();

    if target.contains("windows-msvc") {
        let obj = out.join("spp_native.obj");
        let cobj = out.join("spp_native_c.obj");
        let lib = out.join("spp_native.lib");
        let cxx = env::var("CXX").unwrap_or_else(|_| "cl.exe".into());
        let cc = env::var("CC").unwrap_or_else(|_| "cl.exe".into());
        let mut compile = Command::new(&cxx);
        compile.args(["/nologo", "/std:c++17", "/O2", "/EHsc-", "/c", "native\\spp_native.cpp"]);
        compile.arg(format!("/Fo{}", obj.display()));
        run(compile, "C++ compiler");

        // C11 core (hashing / low-level primitives)
        let mut ccompile = Command::new(&cc);
        ccompile.args(["/nologo", "/O2", "/c", "native\\spp_native.c"]);
        ccompile.arg(format!("/Fo{}", cobj.display()));
        run(ccompile, "C compiler");

        let librarian = env::var("AR").unwrap_or_else(|_| "lib.exe".into());
        let mut libcmd = Command::new(&librarian);
        libcmd.args(["/nologo"])
              .arg(format!("/OUT:{}", lib.display()))
              .arg(&obj)
              .arg(&cobj);
        run(libcmd, "MSVC librarian");
    } else {
        let obj = out.join("spp_native.o");
        let cobj = out.join("spp_native_c.o");
        let lib = out.join("libspp_native.a");
        let cxx = env::var("CXX").unwrap_or_else(|_| "c++".into());
        let cc = env::var("CC").unwrap_or_else(|_| "cc".into());
        let ar = env::var("AR").unwrap_or_else(|_| "ar".into());

        let mut compile = Command::new(&cxx);
        compile.args(["-std=c++17", "-O3", "-fno-exceptions", "-fno-rtti", "-c", "native/spp_native.cpp", "-o"])
               .arg(&obj);
        run(compile, "C++ compiler");

        // C11 core (hashing / low-level primitives)
        let mut ccompile = Command::new(&cc);
        ccompile.args(["-std=c11", "-O3", "-c", "native/spp_native.c", "-o"]).arg(&cobj);
        run(ccompile, "C compiler");

        let mut libcmd = Command::new(&ar);
        libcmd.args(["crus"]).arg(&lib).arg(&obj).arg(&cobj);
        run(libcmd, "static librarian");
    }

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=spp_native");
}
