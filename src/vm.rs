use crate::ast::{AssignTarget, BinaryOp, Expr, Program, Stmt, Type, UnaryOp};
use std::collections::HashMap;

#[derive(Clone, Debug)]
enum V {
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
}

impl V {
    fn truthy(&self) -> bool {
        match self { Self::Bool(v) => *v, Self::Int(v) => *v != 0, Self::Float(v) => *v != 0.0, Self::Null => false }
    }
}

#[derive(Clone, Debug)]
enum Op {
    Const(V), Load(usize), Store(usize), Pop,
    Add, Sub, Mul, Div, Mod,
    Eq, Ne, Lt, Le, Gt, Ge,
    Neg, Not,
    Jump(usize), JumpIfFalse(usize),
    Return,
}

pub struct BytecodeProgram {
    code: Vec<Op>,
    slots: usize,
}

pub struct BytecodeCompiler {
    code: Vec<Op>,
    scopes: Vec<HashMap<String, usize>>,
    next_slot: usize,
    loops: Vec<LoopTargets>,
}

#[derive(Default)]
struct LoopTargets {
    breaks: Vec<usize>,
    continue_target: usize,
    continue_patches: Vec<usize>,
}

impl BytecodeProgram {
    pub fn compile(program: &Program) -> Result<Self, String> {
        if program.classes.len() > 0 { return Err("bytecode VM: classes are not supported yet".into()); }
        if program.functions.len() != 1 || program.functions[0].name != "main" {
            return Err("bytecode VM: only a single main() program is optimized".into());
        }
        let main = &program.functions[0];
        for p in &main.params {
            if !matches!(p.ty, Type::Any) { /* main parameters are not part of current CLI */ }
            return Err("bytecode VM: main parameters are not supported".into());
        }
        let mut c = BytecodeCompiler::new();
        c.compile_block(&main.body)?;
        c.code.push(Op::Const(V::Null));
        c.code.push(Op::Return);
        Ok(Self { code: c.code, slots: c.next_slot })
    }

    pub fn run(&self) -> Result<(), String> {
        let mut stack: Vec<V> = Vec::with_capacity(64);
        let mut locals = vec![V::Null; self.slots];
        let mut ip = 0usize;
        loop {
            let op = self.code.get(ip).cloned().ok_or_else(|| "bytecode VM: instruction pointer out of bounds".to_string())?;
            match op {
                Op::Const(v) => stack.push(v),
                Op::Load(slot) => stack.push(locals.get(slot).cloned().ok_or_else(|| "bytecode VM: local slot out of bounds".to_string())?),
                Op::Store(slot) => {
                    let v = stack.pop().ok_or_else(|| "bytecode VM: stack underflow on store".to_string())?;
                    let local = locals.get_mut(slot).ok_or_else(|| "bytecode VM: local slot out of bounds".to_string())?;
                    *local = v;
                }
                Op::Pop => { stack.pop().ok_or_else(|| "bytecode VM: stack underflow on pop".to_string())?; }
                Op::Add => binary_numeric(&mut stack, BinaryNumeric::Add)?,
                Op::Sub => binary_numeric(&mut stack, BinaryNumeric::Sub)?,
                Op::Mul => binary_numeric(&mut stack, BinaryNumeric::Mul)?,
                Op::Div => {
                    let b = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
                    let a = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
                    match (a,b) {
                        (V::Int(x), V::Int(y)) => {
                            if y == 0 { return Err("SPP runtime: division by zero".into()); }
                            let v = x.checked_div(y).ok_or_else(|| "SPP runtime: integer division overflow".to_string())?;
                            stack.push(V::Int(v));
                        }
                        (a,b) => stack.push(V::Float(to_float(a)? / to_float(b)?)),
                    }
                }
                Op::Mod => {
                    let b = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
                    let a = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
                    match (a,b) {
                        (V::Int(x), V::Int(y)) => {
                            if y == 0 { return Err("SPP runtime: modulo by zero".into()); }
                            let v = x.checked_rem(y).ok_or_else(|| "SPP runtime: integer modulo overflow".to_string())?;
                            stack.push(V::Int(v));
                        }
                        (a,b) => stack.push(V::Float(to_float(a)? % to_float(b)?)),
                    }
                }
                Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => {
                    let b = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
                    let a = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
                    let result = compare_values(a, b, &op)?;
                    stack.push(V::Bool(result));
                }
                Op::Neg => {
                    let v = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
                    match v { V::Int(x) => stack.push(V::Int(x.checked_neg().ok_or_else(|| "SPP runtime: integer overflow".to_string())?)), V::Float(x) => stack.push(V::Float(-x)), _ => return Err("SPP runtime: unary '-' needs a number".into()) }
                }
                Op::Not => {
                    let v = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
                    stack.push(V::Bool(!v.truthy()));
                }
                Op::Jump(target) => { ip = target; continue; }
                Op::JumpIfFalse(target) => {
                    let v = stack.pop().ok_or_else(|| "bytecode VM: stack underflow on conditional jump".to_string())?;
                    if !v.truthy() { ip = target; continue; }
                }
                Op::Return => { return Ok(()); }
            }
            ip += 1;
        }
    }
}

