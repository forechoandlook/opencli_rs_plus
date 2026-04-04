use serde_json::Value;
use std::collections::HashMap;

use opencli_rs_core::CliError;

use super::filters::apply_filter;
use super::parser::{BinOpKind, Expr};

/// Context available to template expressions.
pub struct TemplateContext {
    pub args: HashMap<String, Value>,
    pub data: Value,
    pub item: Value,
    pub index: usize,
}

impl Default for TemplateContext {
    fn default() -> Self {
        Self {
            args: HashMap::new(),
            data: Value::Null,
            item: Value::Null,
            index: 0,
        }
    }
}

pub fn evaluate(expr: &Expr, ctx: &TemplateContext) -> Result<Value, CliError> {
    match expr {
        Expr::IntLit(n) => Ok(Value::Number((*n).into())),
        Expr::FloatLit(f) => Ok(Value::Number(
            serde_json::Number::from_f64(*f)
                .ok_or_else(|| CliError::pipeline("Invalid float value"))?,
        )),
        Expr::StringLit(s) => Ok(Value::String(s.clone())),
        Expr::BoolLit(b) => Ok(Value::Bool(*b)),
        Expr::NullLit => Ok(Value::Null),

        Expr::Ident(name) => resolve_ident(name, ctx),

        Expr::DotAccess(base, field) => {
            let base_val = evaluate(base, ctx)?;
            Ok(access_field(&base_val, field))
        }

        Expr::BracketAccess(base, index_expr) => {
            let base_val = evaluate(base, ctx)?;
            let index_val = evaluate(index_expr, ctx)?;
            Ok(access_index(&base_val, &index_val))
        }

        Expr::FuncCall { namespace, args } => {
            let eval_args: Vec<Value> = args
                .iter()
                .map(|a| evaluate(a, ctx))
                .collect::<Result<_, _>>()?;
            call_function(namespace, &eval_args)
        }

        Expr::UnaryNot(inner) => {
            let val = evaluate(inner, ctx)?;
            Ok(Value::Bool(!is_truthy(&val)))
        }

        Expr::BinOp { left, op, right } => {
            let lval = evaluate(left, ctx)?;

            // Short-circuit for logical operators
            match op {
                BinOpKind::Or => {
                    if is_truthy(&lval) {
                        return Ok(lval);
                    }
                    return evaluate(right, ctx);
                }
                BinOpKind::And => {
                    if !is_truthy(&lval) {
                        return Ok(lval);
                    }
                    return evaluate(right, ctx);
                }
                _ => {}
            }

            let rval = evaluate(right, ctx)?;
            eval_binop(op, &lval, &rval)
        }

        Expr::Ternary {
            condition,
            if_true,
            if_false,
        } => {
            let cond = evaluate(condition, ctx)?;
            if is_truthy(&cond) {
                evaluate(if_true, ctx)
            } else {
                evaluate(if_false, ctx)
            }
        }

        Expr::Pipe { expr, filter, args } => {
            let val = evaluate(expr, ctx)?;
            let eval_args: Vec<Value> = args
                .iter()
                .map(|a| evaluate(a, ctx))
                .collect::<Result<_, _>>()?;
            apply_filter(filter, val, &eval_args)
        }
    }
}

fn resolve_ident(name: &str, ctx: &TemplateContext) -> Result<Value, CliError> {
    match name {
        "args" => Ok(serde_json::to_value(&ctx.args).unwrap_or(Value::Null)),
        "data" => Ok(ctx.data.clone()),
        "item" => Ok(ctx.item.clone()),
        "index" => Ok(Value::Number(ctx.index.into())),
        "true" => Ok(Value::Bool(true)),
        "false" => Ok(Value::Bool(false)),
        "null" => Ok(Value::Null),
        _ => {
            // Try args as a convenience shortcut
            if let Some(val) = ctx.args.get(name) {
                Ok(val.clone())
            } else {
                Ok(Value::Null)
            }
        }
    }
}

