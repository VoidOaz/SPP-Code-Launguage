use crate::{
    ast::{AssignOp, AssignTarget, BinaryOp, Class, Expr, Function, Program, Stmt, Type, UnaryOp},
    value::{Object, Value},
    stdlib,
    vm::BytecodeProgram,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    io::{self, Write},
    rc::Rc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
enum Flow { Continue, Break, Return(Value) }

/// Execute a program through the optimized bytecode path when the program is compatible.
/// Complex language features automatically fall back to the reference interpreter so
/// optimizations never change the supported SPP semantics.
pub fn run_program(program: &Program) -> Result<Value, String> {
    match BytecodeProgram::compile(program) {
        Ok(bytecode) => {
            bytecode.run()?;
            Ok(Value::Null)
        }
        Err(_) => Interpreter::new(program).run(),
    }
}

pub struct Interpreter {
    functions: HashMap<String, Function>,
    classes: HashMap<String, Class>,
    scopes: Vec<HashMap<String, Value>>,
    call_stack: Vec<String>,
    started: Instant,
}

impl Interpreter {
    pub fn new(program: &Program) -> Self {
        let mut functions = HashMap::new();
        let mut classes = HashMap::new();
        for f in &program.functions { functions.insert(f.name.clone(), f.clone()); }
        for class in &program.classes { classes.insert(class.name.clone(), class.clone()); }
        Self { functions, classes, scopes: vec![HashMap::new()], call_stack: Vec::new(), started: Instant::now() }
    }

    pub fn run(&mut self) -> Result<Value, String> { self.call_named("main", Vec::new(), None) }

    pub fn call_named(&mut self, name: &str, args: Vec<Value>, this_value: Option<Value>) -> Result<Value, String> {
        let function = self.functions.get(name).cloned().ok_or_else(|| format!("SPP runtime: unknown function '{name}'"))?;
        self.call_function(function, args, this_value)
    }

    fn call_function(&mut self, function: Function, args: Vec<Value>, this_value: Option<Value>) -> Result<Value, String> {
        if function.params.len() != args.len() {
            return Err(format!("SPP runtime: function '{}' expects {} argument(s), got {}", function.name, function.params.len(), args.len()));
        }
        self.call_stack.push(function.name.clone());
        self.scopes.push(HashMap::new());
        if let Some(this) = this_value { self.scopes.last_mut().unwrap().insert("this".into(), this); }
        for (param, value) in function.params.iter().zip(args.into_iter()) {
            let value = coerce_value(value, &param.ty, &self.classes).map_err(|e| format!("SPP runtime: parameter '{}': {e}", param.name))?;
            self.scopes.last_mut().unwrap().insert(param.name.clone(), value);
        }
        let flow = self.exec_block(&function.body);
        self.scopes.pop();
        self.call_stack.pop();
        match flow? {
            Flow::Return(value) => coerce_value(value, &function.return_type, &self.classes),
            Flow::Continue | Flow::Break => coerce_value(Value::Null, &function.return_type, &self.classes),
        }
    }

    fn exec_block(&mut self, statements: &[Stmt]) -> Result<Flow, String> {
        self.scopes.push(HashMap::new());
        for statement in statements {
            let flow = self.exec_stmt(statement)?;
            match flow {
                Flow::Continue => {}
                _ => { self.scopes.pop(); return Ok(flow); }
            }
        }
        self.scopes.pop();
        Ok(Flow::Continue)
    }

    fn exec_stmt(&mut self, statement: &Stmt) -> Result<Flow, String> {
        match statement {
            Stmt::VarDecl { ty, name, value } => {
                let value = match value { Some(expr) => self.eval(expr)?, None => Value::Null };
                let value = if matches!(ty, Type::Any) { value } else { coerce_value(value, ty, &self.classes).map_err(|e| format!("variable '{name}': {e}"))? };
                self.scopes.last_mut().unwrap().insert(name.clone(), value);
                Ok(Flow::Continue)
            }
            Stmt::Assign { target, op, value } => {
                let rhs = self.eval(value)?;
                self.assign(target, op, rhs)?;
                Ok(Flow::Continue)
            }
            Stmt::Expr(expr) => { let _ = self.eval(expr)?; Ok(Flow::Continue) }
            Stmt::If { branches, else_body } => {
                for (condition, body) in branches {
                    if self.eval(condition)?.is_truthy() { return self.exec_block(body); }
                }
                match else_body { Some(body) => self.exec_block(body), None => Ok(Flow::Continue) }
            }
            Stmt::While { condition, body } => {
                while self.eval(condition)?.is_truthy() {
                    match self.exec_block(body)? {
                        Flow::Continue => {}
                        Flow::Break => break,
                        Flow::Return(value) => return Ok(Flow::Return(value)),
                    }
                }
                Ok(Flow::Continue)
            }
            Stmt::ForEach { name, iterable, body } => {
                let iterable = self.eval(iterable)?;
                let items = match iterable {
                    Value::Array(v) => v.borrow().clone(),
                    Value::String(s) => s.chars().map(|c| Value::String(c.to_string())).collect(),
                    _ => return Err("SPP runtime: foreach needs an array or string".into()),
                };
                for item in items {
                    self.scopes.push(HashMap::new());
                    self.scopes.last_mut().unwrap().insert(name.clone(), item);
                    let flow = self.exec_block_without_scope(body)?;
                    self.scopes.pop();
                    match flow {
                        Flow::Continue => {}
                        Flow::Break => break,
                        Flow::Return(value) => return Ok(Flow::Return(value)),
                    }
                }
                Ok(Flow::Continue)
            }
            Stmt::ForC { init, condition, step, body } => {
                if let Some(init) = init { self.exec_stmt(init)?; }
                loop {
                    if let Some(cond) = condition { if !self.eval(cond)?.is_truthy() { break; } }
                    match self.exec_block(body)? {
                        Flow::Continue => {}
                        Flow::Break => break,
                        Flow::Return(value) => return Ok(Flow::Return(value)),
                    }
                    if let Some(step) = step { self.exec_stmt(step)?; }
                }
                Ok(Flow::Continue)
            }
            Stmt::Break => Ok(Flow::Break),
            Stmt::Continue => Ok(Flow::Continue),
            Stmt::Return(expr) => {
                let value = expr.as_ref().map(|e| self.eval(e)).transpose()?.unwrap_or(Value::Null);
                Ok(Flow::Return(value))
            }
        }
    }

    fn exec_block_without_scope(&mut self, statements: &[Stmt]) -> Result<Flow, String> {
        for statement in statements {
            match self.exec_stmt(statement)? {
                Flow::Continue => {}
                flow => return Ok(flow),
            }
        }
        Ok(Flow::Continue)
    }

    fn eval(&mut self, expr: &Expr) -> Result<Value, String> {
        match expr {
            Expr::Int(v) => Ok(Value::Int(*v)),
            Expr::Float(v) => Ok(Value::Float(*v)),
            Expr::Bool(v) => Ok(Value::Bool(*v)),
            Expr::String(v) => Ok(Value::String(v.clone())),
            Expr::Null => Ok(Value::Null),
            Expr::Variable(name) => self.get_variable(name).cloned(),
            Expr::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items { out.push(self.eval(item)?); }
                Ok(Value::Array(Rc::new(RefCell::new(out))))
            }
            Expr::New { class_name, args } => self.instantiate(class_name, args),
            Expr::Unary { op, expr } => {
                let value = self.eval(expr)?;
                match op {
                    UnaryOp::Neg => match value {
                        Value::Int(v) => Ok(Value::Int(v.checked_neg().ok_or_else(|| "SPP runtime: integer overflow".to_string())?)),
                        Value::Float(v) => Ok(Value::Float(-v)),
                        _ => Err("SPP runtime: unary '-' needs a number".into()),
                    },
                    UnaryOp::Positive => match value { Value::Int(v) => Ok(Value::Int(v)), Value::Float(v) => Ok(Value::Float(v)), _ => Err("SPP runtime: unary '+' needs a number".into()) },
                    UnaryOp::Not => Ok(Value::Bool(!value.is_truthy())),
                }
            }
            Expr::Binary { left, op, right } => {
                if matches!(op, BinaryOp::And) {
                    let left = self.eval(left)?; if !left.is_truthy() { return Ok(Value::Bool(false)); }
                    return Ok(Value::Bool(self.eval(right)?.is_truthy()));
                }
                if matches!(op, BinaryOp::Or) {
                    let left = self.eval(left)?; if left.is_truthy() { return Ok(Value::Bool(true)); }
                    return Ok(Value::Bool(self.eval(right)?.is_truthy()));
                }
                let left = self.eval(left)?; let right = self.eval(right)?; self.binary_op(left, op, right)
            }
            Expr::Call { callee, args } => self.eval_call(callee, args),
            Expr::Index { target, index } => {
                let target_value = self.eval(target)?;
                let index_value = self.eval(index)?;
                self.index_get(&target_value, &index_value)
            }
            Expr::Member { target, member } => {
                let target_value = self.eval(target)?;
                self.member_get(&target_value, member)
            }
        }
    }

    fn eval_call(&mut self, callee: &Expr, args: &[Expr]) -> Result<Value, String> {
        let values = args.iter().map(|a| self.eval(a)).collect::<Result<Vec<_>, _>>()?;
        match callee {
            Expr::Variable(name) => self.call_builtin_or_named(name, values),
            Expr::Member { target, member } => {
                if let Expr::Variable(module_name) = target.as_ref() {
                    if let Some(module) = stdlib::canonical_module(module_name) {
                        if !stdlib::is_allowed_member(module, member) {
                            return Err(format!("SPP runtime: unknown module member '{}.{}'", module, member));
                        }
                        let monotonic_ms = self.started.elapsed().as_millis() as i64;
                        return stdlib::module_call(module, member, &values, monotonic_ms);
                    }
                }
                let target_value = self.eval(target)?;
                self.call_member(target_value, member, values)
            }
            _ => Err("SPP runtime: only functions, methods, and built-ins can be called".into()),
        }
    }

    fn call_builtin_or_named(&mut self, name: &str, args: Vec<Value>) -> Result<Value, String> {
        match name {
            "print" | "println" => { expect_args(name, &args, 1)?; native_print(&args[0], true)?; Ok(Value::Null) }
            "write" => { expect_args(name, &args, 1)?; native_print(&args[0], false)?; Ok(Value::Null) }
            "len" => { expect_args(name, &args, 1)?; Ok(Value::Int(value_len(&args[0])?)) }
            "range" => {
                if args.len() == 1 { let end = as_int(&args[0], "range")?; return Ok(make_range(0, end, 1)); }
                if args.len() == 2 { let start = as_int(&args[0], "range")?; let end = as_int(&args[1], "range")?; return Ok(make_range(start, end, 1)); }
                if args.len() == 3 { let start = as_int(&args[0], "range")?; let end = as_int(&args[1], "range")?; let step = as_int(&args[2], "range")?; if step == 0 { return Err("SPP runtime: range step cannot be zero".into()); } return Ok(make_range(start, end, step)); }
                Err("SPP runtime: range expects 1 to 3 arguments".into())
            }
            "push" => { expect_args(name, &args, 2)?; let Value::Array(arr) = &args[0] else { return Err("SPP runtime: push expects an array".into()); }; arr.borrow_mut().push(args[1].clone()); Ok(Value::Null) }
            "pop" => { expect_args(name, &args, 1)?; let Value::Array(arr) = &args[0] else { return Err("SPP runtime: pop expects an array".into()); }; Ok(arr.borrow_mut().pop().unwrap_or(Value::Null)) }
            "input" | "readLine" => {
                if args.len() > 1 { return Err(format!("SPP runtime: {name} expects zero or one arguments")); }
                if let Some(prompt) = args.first() { native_print(prompt, false)?; }
                io::stdout().flush().map_err(|e| e.to_string())?;
                let mut line = String::new(); io::stdin().read_line(&mut line).map_err(|e| e.to_string())?;
                Ok(Value::String(line.trim_end_matches(['\r','\n']).to_string()))
            }
            "str" | "string" => { expect_args(name, &args, 1)?; Ok(Value::String(args[0].to_string())) }
            "int" => { expect_args(name, &args, 1)?; convert_int(&args[0]) }
            "float" | "double" => { expect_args(name, &args, 1)?; Ok(Value::Float(convert_float(&args[0])?)) }
            "bool" => { expect_args(name, &args, 1)?; Ok(Value::Bool(args[0].is_truthy())) }
            "typeOf" | "typeof" => { expect_args(name, &args, 1)?; Ok(Value::String(value_type_name(&args[0]))) }
            "abs" => { expect_args(name, &args, 1)?; match &args[0] { Value::Int(v) => Ok(Value::Int(v.checked_abs().ok_or_else(|| "SPP runtime: integer overflow".to_string())?)), Value::Float(v) => Ok(Value::Float(v.abs())), _ => Err("SPP runtime: abs expects a number".into()) } }
            "sqrt" => { expect_args(name, &args, 1)?; Ok(Value::Float(convert_float(&args[0])?.sqrt())) }
            "floor" => { expect_args(name, &args, 1)?; Ok(Value::Float(convert_float(&args[0])?.floor())) }
            "ceil" => { expect_args(name, &args, 1)?; Ok(Value::Float(convert_float(&args[0])?.ceil())) }
            "min" | "max" => { expect_args(name, &args, 2)?; numeric_minmax(name, &args[0], &args[1]) }
            "time_ms" => { expect_args(name, &args, 0)?; let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?; Ok(Value::Int(now.as_millis() as i64)) }
            "sleep_ms" => { expect_args(name, &args, 1)?; thread::sleep(Duration::from_millis(as_int(&args[0], "sleep_ms")?.max(0) as u64)); Ok(Value::Null) }
            "assert" => { expect_args(name, &args, 1)?; if !args[0].is_truthy() { return Err("SPP assertion failed".into()); } Ok(Value::Null) }
            "readFile" => { expect_args(name, &args, 1)?; let path = value_string(&args[0])?; Ok(Value::String(fs::read_to_string(path).map_err(|e| e.to_string())?)) }
            "writeFile" => { expect_args(name, &args, 2)?; let path = value_string(&args[0])?; let data = value_string(&args[1])?; fs::write(path, data).map_err(|e| e.to_string())?; Ok(Value::Null) }
            _ => self.call_named(name, args, None),
        }
    }

    fn call_member(&mut self, target: Value, member: &str, args: Vec<Value>) -> Result<Value, String> {
        match target.clone() {
            Value::Object(object) => {
                let class_name = object.borrow().class_name.clone();
                let method = self.classes.get(&class_name).and_then(|class| class.methods.iter().find(|m| m.name == member).cloned());
                let Some(method) = method else { return Err(format!("SPP runtime: class '{class_name}' has no method '{member}'")); };
                self.call_function(method, args, Some(target))
            }
            Value::Array(array) => match member {
                "push" => { expect_args(member, &args, 1)?; array.borrow_mut().push(args[0].clone()); Ok(Value::Null) }
                "pop" => { expect_args(member, &args, 0)?; Ok(array.borrow_mut().pop().unwrap_or(Value::Null)) }
                "length" | "len" => { expect_args(member, &args, 0)?; Ok(Value::Int(array.borrow().len() as i64)) }
                "clear" => { expect_args(member, &args, 0)?; array.borrow_mut().clear(); Ok(Value::Null) }
                _ => Err(format!("SPP runtime: array has no method '{member}'")),
            },
            Value::String(s) => match member {
                "length" | "len" => { expect_args(member, &args, 0)?; Ok(Value::Int(s.chars().count() as i64)) }
                "upper" | "toUpper" => { expect_args(member, &args, 0)?; Ok(Value::String(s.to_uppercase())) }
                "lower" | "toLower" => { expect_args(member, &args, 0)?; Ok(Value::String(s.to_lowercase())) }
                "contains" => { expect_args(member, &args, 1)?; Ok(Value::Bool(s.contains(&value_string(&args[0])?))) }
                _ => Err(format!("SPP runtime: string has no method '{member}'")),
            },
            _ => Err(format!("SPP runtime: value has no member method '{member}'")),
        }
    }

    fn instantiate(&mut self, class_name: &str, args: &[Expr]) -> Result<Value, String> {
        let class = self.classes.get(class_name).cloned().ok_or_else(|| format!("SPP runtime: unknown class '{class_name}'"))?;
        let object = Rc::new(RefCell::new(Object { class_name: class_name.to_string(), fields: HashMap::new() }));
        for field in &class.fields { object.borrow_mut().fields.insert(field.name.clone(), Value::Null); }
        self.scopes.push(HashMap::new());
        self.scopes.last_mut().unwrap().insert("this".into(), Value::Object(object.clone()));
        for field in &class.fields {
            if let Some(default) = &field.default {
                let value = self.eval(default)?;
                let value = coerce_value(value, &field.ty, &self.classes).map_err(|e| format!("field '{}': {e}", field.name))?;
                object.borrow_mut().fields.insert(field.name.clone(), value);
            }
        }
        self.scopes.pop();
        if let Some(init) = class.methods.iter().find(|m| m.name == "init").cloned() {
            let values = args.iter().map(|a| self.eval(a)).collect::<Result<Vec<_>, _>>()?;
            self.call_function(init, values, Some(Value::Object(object.clone())))?;
        } else if !args.is_empty() {
            return Err(format!("SPP runtime: class '{class_name}' has no init constructor"));
        }
        Ok(Value::Object(object))
    }

    fn index_get(&self, target: &Value, index: &Value) -> Result<Value, String> {
        let i = as_int(index, "index")?;
        if i < 0 { return Err("SPP runtime: index cannot be negative".into()); }
        let i = i as usize;
        match target {
            Value::Array(arr) => arr.borrow().get(i).cloned().ok_or_else(|| format!("SPP runtime: array index {i} out of bounds")),
            Value::String(s) => s.chars().nth(i).map(|c| Value::String(c.to_string())).ok_or_else(|| format!("SPP runtime: string index {i} out of bounds")),
            _ => Err("SPP runtime: indexing requires an array or string".into()),
        }
    }

    fn member_get(&self, target: &Value, member: &str) -> Result<Value, String> {
        match target {
            Value::Object(object) => object.borrow().fields.get(member).cloned().ok_or_else(|| format!("SPP runtime: unknown field '{member}' on {}", object.borrow().class_name)),
            Value::Array(arr) if member == "length" || member == "len" => Ok(Value::Int(arr.borrow().len() as i64)),
            Value::String(s) if member == "length" || member == "len" => Ok(Value::Int(s.chars().count() as i64)),
            Value::Module(module) => {
                if let Some(value) = stdlib::module_constant(&module, member) {
                    Ok(value)
                } else {
                    Err(format!("SPP runtime: module '{module}' does not expose member '{member}' as a value"))
                }
            }
            _ => Err(format!("SPP runtime: unknown member '{member}'")),
        }
    }

    fn assign(&mut self, target: &AssignTarget, op: &AssignOp, rhs: Value) -> Result<(), String> {
        match target {
            AssignTarget::Variable(name) => {
                let old = self.get_variable(name)?.clone();
                let value = apply_assign(&old, op, rhs)?;
                self.set_variable(name, value)
            }
            AssignTarget::Index { target, index } => {
                let target_value = self.eval(target)?;
                let index = as_int(&self.eval(index)?, "index")?;
                if index < 0 { return Err("SPP runtime: index cannot be negative".into()); }
                match target_value {
                    Value::Array(arr) => {
                        let idx = index as usize;
                        let mut borrow = arr.borrow_mut();
                        let Some(old) = borrow.get(idx).cloned() else { return Err(format!("SPP runtime: array index {idx} out of bounds")); };
                        borrow[idx] = apply_assign(&old, op, rhs)?;
                        Ok(())
                    }
                    _ => Err("SPP runtime: indexed assignment requires an array".into()),
                }
            }
            AssignTarget::Member { target, member } => {
                let target_value = self.eval(target)?;
                let Value::Object(object) = target_value else { return Err("SPP runtime: member assignment requires an object".into()); };
                let old = object.borrow().fields.get(member).cloned().ok_or_else(|| format!("SPP runtime: unknown field '{member}'"))?;
                let value = apply_assign(&old, op, rhs)?;
                object.borrow_mut().fields.insert(member.clone(), value);
                Ok(())
            }
        }
    }

    fn get_variable(&self, name: &str) -> Result<&Value, String> {
        for scope in self.scopes.iter().rev() { if let Some(v) = scope.get(name) { return Ok(v); } }
        Err(self.runtime_error(&format!("unknown variable '{name}'")))
    }

    fn set_variable(&mut self, name: &str, value: Value) -> Result<(), String> {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) { scope.insert(name.to_string(), value); return Ok(()); }
        }
        Err(self.runtime_error(&format!("unknown variable '{name}'")))
    }

    fn runtime_error(&self, msg: &str) -> String {
        if self.call_stack.is_empty() { return format!("SPP runtime: {msg}"); }
        format!("SPP runtime: {msg}\nCall stack: {}", self.call_stack.join(" -> "))
    }
}

