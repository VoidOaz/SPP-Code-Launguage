//! Optimized stack-based bytecode VM for SPP (engine v2.1).
//!
//! This is the *primary* execution engine, not a narrow fast path:
//! - multiple functions with real call frames (parameters, locals, returns);
//! - int, float, bool, null and string values (concatenation + comparison);
//! - shared constant pool with compile-time constant folding;
//! - `for (int i = 0; i < n; i++)` compiles to a native integer counter loop
//!   (`CounterStep`) — no per-iteration value boxing on the counter path;
//! - programs using features outside this VM's scope (classes, arrays, modules)
//!   fall back to the reference AST interpreter automatically, so semantics
//!   never regress.

use crate::ast::{AssignOp, AssignTarget, BinaryOp, Expr, Program, Stmt, Type, UnaryOp};
use std::collections::HashMap;
use std::rc::Rc;

pub const VERSION: &str = "2.1.0";

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum V {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(Rc<String>),
    Null,
}

impl V {
    #[inline]
    fn truthy(&self) -> bool {
        match self {
            Self::Bool(v) => *v,
            Self::Int(v) => *v != 0,
            Self::Float(v) => *v != 0.0,
            Self::Str(v) => !v.is_empty(),
            Self::Null => false,
        }
    }

    #[inline]
    fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(v) => Some(*v as f64),
            Self::Float(v) => Some(*v),
            Self::Bool(v) => Some(if *v { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
}

impl std::fmt::Display for V {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            V::Int(v) => write!(f, "{v}"),
            V::Float(v) => write!(f, "{v}"),
            V::Bool(v) => write!(f, "{v}"),
            V::Str(v) => write!(f, "{v}"),
            V::Null => write!(f, "null"),
        }
    }
}

// ---------------------------------------------------------------------------
// Instructions
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub enum Op {
    Const(u32),
    Load(u16),
    Store(u16),
    Pop,
    Dup,
    Add, Sub, Mul, Div, Mod,
    Eq, Ne, Lt, Le, Gt, Ge,
    Neg, Not,
    Jump(u32),
    JumpIfFalse(u32),
    JumpIfTrue(u32),
    Call { function: u32, arity: u16 },
    Return,
    Print,
    Assert,
    /// Native counter loop step used by the classic C-style `for` shape:
    /// `locals[slot] += delta; if locals[slot] < locals[limit_slot] { jump(test_ip) }`
    CounterStep { slot: u16, limit_slot: u16, delta: i32, test_ip: u32 },
}

pub struct BytecodeProgram {
    codes: Vec<Rc<Vec<Op>>>,
    frames_meta: Vec<(usize, usize)>, // (slots, arity)
    names: Vec<String>,
    main: usize,
    constants: Vec<V>,
}

// ---------------------------------------------------------------------------
// Constant interning (per compile batch, thread-safe via RefCell locals)
// ---------------------------------------------------------------------------

#[derive(PartialEq, Eq, Hash)]
enum ConstKey {
    Int(i64),
    FloatBits(u64),
    Bool(bool),
    Null,
    Str(String),
}

impl ConstKey {
    fn of(v: &V) -> Self {
        match v {
            V::Int(i) => Self::Int(*i),
            V::Float(f) => Self::FloatBits(f.to_bits()),
            V::Bool(b) => Self::Bool(*b),
            V::Null => Self::Null,
            V::Str(s) => Self::Str(s.as_ref().clone()),
        }
    }
}

struct Pool {
    constants: Vec<V>,
    index: HashMap<ConstKey, u32>,
}

impl Pool {
    fn new() -> Self {
        let mut pool = Self { constants: Vec::with_capacity(64), index: HashMap::new() };
        // Index 0 is always Null (implicit returns / uninitialized locals).
        pool.intern(V::Null);
        pool
    }

    fn intern(&mut self, v: V) -> u32 {
        let key = ConstKey::of(&v);
        if let Some(&id) = self.index.get(&key) { return id; }
        let id = self.constants.len() as u32;
        self.constants.push(v);
        self.index.insert(key, id);
        id
    }
}

// ---------------------------------------------------------------------------
// Compiler
// ---------------------------------------------------------------------------

const MAX_CODE_LEN: usize = 1 << 24;
const LOCAL_LIMIT: usize = 8192;

struct FuncCompiler<'p> {
    code: Vec<Op>,
    scopes: Vec<HashMap<String, u16>>,
    next_slot: u16,
    loops: Vec<LoopTargets>,
    callsites: Vec<(usize, String)>,
    pool: &'p mut Pool,
}

