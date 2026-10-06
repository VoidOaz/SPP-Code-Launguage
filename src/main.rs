mod ast;
mod lexer;
mod native;
mod parser;
mod runtime;
mod stdlib;
mod token;
mod value;
mod vm;

use ast::{Class, Function, Program};
use lexer::Lexer;
use parser::Parser;
use runtime::{run_program, Interpreter};
use stdlib::is_builtin_module;
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process,
    time::Instant,
};

const VERSION: &str = "2.0.0-beta.1";
const EMBED_MAGIC: &[u8] = b"\nSPP_EXECUTABLE_PAYLOAD_V2\n";

fn usage() {
    println!("SPP {VERSION} — Scriptable Performance Programming");
    println!();
    println!("Usage:");
    println!("  spp run <file.spp>                         Run an SPP file");
    println!("  spp build <file.spp>                       Package as build/<file>.exe");
    println!("  spp build <output.exe> <file.spp>          Package to an explicit path");
    println!("  spp check <file.spp>                       Validate source and imports");
    println!("  spp tokens <file.spp>                      Show lexer tokens");
    println!("  spp ast <file.spp>                         Show parser AST");
    println!("  spp benchmark <file.spp> [runs]            Benchmark optimized VM when possible");
    println!("  spp modules                                List built-in modules");
    println!("  spp --version");
    println!();
    println!("Engine: Rust runtime + C++17 native math (SSE2 batch ops) + C11 core (native hashing) + optimized bytecode VM.");
    println!("Native ABI: v{}.", crate::native::native_version_string());
}

fn main() {
    if env::args().len() == 1 {
        if let Some(sources) = embedded_sources() {
            match parse_embedded_program(&sources).and_then(|program| run_program(&program).map(|_| ())) {
                Ok(()) => return,
                Err(error) => fail(error),
            }
        }
        usage();
        return;
    }

    let args: Vec<String> = env::args().skip(1).collect();
    let command = args[0].as_str();
    let result = match command {
        "run" if args.len() == 2 => run_path(Path::new(&args[1])),
        "build" if args.len() == 2 => build_path_args(&args[1], None),
        "build" if args.len() == 3 => build_path_args(&args[1], Some(&args[2])),
        "check" if args.len() == 2 => check_path(Path::new(&args[1])),
        "tokens" if args.len() == 2 => tokens_path(Path::new(&args[1])),
        "ast" if args.len() == 2 => ast_path(Path::new(&args[1])),
        "benchmark" if (2..=3).contains(&args.len()) => benchmark_path(
            Path::new(&args[1]),
            args.get(2).and_then(|v| v.parse::<usize>().ok()).unwrap_or(5),
        ),
        "modules" if args.len() == 1 => {
            print_modules();
            Ok(())
        }
        "--version" | "-V" => {
            println!("SPP {VERSION}");
            Ok(())
        }
        "help" | "--help" | "-h" => {
            usage();
            Ok(())
        }
        _ => {
            usage();
            Err("invalid command or arguments".into())
        }
    };
    if let Err(error) = result {
        fail(error);
    }
}

fn run_path(path: &Path) -> Result<(), String> {
    let program = load_with_imports(path)?;
    run_program(&program).map(|_| ()).map_err(|e| add_source(e, path))
}

fn check_path(path: &Path) -> Result<(), String> {
    let program = load_with_imports(path)?;
    println!(
        "OK: {} function(s), {} class(es), {} import(s).",
        program.functions.len(),
        program.classes.len(),
        program.imports.len()
    );
    Ok(())
}

fn tokens_path(path: &Path) -> Result<(), String> {
    let source = fs::read_to_string(path)
        .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    for token in Lexer::new(&source).tokenize()? {
        println!("{:?} @ {}:{}", token.kind, token.line, token.column);
    }
    Ok(())
}

fn ast_path(path: &Path) -> Result<(), String> {
    let program = load_with_imports(path)?;
    println!("{program:#?}");
    Ok(())
}

fn benchmark_path(path: &Path, runs: usize) -> Result<(), String> {
    let program = load_with_imports(path)?;
    let runs = runs.max(1);
    let bytecode = vm::BytecodeProgram::compile(&program).ok();
    let mut samples = Vec::with_capacity(runs);

    for _ in 0..runs {
        let start = Instant::now();
        if let Some(ref vm) = bytecode {
            vm.run().map_err(|e| add_source(e, path))?;
        } else {
            let mut interpreter = Interpreter::new(&program);
            interpreter.run().map_err(|e| add_source(e, path))?;
        }
        samples.push(start.elapsed());
    }

    let total: f64 = samples.iter().map(|d| d.as_secs_f64()).sum();
    let avg = total / runs as f64;
    let min = samples.iter().min().unwrap();
    let max = samples.iter().max().unwrap();
    println!(
        "SPP benchmark — {}",
        if bytecode.is_some() {
            "optimized bytecode VM"
        } else {
            "reference AST interpreter"
        }
    );
    println!("runs: {runs}");
    println!("avg: {:.3} ms", avg * 1000.0);
    println!("min: {:.3} ms", min.as_secs_f64() * 1000.0);
    println!("max: {:.3} ms", max.as_secs_f64() * 1000.0);
    Ok(())
}