enum BinaryNumeric { Add, Sub, Mul }

fn binary_numeric(stack: &mut Vec<V>, op: BinaryNumeric) -> Result<(), String> {
    let b = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
    let a = stack.pop().ok_or_else(|| "bytecode VM: stack underflow".to_string())?;
    match (a,b) {
        (V::Int(x), V::Int(y)) => {
            let v = match op {
                BinaryNumeric::Add => x.checked_add(y),
                BinaryNumeric::Sub => x.checked_sub(y),
                BinaryNumeric::Mul => x.checked_mul(y),
            }.ok_or_else(|| "SPP runtime: integer overflow".to_string())?;
            stack.push(V::Int(v));
        }
        (a,b) => {
            let x = to_float(a)?; let y = to_float(b)?;
            let v = match op {
                BinaryNumeric::Add => x + y,
                BinaryNumeric::Sub => x - y,
                BinaryNumeric::Mul => x * y,
            };
            stack.push(V::Float(v));
        }
    }
    Ok(())
}

fn to_float(v: V) -> Result<f64, String> {
    match v { V::Int(v) => Ok(v as f64), V::Float(v) => Ok(v), V::Bool(v) => Ok(if v { 1.0 } else { 0.0 }), _ => Err("SPP runtime: expected a number".into()) }
}

fn compare_values(a: V, b: V, op: &Op) -> Result<bool, String> {
    match (a,b) {
        (V::Int(x),V::Int(y)) => Ok(match op { Op::Eq=>x==y, Op::Ne=>x!=y, Op::Lt=>x<y, Op::Le=>x<=y, Op::Gt=>x>y, Op::Ge=>x>=y, _=>false }),
        (V::Float(x),V::Float(y)) => Ok(match op { Op::Eq=>x==y, Op::Ne=>x!=y, Op::Lt=>x<y, Op::Le=>x<=y, Op::Gt=>x>y, Op::Ge=>x>=y, _=>false }),
        (V::Int(x),V::Float(y)) => compare_values(V::Float(x as f64), V::Float(y), op),
        (V::Float(x),V::Int(y)) => compare_values(V::Float(x), V::Float(y as f64), op),
        (V::Bool(x),V::Bool(y)) => Ok(match op { Op::Eq=>x==y, Op::Ne=>x!=y, _=>return Err("SPP runtime: booleans only support == and !=".into()) }),
        (V::Null,V::Null) => match op { Op::Eq => Ok(true), Op::Ne => Ok(false), _ => Err("SPP runtime: null values are only comparable with == and !=".into()) },
        _ => Err("SPP runtime: values are not comparable".into()),
    }
}

impl BytecodeCompiler {
    fn new() -> Self { Self { code: Vec::with_capacity(128), scopes: vec![HashMap::new()], next_slot: 0, loops: Vec::new() } }