#[derive(Default)]
struct LoopTargets {
    breaks: Vec<usize>,
    continue_patches: Vec<usize>,
}

impl BytecodeProgram {
    /// Compile every top-level function. Returns `Err` when the program uses
    /// constructs outside the VM's scope so callers can fall back to the
    /// reference interpreter.
    pub fn compile(program: &Program) -> Result<Self, String> {
        if !program.classes.is_empty() {
            return Err("bytecode VM: classes are handled by the reference interpreter".into());
        }
        let main_count = program.functions.iter().filter(|f| f.name == "main").count();
        if main_count != 1 {
            return Err("bytecode VM: exactly one main() is required".into());
        }

        let mut pool = Pool::new();
        let mut codes: Vec<Rc<Vec<Op>>> = Vec::with_capacity(program.functions.len());
        let mut frames_meta = Vec::with_capacity(program.functions.len());
        let mut names = Vec::with_capacity(program.functions.len());
        let mut all_callsites = Vec::with_capacity(program.functions.len());
        let mut main = usize::MAX;

        for (index, function) in program.functions.iter().enumerate() {
            if function.name == "main" { main = index; }
            if function.params.len() > u16::MAX as usize {
                return Err(format!("bytecode VM: function '{}' has too many parameters", function.name));
            }
            for param in &function.params {
                if !matches!(param.ty, Type::Any | Type::Int | Type::Float | Type::String | Type::Bool) {
                    return Err("bytecode VM: only int/float/string/bool/any parameter types are optimized".into());
                }
            }
            let mut c = FuncCompiler::new(&mut pool);
            c.scopes[0].clear();
            for (slot, param) in function.params.iter().enumerate() {
                c.scopes[0].insert(param.name.clone(), slot as u16);
            }
            c.next_slot = function.params.len() as u16;
            c.compile_block(&function.body)?;
            c.emit(Op::Const(0))?; // implicit `return null`
            c.emit(Op::Return)?;
            names.push(function.name.clone());
            frames_meta.push((c.next_slot as usize, function.params.len()));
            all_callsites.push(std::mem::take(&mut c.callsites));
            codes.push(Rc::new(c.code));
        }

        // Link cross-function calls.
        let mut by_name: HashMap<&str, u32> = HashMap::with_capacity(names.len());
        for (index, name) in names.iter().enumerate() {
            by_name.insert(name.as_str(), index as u32);
        }
        for (fi, sites) in all_callsites.iter().enumerate() {
            for (site, name) in sites {
                let target = by_name.get(name.as_str()).copied()
                    .ok_or_else(|| format!("bytecode VM: call to unknown function '{name}'"))?;
                let code = Rc::get_mut(&mut codes[fi])
                    .ok_or_else(|| "bytecode VM: internal linking error".to_string())?;
                match &mut code[*site] {
                    Op::Call { function, .. } => *function = target,
                    _ => return Err("bytecode VM: internal callsite error".into()),
                }
            }
        }

        Ok(Self { codes, frames_meta, names, main, constants: pool.constants })
    }