fn apply_assign(old: &Value, op: &AssignOp, rhs: Value) -> Result<Value, String> {
    match op {
        AssignOp::Set => Ok(rhs),
        AssignOp::Add => binary_add(old.clone(), rhs),
        AssignOp::Sub => binary_math(old.clone(), rhs, BinaryOp::Sub),
        AssignOp::Mul => binary_math(old.clone(), rhs, BinaryOp::Mul),
        AssignOp::Div => binary_math(old.clone(), rhs, BinaryOp::Div),
    }
}

fn binary_add(left: Value, right: Value) -> Result<Value, String> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.checked_add(b).ok_or_else(|| "SPP runtime: integer overflow".to_string())?)),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 + b)),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + b as f64)),
        (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{a}{b}"))),
        (Value::String(a), b) => Ok(Value::String(format!("{a}{b}"))),
        (a, Value::String(b)) => Ok(Value::String(format!("{a}{b}"))),
        (a, b) => Err(format!("SPP runtime: cannot add {a:?} and {b:?}")),
    }
}

fn binary_math(left: Value, right: Value, op: BinaryOp) -> Result<Value, String> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => match op {
            BinaryOp::Sub => Ok(Value::Int(a.checked_sub(b).ok_or_else(|| "SPP runtime: integer overflow".to_string())?)),
            BinaryOp::Mul => Ok(Value::Int(a.checked_mul(b).ok_or_else(|| "SPP runtime: integer overflow".to_string())?)),
            BinaryOp::Div => {
                if b == 0 { Err("SPP runtime: division by zero".into()) }
                else { Ok(Value::Int(a.checked_div(b).ok_or_else(|| "SPP runtime: integer division overflow".to_string())?)) }
            },
            BinaryOp::Mod => {
                if b == 0 { Err("SPP runtime: modulo by zero".into()) }
                else { Ok(Value::Int(a.checked_rem(b).ok_or_else(|| "SPP runtime: integer modulo overflow".to_string())?)) }
            },
            _ => unreachable!(),
        },
        (a, b) => {
            let a = convert_float(&a)?; let b = convert_float(&b)?;
            match op {
                BinaryOp::Sub => Ok(Value::Float(a - b)), BinaryOp::Mul => Ok(Value::Float(a * b)),
                BinaryOp::Div => if b == 0.0 { Err("SPP runtime: division by zero".into()) } else { Ok(Value::Float(a / b)) },
                BinaryOp::Mod => Ok(Value::Float(a % b)), _ => unreachable!(),
            }
        }
    }
}