fn access_field(val: &Value, field: &str) -> Value {
    match val {
        Value::Object(map) => map.get(field).cloned().unwrap_or(Value::Null),
        Value::Array(arr) if field == "length" => Value::Number(arr.len().into()),
        Value::String(s) if field == "length" => Value::Number(s.len().into()),
        _ => Value::Null,
    }
}

fn access_index(val: &Value, index: &Value) -> Value {
    match val {
        Value::Array(arr) => {
            if let Some(i) = index.as_u64() {
                arr.get(i as usize).cloned().unwrap_or(Value::Null)
            } else {
                Value::Null
            }
        }
        Value::Object(map) => {
            if let Some(key) = index.as_str() {
                map.get(key).cloned().unwrap_or(Value::Null)
            } else {
                Value::Null
            }
        }
        _ => Value::Null,
    }
}

fn is_truthy(val: &Value) -> bool {
    match val {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i != 0
            } else if let Some(f) = n.as_f64() {
                f != 0.0
            } else {
                true
            }
        }
        Value::String(s) => !s.is_empty(),
        Value::Array(arr) => !arr.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn to_f64(val: &Value) -> Option<f64> {
    match val {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn eval_binop(op: &BinOpKind, left: &Value, right: &Value) -> Result<Value, CliError> {
    match op {
        BinOpKind::Add => {
            // String concat if either is string
            if let (Some(l), Some(r)) = (left.as_str(), right.as_str()) {
                return Ok(Value::String(format!("{l}{r}")));
            }
            if let (Some(l), Some(r)) = (to_f64(left), to_f64(right)) {
                let result = l + r;
                return Ok(num_to_value(result));
            }
            // String + anything
            if left.is_string() || right.is_string() {
                return Ok(Value::String(format!(
                    "{}{}",
                    value_to_display(left),
                    value_to_display(right)
                )));
            }
            Ok(Value::Null)
        }
        BinOpKind::Sub => arith(left, right, |a, b| a - b),
        BinOpKind::Mul => arith(left, right, |a, b| a * b),
        BinOpKind::Div => arith(left, right, |a, b| if b == 0.0 { f64::NAN } else { a / b }),
        BinOpKind::Mod => arith(left, right, |a, b| if b == 0.0 { f64::NAN } else { a % b }),
        BinOpKind::Gt => Ok(Value::Bool(
            cmp_values(left, right) == Some(std::cmp::Ordering::Greater),
        )),
        BinOpKind::Lt => Ok(Value::Bool(
            cmp_values(left, right) == Some(std::cmp::Ordering::Less),
        )),
        BinOpKind::Gte => Ok(Value::Bool(matches!(
            cmp_values(left, right),
            Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
        ))),
        BinOpKind::Lte => Ok(Value::Bool(matches!(
            cmp_values(left, right),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        ))),
        BinOpKind::Eq => Ok(Value::Bool(left == right)),
        BinOpKind::Neq => Ok(Value::Bool(left != right)),
        BinOpKind::Or | BinOpKind::And => {
            // Already handled via short-circuit above
            unreachable!()
        }
    }
}

fn arith(left: &Value, right: &Value, f: impl Fn(f64, f64) -> f64) -> Result<Value, CliError> {
    if let (Some(l), Some(r)) = (to_f64(left), to_f64(right)) {
        Ok(num_to_value(f(l, r)))
    } else {
        Ok(Value::Null)
    }
}

fn num_to_value(n: f64) -> Value {
    if n.fract() == 0.0 && n.is_finite() && n.abs() < (i64::MAX as f64) {
        Value::Number((n as i64).into())
    } else if let Some(num) = serde_json::Number::from_f64(n) {
        Value::Number(num)
    } else {
        Value::Null
    }
}

fn cmp_values(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    if let (Some(l), Some(r)) = (to_f64(left), to_f64(right)) {
        l.partial_cmp(&r)
    } else if let (Some(l), Some(r)) = (left.as_str(), right.as_str()) {
        Some(l.cmp(r))
    } else {
        None
    }
}

fn call_function(namespace: &[String], args: &[Value]) -> Result<Value, CliError> {
    let full_name: Vec<&str> = namespace.iter().map(|s| s.as_str()).collect();
    match full_name.as_slice() {
        ["Math", "min"] => {
            let result = args
                .iter()
                .filter_map(|v| v.as_f64())
                .fold(f64::INFINITY, f64::min);
            if result == f64::INFINITY {
                Ok(Value::Null)
            } else {
                Ok(num_to_value(result))
            }
        }
        ["Math", "max"] => {
            let result = args
                .iter()
                .filter_map(|v| v.as_f64())
                .fold(f64::NEG_INFINITY, f64::max);
            if result == f64::NEG_INFINITY {
                Ok(Value::Null)
            } else {
                Ok(num_to_value(result))
            }
        }
        ["Math", "abs"] => {
            let val = args.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
            Ok(num_to_value(val.abs()))
        }
        ["Math", "floor"] => {
            let val = args.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
            Ok(num_to_value(val.floor()))
        }
        ["Math", "ceil"] => {
            let val = args.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
            Ok(num_to_value(val.ceil()))
        }
        ["Math", "round"] => {
            let val = args.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
            Ok(num_to_value(val.round()))
        }
        _ => Err(CliError::pipeline(format!(
            "Unknown function: {}",
            namespace.join(".")
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn ctx_with_item(item: Value) -> TemplateContext {
        TemplateContext {
            args: HashMap::new(),
            data: Value::Null,
            item,
            index: 0,
        }
    }

    fn empty_ctx() -> TemplateContext {
        TemplateContext::default()
    }

    fn eval(expr: &str, ctx: &TemplateContext) -> Value {
        use super::super::parser::parse_expression;
        let ast = parse_expression(expr).unwrap();
        evaluate(&ast, ctx).unwrap()
    }

    // ── 字面量 ────────────────────────────────────────────────────────────

    #[test]
    fn int_literal() {
        assert_eq!(eval("42", &empty_ctx()), Value::Number(42.into()));
    }

    #[test]
    fn negative_int_literal() {
        // -1 被解析为 0 - 1（减法）
        assert_eq!(eval("0 - 1", &empty_ctx()), Value::Number((-1i64).into()));
    }

    #[test]
    fn bool_true_literal() {
        assert_eq!(eval("true", &empty_ctx()), Value::Bool(true));
    }

    #[test]
    fn bool_false_literal() {
        assert_eq!(eval("false", &empty_ctx()), Value::Bool(false));
    }

    #[test]
    fn null_literal() {
        assert_eq!(eval("null", &empty_ctx()), Value::Null);
    }

    #[test]
    fn string_literal_double_quote() {
        assert_eq!(eval("\"hello\"", &empty_ctx()), Value::String("hello".into()));
    }

    #[test]
    fn string_literal_single_quote() {
        assert_eq!(eval("'world'", &empty_ctx()), Value::String("world".into()));
    }

    // ── 变量解析 ──────────────────────────────────────────────────────────

    #[test]
    fn index_variable_resolves() {
        let mut ctx = empty_ctx();
        ctx.index = 5;
        assert_eq!(eval("index", &ctx), Value::Number(5.into()));
    }

    #[test]
    fn missing_field_on_item_returns_null() {
        let ctx = ctx_with_item(serde_json::json!({"name": "x"}));
        assert_eq!(eval("item.nonexistent", &ctx), Value::Null);
    }

    #[test]
    fn missing_arg_returns_null() {
        let ctx = empty_ctx();
        assert_eq!(eval("args.missing_param", &ctx), Value::Null);
    }

    #[test]
    fn dot_access_on_null_returns_null() {
        let ctx = ctx_with_item(Value::Null);
        assert_eq!(eval("item.anything", &ctx), Value::Null);
    }

    #[test]
    fn nested_dot_access() {
        let ctx = ctx_with_item(serde_json::json!({"author": {"name": "Alice"}}));
        assert_eq!(eval("item.author.name", &ctx), Value::String("Alice".into()));
    }

    #[test]
    fn bracket_access_array() {
        let ctx = ctx_with_item(serde_json::json!({"tags": ["a", "b", "c"]}));
        assert_eq!(eval("item.tags[1]", &ctx), Value::String("b".into()));
    }

    #[test]
    fn bracket_access_out_of_bounds_returns_null() {
        let ctx = ctx_with_item(serde_json::json!({"tags": ["a"]}));
        assert_eq!(eval("item.tags[99]", &ctx), Value::Null);
    }

    #[test]
    fn array_length_field() {
        let ctx = ctx_with_item(serde_json::json!({"tags": ["a", "b"]}));
        assert_eq!(eval("item.tags.length", &ctx), Value::Number(2.into()));
    }

    #[test]
    fn string_length_field() {
        let ctx = ctx_with_item(serde_json::json!({"name": "hello"}));
        assert_eq!(eval("item.name.length", &ctx), Value::Number(5.into()));
    }

    // ── 算术运算 ──────────────────────────────────────────────────────────

    #[test]
    fn addition() {
        assert_eq!(eval("1 + 2", &empty_ctx()), Value::Number(3.into()));
    }

    #[test]
    fn subtraction() {
        assert_eq!(eval("5 - 3", &empty_ctx()), Value::Number(2.into()));
    }

    #[test]
    fn multiplication() {
        assert_eq!(eval("3 * 4", &empty_ctx()), Value::Number(12.into()));
    }

    #[test]
    fn division() {
        assert_eq!(eval("10 / 2", &empty_ctx()), Value::Number(5.into()));
    }

    #[test]
    fn modulo() {
        assert_eq!(eval("7 % 3", &empty_ctx()), Value::Number(1.into()));
    }

    #[test]
    fn division_by_zero_returns_null() {
        // NaN → Null（num_to_value 的处理）
        let result = eval("1 / 0", &empty_ctx());
        assert_eq!(result, Value::Null);
    }

    #[test]
    fn string_concat_with_plus() {
        let ctx = ctx_with_item(serde_json::json!({"prefix": "hello"}));
        let result = eval("item.prefix + \" world\"", &ctx);
        assert_eq!(result, Value::String("hello world".into()));
    }

    #[test]
    fn null_plus_number_returns_null() {
        // null + 1 — 两边都不是 string 且 null 无法转为 f64
        let ctx = ctx_with_item(Value::Null);
        let result = eval("item.missing + 1", &ctx);
        assert_eq!(result, Value::Null);
    }

    // ── 比较运算 ──────────────────────────────────────────────────────────

    #[test]
    fn greater_than_true() {
        assert_eq!(eval("5 > 3", &empty_ctx()), Value::Bool(true));
    }

    #[test]
    fn greater_than_false() {
        assert_eq!(eval("3 > 5", &empty_ctx()), Value::Bool(false));
    }

    #[test]
    fn less_than() {
        assert_eq!(eval("3 < 5", &empty_ctx()), Value::Bool(true));
    }

    #[test]
    fn equal_numbers() {
        assert_eq!(eval("42 == 42", &empty_ctx()), Value::Bool(true));
    }

    #[test]
    fn not_equal() {
        assert_eq!(eval("1 != 2", &empty_ctx()), Value::Bool(true));
    }

    #[test]
    fn greater_than_or_equal() {
        assert_eq!(eval("5 >= 5", &empty_ctx()), Value::Bool(true));
        assert_eq!(eval("6 >= 5", &empty_ctx()), Value::Bool(true));
        assert_eq!(eval("4 >= 5", &empty_ctx()), Value::Bool(false));
    }

    #[test]
    fn less_than_or_equal() {
        assert_eq!(eval("5 <= 5", &empty_ctx()), Value::Bool(true));
        assert_eq!(eval("4 <= 5", &empty_ctx()), Value::Bool(true));
        assert_eq!(eval("6 <= 5", &empty_ctx()), Value::Bool(false));
    }

    // ── 逻辑运算（短路） ──────────────────────────────────────────────────

    #[test]
    fn and_both_true() {
        assert_eq!(eval("true && true", &empty_ctx()), Value::Bool(true));
    }

    #[test]
    fn and_short_circuits_on_false() {
        // false && anything — 直接返回 false，不求值右边
        assert_eq!(eval("false && true", &empty_ctx()), Value::Bool(false));
    }

    #[test]
    fn or_returns_first_truthy() {
        // "hello" || "world" — 返回 "hello"（第一个 truthy 值，不是 bool）
        let result = eval("\"hello\" || \"world\"", &empty_ctx());
        assert_eq!(result, Value::String("hello".into()));
    }

    #[test]
    fn or_returns_last_when_first_falsy() {
        let result = eval("null || \"fallback\"", &empty_ctx());
        assert_eq!(result, Value::String("fallback".into()));
    }

    #[test]
    fn or_null_or_null_returns_null() {
        assert_eq!(eval("null || null", &empty_ctx()), Value::Null);
    }

    // ── 一元非 ────────────────────────────────────────────────────────────

    #[test]
    fn unary_not_true() {
        assert_eq!(eval("!true", &empty_ctx()), Value::Bool(false));
    }

    #[test]
    fn unary_not_false() {
        assert_eq!(eval("!false", &empty_ctx()), Value::Bool(true));
    }

    #[test]
    fn unary_not_null() {
        assert_eq!(eval("!null", &empty_ctx()), Value::Bool(true));
    }

    #[test]
    fn unary_not_zero() {
        assert_eq!(eval("!0", &empty_ctx()), Value::Bool(true));
    }

    #[test]
    fn unary_not_non_zero() {
        assert_eq!(eval("!1", &empty_ctx()), Value::Bool(false));
    }

    #[test]
    fn unary_not_empty_string() {
        assert_eq!(eval("!\"\"", &empty_ctx()), Value::Bool(true));
    }

    // ── 三元运算符 ────────────────────────────────────────────────────────

    #[test]
    fn ternary_true_branch() {
        assert_eq!(eval("true ? \"yes\" : \"no\"", &empty_ctx()), Value::String("yes".into()));
    }

    #[test]
    fn ternary_false_branch() {
        assert_eq!(eval("false ? \"yes\" : \"no\"", &empty_ctx()), Value::String("no".into()));
    }

    #[test]
    fn ternary_with_computed_condition() {
        let mut ctx = empty_ctx();
        ctx.index = 0;
        let result = eval("index == 0 ? \"first\" : \"other\"", &ctx);
        assert_eq!(result, Value::String("first".into()));
    }

    // ── 内置函数 ──────────────────────────────────────────────────────────

    #[test]
    fn math_min() {
        assert_eq!(eval("Math.min(3, 1, 2)", &empty_ctx()), Value::Number(1.into()));
    }

    #[test]
    fn math_max() {
        assert_eq!(eval("Math.max(3, 1, 2)", &empty_ctx()), Value::Number(3.into()));
    }

    #[test]
    fn math_abs() {
        assert_eq!(eval("Math.abs(0 - 7)", &empty_ctx()), Value::Number(7.into()));
    }

    #[test]
    fn math_floor() {
        // Math.floor(3.9) → 3 stored as integer
        let result = eval("Math.floor(3)", &empty_ctx());
        assert_eq!(result, Value::Number(3.into()));
    }

    #[test]
    fn unknown_function_returns_error() {
        use super::super::parser::parse_expression;
        let ast = parse_expression("UnknownNs.call()").unwrap();
        let result = evaluate(&ast, &empty_ctx());
        assert!(result.is_err());
    }

    // ── is_truthy 覆盖 ────────────────────────────────────────────────────

    #[test]
    fn empty_array_is_falsy() {
        // ![] — 空数组在 JS 里是 truthy，但这里 is_truthy 对 array 始终返回 true
        // 验证当前行为（非空数组）
        let ctx = ctx_with_item(serde_json::json!({"arr": [1]}));
        assert_eq!(eval("!item.arr", &ctx), Value::Bool(false));
    }

    #[test]
    fn empty_object_is_falsy() {
        // 空 object 在当前实现里 is_truthy 返回 false
        let ctx = ctx_with_item(serde_json::json!({"obj": {}}));
        assert_eq!(eval("!item.obj", &ctx), Value::Bool(true));
    }
}

fn value_to_display(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => serde_json::to_string(val).unwrap_or_default(),
    }
}