fn execute_source(label: &str, source: &str) -> Result<(), String> {
    let program = parse_program(label, source)?;
    run_program(&program)
        .map(|_| ())
        .map_err(|e| add_source(e, Path::new(label)))
}

fn parse_program(label: &str, source: &str) -> Result<Program, String> {
    let tokens = Lexer::new(source).tokenize()?;
    Parser::new(tokens)
        .parse()
        .map_err(|e| format!("{label}: {e}"))
}

fn load_with_imports(path: &Path) -> Result<Program, String> {
    let root = canonicalize_friendly(path)?;
    let mut visited = std::collections::HashSet::new();
    let mut merged = Program {
        package: None,
        imports: Vec::new(),
        functions: Vec::new(),
        classes: Vec::new(),
    };
    load_module(&root, &mut visited, &mut merged, true)?;
    if merged.functions.iter().filter(|f| f.name == "main").count() != 1 {
        return Err(format!(
            "SPP:{}: program must contain exactly one fn main() function",
            path.display()
        ));
    }
    Ok(merged)
}

fn load_module(
    path: &Path,
    visited: &mut std::collections::HashSet<PathBuf>,
    merged: &mut Program,
    is_root: bool,
) -> Result<(), String> {
    let path = canonicalize_friendly(path)?;
    if !visited.insert(path.clone()) {
        return Ok(());
    }
    let source = fs::read_to_string(&path)
        .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let program = parse_program(&path.display().to_string(), &source)?;
    if is_root {
        merged.package = program.package.clone();
    }
    merged.imports.extend(program.imports.clone());

    for f in &program.functions {
        if !is_root && f.name == "main" {
            continue;
        }
        if f.method_of.is_none() {
            if merged.functions.iter().any(|x: &Function| x.name == f.name) {
                return Err(format!(
                    "SPP:{}: duplicate function '{}'",
                    path.display(),
                    f.name
                ));
            }
            merged.functions.push(f.clone());
        }
    }

    for c in &program.classes {
        if merged.classes.iter().any(|x: &Class| x.name == c.name) {
            return Err(format!(
                "SPP:{}: duplicate class '{}'",
                path.display(),
                c.name
            ));
        }
        merged.classes.push(c.clone());
    }

    for import in &program.imports {
        if is_builtin_module(import) {
            continue;
        }
        let mut child = path.parent().unwrap_or_else(|| Path::new(".")).join(import);
        if child.extension().is_none() {
            child.set_extension("spp");
        }
        load_module(&child, visited, merged, false)?;
    }

    if is_root {
        for c in &mut merged.classes {
            for method in &mut c.methods {
                if method.method_of.is_none() {
                    method.method_of = Some(c.name.clone());
                }
            }
        }
    }
    Ok(())
}

fn canonicalize_friendly(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path)
        .map_err(|e| format!("Could not resolve {}: {e}", path.display()))
}

fn build_path_args(a: &str, b: Option<&str>) -> Result<(), String> {
    let (output, source) = match b {
        Some(second)
            if a.to_ascii_lowercase().ends_with(".spp")
                && second.to_ascii_lowercase().ends_with(".exe") =>
        {
            (PathBuf::from(second), PathBuf::from(a))
        }
        Some(second) if a == ".exe" && second.to_ascii_lowercase().ends_with(".spp") => {
            let stem = Path::new(second)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("app");
            (PathBuf::from(format!("{stem}.exe")), PathBuf::from(second))
        }
        Some(second) => (PathBuf::from(a), PathBuf::from(second)),
        None => {
            let source = PathBuf::from(a);
            let stem = source
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("app");
            (PathBuf::from(format!("{stem}.exe")), source)
        }
    };
    build_executable(&source, &output)
}

fn print_modules() {
    println!("SPP built-in modules:");
    println!("  Math   — native numerical functions, constants and SSE2 batch ops (sum/dot)");
    println!("  Time   — wall-clock and monotonic timing");
    println!("  Random — fast pseudo-random helpers");
    println!("  Spp3D  — native 3D vector/math primitives");
    println!("  File   — file operations");
    println!("  OS     — process/environment helpers");
}