fn binary_compare(left: Value, op: &BinaryOp, right: Value) -> Result<Value, String> {
    let result = match (left, right) {
        (Value::Int(a), Value::Int(b)) => compare_fns(a as f64, b as f64, op),
        (Value::Float(a), Value::Float(b)) => compare_fns(a, b, op),
        (Value::Int(a), Value::Float(b)) => compare_fns(a as f64, b, op),
        (Value::Float(a), Value::Int(b)) => compare_fns(a, b as f64, op),
        (Value::String(a), Value::String(b)) => compare_string(&a, &b, op),
        (a, b) => return Err(format!("SPP runtime: values are not comparable: {a:?} and {b:?}")),
    };
    Ok(Value::Bool(result))
}

fn compare_fns(a: f64, b: f64, op: &BinaryOp) -> bool {
    match op { BinaryOp::Less => a < b, BinaryOp::LessEqual => a <= b, BinaryOp::Greater => a > b, BinaryOp::GreaterEqual => a >= b, _ => false }
}
fn compare_string(a: &str, b: &str, op: &BinaryOp) -> bool {
    match op { BinaryOp::Less => a < b, BinaryOp::LessEqual => a <= b, BinaryOp::Greater => a > b, BinaryOp::GreaterEqual => a >= b, _ => false }
}