    fn compile_block(&mut self, body: &[Stmt]) -> Result<(), String> {
        self.scopes.push(HashMap::new());
        for stmt in body { self.compile_stmt(stmt)?; }
        self.scopes.pop();
        Ok(())
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::VarDecl { ty, name, value } => {
                if !matches!(ty, Type::Int) { return Err("bytecode VM: only initialized int locals are optimized".into()); }
                let Some(expr) = value else { return Err("bytecode VM: uninitialized int locals are not optimized".into()); };
                if !self.is_int_expr(expr) { return Err("bytecode VM: non-integer local initializer".into()); }
                let slot = self.declare(name);
                self.compile_expr(expr)?;
                self.code.push(Op::Store(slot));
            }
            Stmt::Assign { target, op, value } => {
                let AssignTarget::Variable(name) = target else { return Err("indexed/member assignment not supported by bytecode VM".into()); };
                let Some(slot) = self.resolve(name) else { return Err(format!("unknown variable '{name}'")); };
                if !self.is_int_expr(value) { return Err("bytecode VM: assignment is not proven integer-safe".into()); }
                if !matches!(op, crate::ast::AssignOp::Set | crate::ast::AssignOp::Add | crate::ast::AssignOp::Sub | crate::ast::AssignOp::Mul | crate::ast::AssignOp::Div) { return Err("unsupported assignment operator".into()); }
                if matches!(op, crate::ast::AssignOp::Set) {
                    self.compile_expr(value)?;
                } else {
                    self.code.push(Op::Load(slot));
                    self.compile_expr(value)?;
                    self.code.push(match op { crate::ast::AssignOp::Add=>Op::Add, crate::ast::AssignOp::Sub=>Op::Sub, crate::ast::AssignOp::Mul=>Op::Mul, crate::ast::AssignOp::Div=>Op::Div, crate::ast::AssignOp::Set=>unreachable!() });
                }
                self.code.push(Op::Store(slot));
            }
            Stmt::Expr(expr) => { self.compile_expr(expr)?; self.code.push(Op::Pop); }
            Stmt::If { branches, else_body } => self.compile_if(branches, else_body.as_deref())?,
            Stmt::While { condition, body } => self.compile_while(condition, body)?,
            Stmt::ForC { init, condition, step, body } => self.compile_for(init.as_deref(), condition.as_ref(), step.as_deref(), body)?,
            Stmt::Break => {
                let idx = self.loops.len().checked_sub(1).ok_or_else(|| "break used outside loop".to_string())?;
                let patch = self.code.len();
                self.code.push(Op::Jump(usize::MAX));
                self.loops[idx].breaks.push(patch);
            }
            Stmt::Continue => {
                let idx = self.loops.len().checked_sub(1).ok_or_else(|| "continue used outside loop".to_string())?;
                let patch = self.code.len();
                self.code.push(Op::Jump(usize::MAX));
                self.loops[idx].continue_patches.push(patch);
            }
            Stmt::Return(expr) => {
                if let Some(expr) = expr { self.compile_expr(expr)?; } else { self.code.push(Op::Const(V::Null)); }
                self.code.push(Op::Return);
            }
            Stmt::ForEach { .. } => return Err("foreach not supported by bytecode VM".into()),
        }
        Ok(())
    }

    fn compile_if(&mut self, branches: &[(Expr, Vec<Stmt>)], else_body: Option<&[Stmt]>) -> Result<(), String> {
        let mut end_jumps = Vec::new();
        for (cond, body) in branches {
            self.compile_expr(cond)?;
            let jf = self.code.len(); self.code.push(Op::JumpIfFalse(usize::MAX));
            self.compile_block(body)?;
            let je = self.code.len(); self.code.push(Op::Jump(usize::MAX)); end_jumps.push(je);
            let next = self.code.len(); self.code[jf] = Op::JumpIfFalse(next);
        }
        if let Some(body) = else_body { self.compile_block(body)?; }
        let end = self.code.len();
        for j in end_jumps { self.code[j] = Op::Jump(end); }
        Ok(())
    }

    fn compile_while(&mut self, condition: &Expr, body: &[Stmt]) -> Result<(), String> {
        let cond = self.code.len();
        self.compile_expr(condition)?;
        let jf = self.code.len(); self.code.push(Op::JumpIfFalse(usize::MAX));
        self.loops.push(LoopTargets { continue_target: cond, ..LoopTargets::default() });
        self.compile_block(body)?;
        for p in self.loops.last().unwrap().continue_patches.clone() { self.code[p] = Op::Jump(cond); }
        self.code.push(Op::Jump(cond));
        let end = self.code.len();
        self.code[jf] = Op::JumpIfFalse(end);
        let loopinfo = self.loops.pop().unwrap();
        for p in loopinfo.breaks { self.code[p] = Op::Jump(end); }
        Ok(())
    }

    fn compile_for(&mut self, init: Option<&Stmt>, condition: Option<&Expr>, step: Option<&Stmt>, body: &[Stmt]) -> Result<(), String> {
        if let Some(init) = init { self.compile_stmt(init)?; }
        let cond = self.code.len();
        if let Some(condition) = condition { self.compile_expr(condition)?; let jf=self.code.len(); self.code.push(Op::JumpIfFalse(usize::MAX));
            self.loops.push(LoopTargets::default());
            self.compile_block(body)?;
            let step_start = self.code.len();
            for p in self.loops.last().unwrap().continue_patches.clone() { self.code[p] = Op::Jump(step_start); }
            if let Some(step) = step { self.compile_stmt(step)?; }
            self.code.push(Op::Jump(cond));
            let end=self.code.len(); self.code[jf]=Op::JumpIfFalse(end);
            let loopinfo=self.loops.pop().unwrap(); for p in loopinfo.breaks { self.code[p]=Op::Jump(end); }
        } else {
            self.loops.push(LoopTargets::default());
            self.compile_block(body)?;
            let step_start=self.code.len();
            for p in self.loops.last().unwrap().continue_patches.clone() { self.code[p]=Op::Jump(step_start); }
            if let Some(step)=step { self.compile_stmt(step)?; }
            self.code.push(Op::Jump(cond));
            let end=self.code.len();
            let loopinfo=self.loops.pop().unwrap(); for p in loopinfo.breaks { self.code[p]=Op::Jump(end); }
        }
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), String> {
        match expr {
            Expr::Int(v)=>self.code.push(Op::Const(V::Int(*v))),
            Expr::Float(_)=>return Err("bytecode VM: floating-point expressions use the reference interpreter".into()),
            Expr::Bool(v)=>self.code.push(Op::Const(V::Bool(*v))),
            Expr::Null=>self.code.push(Op::Const(V::Null)),
            Expr::String(_)=>return Err("strings not supported by bytecode VM".into()),
            Expr::Variable(name)=> { let Some(slot)=self.resolve(name) else { return Err(format!("unknown variable '{name}'")); }; self.code.push(Op::Load(slot)); }
            Expr::Array(_) | Expr::New{..} | Expr::Index{..} | Expr::Member{..} | Expr::Call{..} => return Err("complex expression not supported by bytecode VM".into()),
            Expr::Unary { op, expr } => { self.compile_expr(expr)?; self.code.push(match op { UnaryOp::Neg=>Op::Neg, UnaryOp::Not=>Op::Not, UnaryOp::Positive=>return Ok(()) }); }
            Expr::Binary { left, op, right } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    self.compile_expr(left)?;
                    let branch = self.code.len();
                    self.code.push(Op::JumpIfFalse(usize::MAX));
                    if matches!(op, BinaryOp::And) {
                        self.compile_expr(right)?;
                        self.code.push(Op::Not);
                        self.code.push(Op::Not);
                        let end = self.code.len();
                        self.code.push(Op::Jump(usize::MAX));
                        let false_label = self.code.len();
                        self.code.push(Op::Const(V::Bool(false)));
                        let final_end = self.code.len();
                        self.code[branch] = Op::JumpIfFalse(false_label);
                        self.code[end] = Op::Jump(final_end);
                    } else {
                        self.code.push(Op::Const(V::Bool(true)));
                        let end = self.code.len();
                        self.code.push(Op::Jump(usize::MAX));
                        let rhs = self.code.len();
                        self.compile_expr(right)?;
                        self.code.push(Op::Not);
                        self.code.push(Op::Not);
                        let final_end = self.code.len();
                        self.code[branch] = Op::JumpIfFalse(rhs);
                        self.code[end] = Op::Jump(final_end);
                    }
                    return Ok(());
                }
                self.compile_expr(left)?; self.compile_expr(right)?;
                self.code.push(match op { BinaryOp::Add=>Op::Add, BinaryOp::Sub=>Op::Sub, BinaryOp::Mul=>Op::Mul, BinaryOp::Div=>Op::Div, BinaryOp::Mod=>Op::Mod, BinaryOp::Equal=>Op::Eq, BinaryOp::NotEqual=>Op::Ne, BinaryOp::Less=>Op::Lt, BinaryOp::LessEqual=>Op::Le, BinaryOp::Greater=>Op::Gt, BinaryOp::GreaterEqual=>Op::Ge, BinaryOp::And|BinaryOp::Or=>unreachable!() });
            }
        }
        Ok(())
    }

    fn is_int_expr(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Int(_) => true,
            Expr::Variable(name) => self.resolve(name).is_some(),
            Expr::Unary { op: UnaryOp::Neg | UnaryOp::Positive, expr } => self.is_int_expr(expr),
            Expr::Binary { left, op, right } => matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod)
                && self.is_int_expr(left) && self.is_int_expr(right),
            _ => false,
        }
    }

    fn declare(&mut self, name: &str) -> usize {
        let slot = self.next_slot; self.next_slot += 1; self.scopes.last_mut().unwrap().insert(name.to_string(), slot); slot
    }

    fn resolve(&self, name: &str) -> Option<usize> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name).copied())
    }
}
