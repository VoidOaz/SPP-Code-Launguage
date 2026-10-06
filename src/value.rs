use crate::ast::Type;
use std::{cell::RefCell, collections::HashMap, fmt, rc::Rc};

#[derive(Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Array(Rc<RefCell<Vec<Value>>>),
    Object(Rc<RefCell<Object>>),
    Module(String),
    Null,
}

#[derive(Clone)]
pub struct Object {
    pub class_name: String,
    pub fields: HashMap<String, Value>,
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(v) => write!(f, "Int({v})"),
            Value::Float(v) => write!(f, "Float({v})"),
            Value::Bool(v) => write!(f, "Bool({v})"),
            Value::String(v) => write!(f, "String({v:?})"),
            Value::Array(v) => write!(f, "Array({v:?})"),
            Value::Object(v) => write!(f, "Object({:?})", v.borrow().class_name),
            Value::Module(v) => write!(f, "Module({v})"),
            Value::Null => write!(f, "Null"),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Array(a), Value::Array(b)) => Rc::ptr_eq(a, b),
            (Value::Object(a), Value::Object(b)) => Rc::ptr_eq(a, b),
            (Value::Module(a), Value::Module(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
    }
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Bool(v) => *v,
            Self::Int(v) => *v != 0,
            Self::Float(v) => *v != 0.0,
            Self::String(v) => !v.is_empty(),
            Self::Array(v) => !v.borrow().is_empty(),
            Self::Object(_) => true,
            Self::Module(_) => true,
            Self::Null => false,
        }
    }

    pub fn ty(&self) -> Type {
        match self {
            Self::Int(_) => Type::Int,
            Self::Float(_) => Type::Float,
            Self::Bool(_) => Type::Bool,
            Self::String(_) => Type::String,
            Self::Array(v) => {
                if let Some(first) = v.borrow().first() {
                    Type::Array(Box::new(first.ty()))
                } else {
                    Type::Array(Box::new(Type::Any))
                }
            }
            Self::Object(v) => Type::Named(v.borrow().class_name.clone()),
            Self::Module(name) => Type::Named(format!("module:{name}")),
            Self::Null => Type::Any,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        if let Self::Int(v) = self { Some(*v) } else { None }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => {
                if v.fract() == 0.0 { write!(f, "{v:.1}") } else { write!(f, "{v}") }
            }
            Self::Bool(v) => write!(f, "{v}"),
            Self::String(v) => write!(f, "{v}"),
            Self::Array(v) => {
                let items = v.borrow().iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
                write!(f, "[{items}]")
            }
            Self::Object(v) => write!(f, "<{} object>", v.borrow().class_name),
            Self::Module(v) => write!(f, "<{v} module>"),
            Self::Null => write!(f, "null"),
        }
    }
}