fn Interpreter_binary_op(left: Value, op: &BinaryOp, right: Value) -> Result<Value, String> {
    match op {
        BinaryOp::Add => binary_add(left, right),
        BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => binary_math(left, right, op.clone()),
        BinaryOp::Equal => Ok(Value::Bool(left == right)),
        BinaryOp::NotEqual => Ok(Value::Bool(left != right)),
        BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => binary_compare(left, op, right),
        BinaryOp::And | BinaryOp::Or => unreachable!(),
    }
}

impl Interpreter {
    fn binary_op(&self, left: Value, op: &BinaryOp, right: Value) -> Result<Value, String> { Interpreter_binary_op(left, op, right) }
}

fn coerce_value(value: Value, ty: &Type, classes: &HashMap<String, Class>) -> Result<Value, String> {
    match ty {
        Type::Any => Ok(value),
        Type::Int if matches!(value, Value::Int(_)) => Ok(value),
        Type::Float if matches!(value, Value::Float(_) | Value::Int(_)) => match value { Value::Int(v) => Ok(Value::Float(v as f64)), _ => Ok(value) },
        Type::Bool if matches!(value, Value::Bool(_)) => Ok(value),
        Type::String if matches!(value, Value::String(_)) => Ok(value),
        Type::Void => Ok(Value::Null),
        Type::Array(inner) => match value {
            Value::Array(values) => {
                let values = values.borrow().iter().cloned().map(|v| coerce_value(v, inner, classes)).collect::<Result<Vec<_>, _>>()?;
                Ok(Value::Array(Rc::new(RefCell::new(values))))
            }
            Value::Null => Ok(Value::Null),
            _ => Err(format!("expected {}, got {}", ty.name(), value_type_name(&value))),
        },
        Type::Named(class_name) => match value {
            Value::Object(ref obj) if &obj.borrow().class_name == class_name && classes.contains_key(class_name) => Ok(value),
            Value::Null => Ok(Value::Null),
            _ => Err(format!("expected {class_name}, got {}", value_type_name(&value))),
        },
        _ => Err(format!("expected {}, got {}", ty.name(), value_type_name(&value))),
    }
}