fn build_executable(source_path: &Path, output_name: &Path) -> Result<(), String> {
    let program = load_with_imports(source_path)?;
    let sources = collect_source_bundle(source_path)?;
    let current_exe = env::current_exe()
        .map_err(|e| format!("Could not locate spp executable: {e}"))?;
    let mut base = fs::read(&current_exe)
        .map_err(|e| format!("Could not read {}: {e}", current_exe.display()))?;

    base.extend_from_slice(EMBED_MAGIC);
    base.extend_from_slice(&(sources.len() as u64).to_le_bytes());
    for source in &sources {
        base.extend_from_slice(&(source.len() as u64).to_le_bytes());
        base.extend_from_slice(source.as_bytes());
    }

    let output = if output_name
        .parent()
        .is_some_and(|p| !p.as_os_str().is_empty())
    {
        output_name.to_path_buf()
    } else {
        PathBuf::from("build").join(output_name)
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create {}: {e}", parent.display()))?;
    }
    let mut file = fs::File::create(&output)
        .map_err(|e| format!("Could not create {}: {e}", output.display()))?;
    file.write_all(&base)
        .map_err(|e| format!("Could not write {}: {e}", output.display()))?;
    file.flush()
        .map_err(|e| format!("Could not finalize {}: {e}", output.display()))?;
    drop(program); // semantic validation was the purpose of load_with_imports above.

    println!("Built: {}", output.display());
    println!("Packaging mode: native SPP executable with embedded source bundle + optimized runtime.");
    Ok(())
}

fn collect_source_bundle(root: &Path) -> Result<Vec<String>, String> {
    let mut visited = std::collections::HashSet::new();
    let mut sources = Vec::new();
    collect_source_bundle_inner(
        &canonicalize_friendly(root)?,
        &mut visited,
        &mut sources,
    )?;
    Ok(sources)
}

fn collect_source_bundle_inner(
    path: &Path,
    visited: &mut std::collections::HashSet<PathBuf>,
    sources: &mut Vec<String>,
) -> Result<(), String> {
    let path = canonicalize_friendly(path)?;
    if !visited.insert(path.clone()) {
        return Ok(());
    }
    let source = fs::read_to_string(&path)
        .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    let program = parse_program(&path.display().to_string(), &source)?;
    sources.push(source);
    for import in &program.imports {
        if is_builtin_module(import) {
            continue;
        }
        let mut child = path.parent().unwrap_or_else(|| Path::new(".")).join(import);
        if child.extension().is_none() {
            child.set_extension("spp");
        }
        collect_source_bundle_inner(&child, visited, sources)?;
    }
    Ok(())
}

fn embedded_sources() -> Option<Vec<String>> {
    let exe = env::current_exe().ok()?;
    let bytes = fs::read(exe).ok()?;
    let pos = bytes.windows(EMBED_MAGIC.len()).rposition(|w| w == EMBED_MAGIC)?;
    let mut cursor = pos + EMBED_MAGIC.len();
    let count = read_u64(&bytes, &mut cursor)? as usize;
    let mut sources = Vec::with_capacity(count);

    for _ in 0..count {
        let len = read_u64(&bytes, &mut cursor)? as usize;
        let end = cursor.checked_add(len)?;
        if end > bytes.len() {
            return None;
        }
        sources.push(String::from_utf8(bytes[cursor..end].to_vec()).ok()?);
        cursor = end;
    }
    Some(sources)
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> Option<u64> {
    let end = cursor.checked_add(8)?;
    if end > bytes.len() {
        return None;
    }
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[*cursor..end]);
    *cursor = end;
    Some(u64::from_le_bytes(raw))
}

fn parse_embedded_program(sources: &[String]) -> Result<Program, String> {
    let mut merged = Program {
        package: None,
        imports: Vec::new(),
        functions: Vec::new(),
        classes: Vec::new(),
    };

    for (index, source) in sources.iter().enumerate() {
        let program = parse_program(&format!("<embedded:{index}>"), source)?;
        if index == 0 {
            merged.package = program.package.clone();
        }
        merged.imports.extend(program.imports.clone());

        for function in &program.functions {
            if index != 0 && function.name == "main" {
                continue;
            }
            if function.method_of.is_none() {
                if merged.functions.iter().any(|f| f.name == function.name) {
                    return Err(format!(
                        "SPP embedded bundle: duplicate function '{}'",
                        function.name
                    ));
                }
                merged.functions.push(function.clone());
            }
        }
        for class in &program.classes {
            if merged.classes.iter().any(|c| c.name == class.name) {
                return Err(format!(
                    "SPP embedded bundle: duplicate class '{}'",
                    class.name
                ));
            }
            merged.classes.push(class.clone());
        }
    }

    for class in &mut merged.classes {
        for method in &mut class.methods {
            if method.method_of.is_none() {
                method.method_of = Some(class.name.clone());
            }
        }
    }

    if merged.functions.iter().filter(|f| f.name == "main").count() != 1 {
        return Err("SPP embedded bundle: program must contain exactly one fn main() function".into());
    }
    Ok(merged)
}

fn add_source(error: String, path: &Path) -> String {
    if error.starts_with("SPP:") || path.to_string_lossy() == "<embedded>" {
        error
    } else {
        format!("SPP:{}: {error}", path.display())
    }
}

fn fail(error: String) -> ! {
    eprintln!("SPP error: {error}");
    process::exit(1);
}

#[allow(dead_code)]
fn _compile_source_entry(label: &str, source: &str) -> Result<(), String> {
    execute_source(label, source)
}