    /// Execute `main()` to completion. Printing happens through `Op::Print`.
    pub fn run(&self) -> Result<(), String> {
        let mut stack: Vec<V> = Vec::with_capacity(256);
        let mut locals: Vec<V> = Vec::with_capacity(4096);
        let mut frames: Vec<Frame> = Vec::with_capacity(64);

        let entry_slots = self.frames_meta[self.main].0;
        frames.push(Frame { function: self.main, return_ip: usize::MAX, base: 0 });
        locals.resize(entry_slots, V::Null);
        let mut code: &[Op] = &self.codes[self.main];
        let mut ip = 0usize;

        loop {
            let Some(op) = code.get(ip).copied() else {
                return Err("bytecode VM: instruction pointer out of bounds".into());
            };
            ip += 1;
            match op {
                Op::Const(idx) => stack.push(self.constants[idx as usize].clone()),
                Op::Load(slot) => {
                    let v = locals[slot as usize].clone();
                    stack.push(v);
                }
                Op::Store(slot) => {
                    let v = stack.pop().ok_or_else(|| "bytecode VM: stack underflow on store".to_string())?;
                    locals[slot as usize] = v;
                }
                Op::Pop => { stack.pop().ok_or_else(|| "bytecode VM: stack underflow on pop".to_string())?; }
                Op::Dup => {
                    let v = stack.last().cloned().ok_or_else(|| "bytecode VM: stack underflow on dup".to_string())?;
                    stack.push(v);
                }
                Op::Add => {
                    let (a, b) = pop2(&mut stack)?;
                    stack.push(add_values(a, b)?);
                }
                Op::Sub | Op::Mul | Op::Div | Op::Mod => {
                    let (a, b) = pop2(&mut stack)?;
                    let m = match op { Op::Sub => MathOp::Sub, Op::Mul => MathOp::Mul, Op::Div => MathOp::Div, _ => MathOp::Mod };
                    stack.push(math_values(a, b, m)?);
                }
                Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => {
                    let (a, b) = pop2(&mut stack)?;
                    let result = compare_values(a, b, op)?;
                    stack.push(V::Bool(result));
                }
                Op::Neg => {
                    let v = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
                    stack.push(match v {
                        V::Int(x) => V::Int(x.checked_neg().ok_or_else(|| "SPP runtime: integer overflow".to_string())?),
                        V::Float(x) => V::Float(-x),
                        _ => return Err("SPP runtime: unary '-' needs a number".into()),
                    });
                }
                Op::Not => {
                    let v = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
                    stack.push(V::Bool(!v.truthy()));
                }
                Op::Jump(target) => { ip = target as usize; }
                Op::JumpIfFalse(target) => {
                    let v = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
                    if !v.truthy() { ip = target as usize; }
                }
                Op::JumpIfTrue(target) => {
                    let v = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
                    if v.truthy() { ip = target as usize; }
                }
                Op::Print => {
                    let v = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
                    println!("{v}");
                }
                Op::Assert => {
                    let v = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
                    if !v.truthy() { return Err("SPP runtime: assertion failed".into()); }
                }
                Op::Call { function, arity } => {
                    let callee = function as usize;
                    let (slots, _) = self.frames_meta[callee];
                    let n = arity as usize;
                    if frames.len() >= 512 {
                        return Err(format!("SPP runtime: maximum call depth (512) exceeded in '{}'", self.names[callee]));
                    }
                    if stack.len() < n { return Err("bytecode VM: stack underflow on call".into()); }
                    let base = locals.len();
                    // Arguments were pushed left-to-right; move them into the new frame.
                    let args_start = stack.len() - n;
                    for i in 0..n {
                        let v = std::mem::replace(&mut stack[args_start + i], V::Null);
                        locals.push(v);
                    }
                    stack.truncate(args_start);
                    locals.resize(base + slots, V::Null);
                    frames.push(Frame { function: callee, return_ip: ip, base });
                    code = &self.codes[callee];
                    ip = 0;
                }
                Op::CounterStep { slot, limit_slot, delta, test_ip } => {
                    let counter = match locals[slot as usize] { V::Int(x) => x, _ => return Err("bytecode VM: counter slot corrupted".into()) };
                    let limit = match locals[limit_slot as usize] { V::Int(x) => x, _ => return Err("bytecode VM: limit slot corrupted".into()) };
                    let next = counter.wrapping_add(delta as i64);
                    locals[slot as usize] = V::Int(next);
                    if next < limit { ip = test_ip as usize; }
                }
                Op::Return => {
                    let value = stack.pop().unwrap_or(V::Null);
                    let frame = frames.pop().ok_or_else(|| "bytecode VM: return without frame".to_string())?;
                    locals.truncate(frame.base);
                    if frame.return_ip == usize::MAX {
                        // Returned from main: program complete.
                        return Ok(());
                    }
                    ip = frame.return_ip;
                    let caller_function = frames.last().map(|f| f.function).unwrap_or(self.main);
                    code = &self.codes[caller_function];
                    stack.push(value);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Frame { function: usize, return_ip: usize, base: usize }

fn pop2(stack: &mut Vec<V>) -> Result<(V, V), String> {
    let b = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
    let a = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
    Ok((a, b))
}

// ---------------------------------------------------------------------------
// Runtime value operations
// ---------------------------------------------------------------------------

fn add_values(a: V, b: V) -> Result<V, String> {
    match (&a, &b) {
        (V::Int(x), V::Int(y)) => Ok(V::Int(x.checked_add(*y).ok_or_else(|| "SPP runtime: integer overflow".to_string())?)),
        (V::Str(_), _) | (_, V::Str(_)) => Ok(V::Str(Rc::new(format!("{a}{b}")))),
        _ => {
            let x = a.as_f64().ok_or_else(|| "SPP runtime: cannot add these values".to_string())?;
            let y = b.as_f64().ok_or_else(|| "SPP runtime: cannot add these values".to_string())?;
            Ok(V::Float(x + y))
        }
    }
}

#[derive(Clone, Copy)]
enum MathOp { Sub, Mul, Div, Mod }

fn math_values(a: V, b: V, op: MathOp) -> Result<V, String> {
    match (a, b) {
        (V::Int(x), V::Int(y)) => {
            let v = match op {
                MathOp::Sub => x.checked_sub(y),
                MathOp::Mul => x.checked_mul(y),
                MathOp::Div => { if y == 0 { return Err("SPP runtime: division by zero".into()); } x.checked_div(y) }
                MathOp::Mod => { if y == 0 { return Err("SPP runtime: modulo by zero".into()); } x.checked_rem(y) }
            }.ok_or_else(|| "SPP runtime: integer overflow".to_string())?;
            Ok(V::Int(v))
        }
        (a, b) => {
            let x = a.as_f64().ok_or_else(|| "SPP runtime: expected numbers".to_string())?;
            let y = b.as_f64().ok_or_else(|| "SPP runtime: expected numbers".to_string())?;
            Ok(V::Float(match op {
                MathOp::Sub => x - y,
                MathOp::Mul => x * y,
                MathOp::Div => { if y == 0.0 { return Err("SPP runtime: division by zero".into()); } x / y }
                MathOp::Mod => x % y,
            }))
        }
    }
}

fn compare_values(a: V, b: V, op: Op) -> Result<bool, String> {
    match (a, b) {
        (V::Int(x), V::Int(y)) => Ok(cmp_i64(x, y, op)),
        (V::Float(x), V::Float(y)) => Ok(cmp_f64(x, y, op)),
        (V::Int(x), V::Float(y)) => Ok(cmp_f64(x as f64, y, op)),
        (V::Float(x), V::Int(y)) => Ok(cmp_f64(x, y as f64, op)),
        (V::Str(x), V::Str(y)) => Ok(match op {
            Op::Eq => x == y, Op::Ne => x != y, Op::Lt => *x < *y, Op::Le => *x <= *y,
            Op::Gt => *x > *y, Op::Ge => *x >= *y, _ => return Err("SPP runtime: bad string comparison".into()),
        }),
        (V::Bool(x), V::Bool(y)) => match op { Op::Eq => Ok(x == y), Op::Ne => Ok(x != y), _ => Err("SPP runtime: booleans only support == and !=".into()) },
        (V::Null, V::Null) => match op { Op::Eq => Ok(true), Op::Ne => Ok(false), _ => Err("SPP runtime: null only supports == and !=".into()) },
        (V::Int(x), V::Bool(y)) => match op { Op::Eq => Ok((x != 0) == y), Op::Ne => Ok((x != 0) != y), _ => Err("SPP runtime: booleans only support == and !=".into()) },
        (V::Bool(y), V::Int(x)) => match op { Op::Eq => Ok((x != 0) == y), Op::Ne => Ok((x != 0) != y), _ => Err("SPP runtime: booleans only support == and !=".into()) },
        (V::Null, _) | (_, V::Null) => match op { Op::Eq => Ok(false), Op::Ne => Ok(true), _ => Err("SPP runtime: null only supports == and !=".into()) },
        _ => Err("SPP runtime: values are not comparable".into()),
    }
}

#[inline]
fn cmp_i64(x: i64, y: i64, op: Op) -> bool {
    match op { Op::Eq => x == y, Op::Ne => x != y, Op::Lt => x < y, Op::Le => x <= y, Op::Gt => x > y, Op::Ge => x >= y, _ => false }
}
#[inline]
fn cmp_f64(x: f64, y: f64, op: Op) -> bool {
    match op { Op::Eq => x == y, Op::Ne => x != y, Op::Lt => x < y, Op::Le => x <= y, Op::Gt => x > y, Op::Ge => x >= y, _ => false }
}

// ---------------------------------------------------------------------------
// Compiler implementation
// ---------------------------------------------------------------------------

impl<'p> FuncCompiler<'p> {
    fn new(pool: &'p mut Pool) -> Self {
        Self {
            code: Vec::with_capacity(128),
            scopes: vec![HashMap::new()],
            next_slot: 0,
            loops: Vec::new(),
            callsites: Vec::new(),
            pool,
        }
    }

    fn emit(&mut self, op: Op) -> Result<(), String> {
        if self.code.len() >= MAX_CODE_LEN { return Err("bytecode VM: function body too large".into()); }
        self.code.push(op);
        Ok(())
    }

    fn declare(&mut self, name: &str) -> Result<u16, String> {
        if self.next_slot as usize >= LOCAL_LIMIT.min(u16::MAX as usize) {
            return Err("bytecode VM: function has too many locals".into());
        }
        let slot = self.next_slot;
        self.next_slot += 1;
        self.scopes.last_mut().unwrap().insert(name.to_string(), slot);
        Ok(slot)
    }

    fn resolve(&self, name: &str) -> Option<u16> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name).copied())
    }

    fn compile_block(&mut self, body: &[Stmt]) -> Result<(), String> {
        self.scopes.push(HashMap::new());
        for stmt in body { self.compile_stmt(stmt)?; }
        self.scopes.pop();
        Ok(())
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::VarDecl { ty, name, value } => {
                if matches!(ty, Type::Array(_) | Type::Named(_) | Type::Void) {
                    return Err("bytecode VM: array/object declarations use the reference interpreter".into());
                }
                let slot = self.declare(name)?;
                match value {
                    Some(expr) => self.compile_expr(expr)?,
                    None => self.emit(Op::Const(0))?,
                }
                self.emit(Op::Store(slot))?;
            }
            Stmt::Assign { target, op, value } => {
                let AssignTarget::Variable(name) = target else {
                    return Err("bytecode VM: indexed/member assignment uses the reference interpreter".into());
                };
                let Some(slot) = self.resolve(name) else {
                    return Err(format!("bytecode VM: unknown variable '{name}'"));
                };
                if matches!(op, AssignOp::Set) {
                    self.compile_expr(value)?;
                } else {
                    self.emit(Op::Load(slot))?;
                    self.compile_expr(value)?;
                    self.emit(match op {
                        AssignOp::Add => Op::Add, AssignOp::Sub => Op::Sub,
                        AssignOp::Mul => Op::Mul, AssignOp::Div => Op::Div, AssignOp::Set => unreachable!(),
                    })?;
                }
                self.emit(Op::Store(slot))?;
            }
            Stmt::Expr(expr) => {
                self.compile_expr(expr)?;
                // Call expressions leave exactly one value (the result); drop it.
                self.emit(Op::Pop)?;
            }
            Stmt::If { branches, else_body } => self.compile_if(branches, else_body.as_deref())?,
            Stmt::While { condition, body } => self.compile_while(condition, body)?,
            Stmt::ForC { init, condition, step, body } => {
                self.compile_for(init.as_deref(), condition.as_ref(), step.as_deref(), body)?
            }
            Stmt::Break => {
                let idx = self.loops.len().checked_sub(1).ok_or_else(|| "SPP: 'break' used outside a loop".to_string())?;
                let patch = self.code.len();
                self.emit(Op::Jump(u32::MAX))?;
                self.loops[idx].breaks.push(patch);
            }
            Stmt::Continue => {
                let idx = self.loops.len().checked_sub(1).ok_or_else(|| "SPP: 'continue' used outside a loop".to_string())?;
                let patch = self.code.len();
                self.emit(Op::Jump(u32::MAX))?;
                self.loops[idx].continue_patches.push(patch);
            }
            Stmt::Return(expr) => {
                match expr { Some(expr) => self.compile_expr(expr)?, None => self.emit(Op::Const(0))? }
                self.emit(Op::Return)?;
            }
            Stmt::ForEach { .. } => return Err("bytecode VM: foreach uses the reference interpreter".into()),
        }
        Ok(())
    }

    fn compile_if(&mut self, branches: &[(Expr, Vec<Stmt>)], else_body: Option<&[Stmt]>) -> Result<(), String> {
        let mut end_jumps = Vec::new();
        for (cond, body) in branches {
            self.compile_expr(cond)?;
            let jf = self.code.len();
            self.emit(Op::JumpIfFalse(u32::MAX))?;
            self.compile_block(body)?;
            let je = self.code.len();
            self.emit(Op::Jump(u32::MAX))?;
            end_jumps.push(je);
            let next = self.code.len() as u32;
            self.code[jf] = Op::JumpIfFalse(next);
        }
        if let Some(body) = else_body { self.compile_block(body)?; }
        let end = self.code.len() as u32;
        for j in end_jumps { self.code[j] = Op::Jump(end); }
        Ok(())
    }

    fn compile_while(&mut self, condition: &Expr, body: &[Stmt]) -> Result<(), String> {
        let cond = self.code.len() as u32;
        self.compile_expr(condition)?;
        let jf = self.code.len();
        self.emit(Op::JumpIfFalse(u32::MAX))?;
        self.loops.push(LoopTargets::default());
        self.compile_block(body)?;
        let after_body = self.code.len() as u32;
        let patches = std::mem::take(&mut self.loops.last_mut().unwrap().continue_patches);
        for p in patches { self.code[p] = Op::Jump(after_body); }
        self.emit(Op::Jump(cond))?;
        let end = self.code.len() as u32;
        self.code[jf] = Op::JumpIfFalse(end);
        let info = self.loops.pop().unwrap();
        for p in info.breaks { self.code[p] = Op::Jump(end); }
        Ok(())
    }

    fn compile_for(&mut self, init: Option<&Stmt>, condition: Option<&Expr>, step: Option<&Stmt>, body: &[Stmt]) -> Result<(), String> {
        // ---- Fast path: for (int i = K; i < LIMIT; i++) --------------------
        if let (
            Some(Stmt::VarDecl { ty, name, value: Some(Expr::Int(start_val)) }),
            Some(Expr::Binary { left, op: BinaryOp::Less, right }),
            Some(Stmt::Assign { target: AssignTarget::Variable(counter), op: AssignOp::Add, value }),
        ) = (init, condition, step)
        {
            let unit_step = matches!(value, Expr::Int(1));
            let same_var = counter == name;
            let lhs_is_counter = matches!(left.as_ref(), Expr::Variable(v) if v == name);
            let simple_ty = matches!(ty, Type::Any | Type::Int);
            if same_var && unit_step && lhs_is_counter && simple_ty {
                let counter_slot = self.declare(name)?;
                let cid = self.pool.intern(V::Int(*start_val));
                self.emit(Op::Const(cid))?;
                self.emit(Op::Store(counter_slot))?;

                // Evaluate the bound ONCE into a hidden slot.
                let limit_slot = self.declare(&format!("\u{1}limit_{counter_slot}"))?;
                self.compile_bound(right)?;
                self.emit(Op::Store(limit_slot))?;

                let cond_pos = self.code.len() as u32;
                self.emit(Op::Load(counter_slot))?;
                self.emit(Op::Load(limit_slot))?;
                self.emit(Op::Lt)?;
                let jf = self.code.len();
                self.emit(Op::JumpIfFalse(u32::MAX))?;

                self.loops.push(LoopTargets::default());
                self.compile_block(body)?;
                let after_body = self.code.len() as u32;
                let patches = std::mem::take(&mut self.loops.last_mut().unwrap().continue_patches);
                for p in patches { self.code[p] = Op::Jump(after_body); }
                self.emit(Op::CounterStep { slot: counter_slot, limit_slot, delta: 1, test_ip: cond_pos })?;
                let end = self.code.len() as u32;
                self.code[jf] = Op::JumpIfFalse(end);
                let info = self.loops.pop().unwrap();
                for p in info.breaks { self.code[p] = Op::Jump(end); }
                return Ok(());
            }
        }

        // ---- General path ---------------------------------------------------
        if let Some(init) = init { self.compile_stmt(init)?; }
        let cond = self.code.len() as u32;
        if let Some(condition) = condition {
            self.compile_expr(condition)?;
            let jf = self.code.len();
            self.emit(Op::JumpIfFalse(u32::MAX))?;
            self.loops.push(LoopTargets::default());
            self.compile_block(body)?;
            let step_start = self.code.len() as u32;
            let patches = std::mem::take(&mut self.loops.last_mut().unwrap().continue_patches);
            for p in patches { self.code[p] = Op::Jump(step_start); }
            if let Some(step) = step { self.compile_stmt(step)?; }
            self.emit(Op::Jump(cond))?;
            let end = self.code.len() as u32;
            self.code[jf] = Op::JumpIfFalse(end);
            let info = self.loops.pop().unwrap();
            for p in info.breaks { self.code[p] = Op::Jump(end); }
        } else {
            self.loops.push(LoopTargets::default());
            self.compile_block(body)?;
            let step_start = self.code.len() as u32;
            let patches = std::mem::take(&mut self.loops.last_mut().unwrap().continue_patches);
            for p in patches { self.code[p] = Op::Jump(step_start); }
            if let Some(step) = step { self.compile_stmt(step)?; }
            self.emit(Op::Jump(cond))?;
            let end = self.code.len() as u32;
            let info = self.loops.pop().unwrap();
            for p in info.breaks { self.code[p] = Op::Jump(end); }
        }
        Ok(())
    }

    /// Compile an expression, folding it immediately when it is a constant.
    fn compile_bound(&mut self, expr: &Expr) -> Result<(), String> {
        if let Some(v) = const_of(expr) {
            let id = self.pool.intern(v);
            return self.emit(Op::Const(id));
        }
        self.compile_expr(expr)
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Int(v) => { let id = self.pool.intern(V::Int(*v)); self.emit(Op::Const(id))?; }
            Expr::Float(v) => { let id = self.pool.intern(V::Float(*v)); self.emit(Op::Const(id))?; }
            Expr::Bool(v) => { let id = self.pool.intern(V::Bool(*v)); self.emit(Op::Const(id))?; }
            Expr::Null => self.emit(Op::Const(0))?,
            Expr::String(s) => {
                let id = self.pool.intern(V::Str(Rc::new(s.clone())));
                self.emit(Op::Const(id))?;
            }
            Expr::Variable(name) => {
                let Some(slot) = self.resolve(name) else {
                    return Err(format!("bytecode VM: unknown variable '{name}'"));
                };
                self.emit(Op::Load(slot))?;
            }
            Expr::Array(_) | Expr::New { .. } | Expr::Index { .. } | Expr::Member { .. } => {
                return Err("bytecode VM: arrays/objects/indexing/member access use the reference interpreter".into());
            }
            Expr::Call { callee, args } => {
                let Expr::Variable(name) = callee.as_ref() else {
                    return Err("bytecode VM: only plain named calls are optimized".into());
                };
                if (name == "print" || name == "println") && args.len() == 1 {
                    self.compile_expr(&args[0])?;
                    self.emit(Op::Print)?;
                    self.emit(Op::Const(0))?; // statement value: null
                    return Ok(());
                }
                if name == "assert" && args.len() == 1 {
                    self.compile_expr(&args[0])?;
                    self.emit(Op::Assert)?;
                    self.emit(Op::Const(0))?;
                    return Ok(());
                }
                for arg in args { self.compile_expr(arg)?; }
                let site = self.code.len();
                self.emit(Op::Call { function: u32::MAX, arity: args.len() as u16 })?;
                self.callsites.push((site, name.clone()));
            }
            Expr::Unary { op, expr } => {
                // Fold unary over constants.
                if let Some(v) = const_of(expr) {
                    if let Some(folded) = fold_unary(op, &v) {
                        let id = self.pool.intern(folded);
                        return self.emit(Op::Const(id));
                    }
                }
                self.compile_expr(expr)?;
                match op {
                    UnaryOp::Neg => self.emit(Op::Neg)?,
                    UnaryOp::Not => self.emit(Op::Not)?,
                    UnaryOp::Positive => {}
                }
            }
            Expr::Binary { left, op, right } => {
                // Compile-time constant folding.
                if let (Some(a), Some(b)) = (const_of(left), const_of(right)) {
                    if let Some(folded) = fold_binary(&a, op, &b) {
                        let id = self.pool.intern(folded);
                        return self.emit(Op::Const(id));
                    }
                }
                match op {
                    BinaryOp::And => {
                        self.compile_expr(left)?;
                        let jf = self.code.len();
                        self.emit(Op::JumpIfFalse(u32::MAX))?;
                        self.compile_expr(right)?;
                        let jmp = self.code.len();
                        self.emit(Op::Jump(u32::MAX))?;
                        let false_label = self.code.len() as u32;
                        let fid = self.pool.intern(V::Bool(false));
                        self.emit(Op::Const(fid))?;
                        let final_end = self.code.len() as u32;
                        self.code[jf] = Op::JumpIfFalse(false_label);
                        self.code[jmp] = Op::Jump(final_end);
                    }
                    BinaryOp::Or => {
                        self.compile_expr(left)?;
                        let jt = self.code.len();
                        self.emit(Op::JumpIfTrue(u32::MAX))?;
                        self.compile_expr(right)?;
                        let jmp = self.code.len();
                        self.emit(Op::Jump(u32::MAX))?;
                        let true_label = self.code.len() as u32;
                        let tid = self.pool.intern(V::Bool(true));
                        self.emit(Op::Const(tid))?;
                        let final_end = self.code.len() as u32;
                        self.code[jt] = Op::JumpIfTrue(true_label);
                        self.code[jmp] = Op::Jump(final_end);
                    }
                    _ => {
                        self.compile_expr(left)?;
                        self.compile_expr(right)?;
                        self.emit(match op {
                            BinaryOp::Add => Op::Add, BinaryOp::Sub => Op::Sub, BinaryOp::Mul => Op::Mul,
                            BinaryOp::Div => Op::Div, BinaryOp::Mod => Op::Mod,
                            BinaryOp::Equal => Op::Eq, BinaryOp::NotEqual => Op::Ne,
                            BinaryOp::Less => Op::Lt, BinaryOp::LessEqual => Op::Le,
                            BinaryOp::Greater => Op::Gt, BinaryOp::GreaterEqual => Op::Ge,
                            BinaryOp::And | BinaryOp::Or => unreachable!(),
                        })?;
                    }
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Folding helpers
// ---------------------------------------------------------------------------

fn const_of(expr: &Expr) -> Option<V> {
    match expr {
        Expr::Int(v) => Some(V::Int(*v)),
        Expr::Float(v) => Some(V::Float(*v)),
        Expr::Bool(v) => Some(V::Bool(*v)),
        Expr::String(v) => Some(V::Str(Rc::new(v.clone()))),
        Expr::Null => Some(V::Null),
        _ => None,
    }
}

fn fold_unary(op: &UnaryOp, v: &V) -> Option<V> {
    match (op, v) {
        (UnaryOp::Neg, V::Int(x)) => Some(V::Int(x.checked_neg()?)),
        (UnaryOp::Neg, V::Float(x)) => Some(V::Float(-x)),
        (UnaryOp::Not, any) => Some(V::Bool(!any.truthy())),
        (UnaryOp::Positive, V::Int(_)) | (UnaryOp::Positive, V::Float(_)) => Some(v.clone()),
        _ => None,
    }
}

fn fold_binary(a: &V, op: &BinaryOp, b: &V) -> Option<V> {
    use BinaryOp::*;
    match (a, b) {
        (V::Int(x), V::Int(y)) => Some(match op {
            Add => V::Int(x.checked_add(*y)?),
            Sub => V::Int(x.checked_sub(*y)?),
            Mul => V::Int(x.checked_mul(*y)?),
            Div => { if *y == 0 { return None; } V::Int(x.checked_div(*y)?) }
            Mod => { if *y == 0 { return None; } V::Int(x.checked_rem(*y)?) }
            Equal => V::Bool(x == y), NotEqual => V::Bool(x != y),
            Less => V::Bool(x < y), LessEqual => V::Bool(x <= y),
            Greater => V::Bool(x > y), GreaterEqual => V::Bool(x >= y),
            And | Or => return None,
        }),
        (V::Float(x), V::Float(y)) => Some(match op {
            Add => V::Float(x + y), Sub => V::Float(x - y), Mul => V::Float(x * y),
            Div => { if *y == 0.0 { return None; } V::Float(x / y) }
            Mod => V::Float(x % y),
            Equal => V::Bool(x == y), NotEqual => V::Bool(x != y),
            Less => V::Bool(x < y), LessEqual => V::Bool(x <= y),
            Greater => V::Bool(x > y), GreaterEqual => V::Bool(x >= y),
            And | Or => return None,
        }),
        (V::Int(x), V::Float(_)) => fold_binary(&V::Float(*x as f64), op, b),
        (V::Float(_), V::Int(y)) => fold_binary(a, op, &V::Float(*y as f64)),
        (V::Str(x), V::Str(y)) => match op {
            Add => Some(V::Str(Rc::new(format!("{x}{y}")))),
            Equal => Some(V::Bool(x == y)),
            NotEqual => Some(V::Bool(x != y)),
            _ => None,
        },
        (V::Bool(x), V::Bool(y)) => match op {
            Equal => Some(V::Bool(x == y)),
            NotEqual => Some(V::Bool(x != y)),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn compile(src: &str) -> Result<BytecodeProgram, String> {
        let tokens = Lexer::new(src).tokenize()?;
        let program = Parser::new(tokens).parse()?;
        BytecodeProgram::compile(&program)
    }

    #[test]
    fn loops_and_functions() {
        let vm = compile("fn add(int a, b) -> int { return a + b; } fn main() { int sum = 0; for (int i = 0; i < 10; i++) { sum += add(i, 1); } assert(sum == 55); }").unwrap();
        vm.run().unwrap();
    }

    #[test]
    fn strings_concat_and_unknown_function_error() {
        let ok = compile(r#"fn main() { string s = "a" + "b"; print(s); }"#).unwrap();
        ok.run().unwrap();
        let err = compile(r#"fn main() { nope(); }"#).unwrap_err();
        assert!(err.contains("unknown function"), "got: {err}");
    }

    #[test]
    fn constant_folding_reduces_code() {
        let vm = compile("fn main() { int x = 2 * 3 + 4; }").unwrap();
        // One Const + Store for x, then Const(null)+Return = 4 ops.
        assert_eq!(vm.codes[0].len(), 4);
    }

    #[test]
    fn recursion_works() {
        let vm = compile("fn fact(int n) -> int { if (n <= 1) { return 1; } return n * fact(n - 1); } fn main() { int v = fact(10); assert(v == 3628800); }").unwrap();
        vm.run().unwrap();
    }

    #[test]
    fn deep_recursion_reports_clean_error() {
        let vm = compile("fn boom() -> int { return boom() + 1; } fn main() { boom(); }").unwrap();
        let err = vm.run().unwrap_err();
        assert!(err.contains("maximum call depth"), "got: {err}");
    }
}