fn expect_args(name: &str, args: &[Value], count: usize) -> Result<(), String> {
    if args.len() == count { Ok(()) } else { Err(format!("SPP runtime: {name} expects {count} argument(s), got {}", args.len())) }
}

fn as_int(value: &Value, name: &str) -> Result<i64, String> { value.as_int().ok_or_else(|| format!("SPP runtime: {name} expects an int")) }
fn value_string(value: &Value) -> Result<String, String> { match value { Value::String(v) => Ok(v.clone()), _ => Err("SPP runtime: expected a string".into()) } }
fn convert_int(value: &Value) -> Result<Value, String> {
    match value { Value::Int(v) => Ok(Value::Int(*v)), Value::Float(v) => Ok(Value::Int(*v as i64)), Value::Bool(v) => Ok(Value::Int(i64::from(*v))), Value::String(s) => s.parse::<i64>().map(Value::Int).map_err(|_| "SPP runtime: cannot convert string to int".into()), _ => Err("SPP runtime: cannot convert value to int".into()) }
}
fn convert_float(value: &Value) -> Result<f64, String> {
    match value { Value::Int(v) => Ok(*v as f64), Value::Float(v) => Ok(*v), Value::Bool(v) => Ok(if *v { 1.0 } else { 0.0 }), Value::String(s) => s.parse::<f64>().map_err(|_| "SPP runtime: cannot convert string to float".into()), _ => Err("SPP runtime: numeric conversion requires int/float/bool/string".into()) }
}
fn value_len(value: &Value) -> Result<i64, String> {
    match value { Value::String(s) => Ok(s.chars().count() as i64), Value::Array(a) => Ok(a.borrow().len() as i64), _ => Err("SPP runtime: len expects an array or string".into()) }
}
fn value_type_name(value: &Value) -> String {
    match value { Value::Int(_) => "int", Value::Float(_) => "float", Value::Bool(_) => "bool", Value::String(_) => "string", Value::Array(_) => "array", Value::Object(o) => return o.borrow().class_name.clone(), Value::Module(m) => return format!("module:{m}"), Value::Null => "null" }.into()
}
fn numeric_minmax(name: &str, a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(if name == "min" { (*a).min(*b) } else { (*a).max(*b) })),
        _ => { let a = convert_float(a)?; let b = convert_float(b)?; Ok(Value::Float(if name == "min" { a.min(b) } else { a.max(b) })) }
    }
}
fn make_range(start: i64, end: i64, step: i64) -> Value {
    let mut out = Vec::new();
    let mut n = start;
    if step > 0 {
        while n < end {
            out.push(Value::Int(n));
            let Some(next) = n.checked_add(step) else { break };
            n = next;
        }
    } else {
        while n > end {
            out.push(Value::Int(n));
            let Some(next) = n.checked_add(step) else { break };
            n = next;
        }
    }
    Value::Array(Rc::new(RefCell::new(out)))
}
fn native_print(value: &Value, newline: bool) -> Result<(), String> {
    if newline { println!("{value}"); } else { print!("{value}"); io::stdout().flush().map_err(|e| e.to_string())?; }
    Ok(())
}

#[allow(dead_code)]
pub fn perf_now() -> Instant { Instant::now() }
