use axum::response::Html;
use minijinja::{Environment, Value};

use crate::error::AppError;

pub fn render(env: &Environment, name: &str, ctx: Value) -> Result<Html<String>, AppError> {
    let tmpl = env.get_template(name).map_err(anyhow::Error::from)?;
    let s = tmpl.render(ctx).map_err(anyhow::Error::from)?;
    Ok(Html(s))
}

fn fmt_with_thousands(v: f64, decimals: usize, signed: bool) -> String {
    let sign = if signed && v >= 0.0 {
        "+"
    } else if v < 0.0 {
        "-"
    } else {
        ""
    };
    let abs = v.abs();
    let formatted = format!("{:.*}", decimals, abs);
    let parts: Vec<&str> = formatted.splitn(2, '.').collect();
    let int_part = parts[0];
    let frac_part = parts.get(1).copied().unwrap_or("");
    let bytes = int_part.as_bytes();
    let mut with_commas = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            with_commas.push(',');
        }
        with_commas.push(*b as char);
    }
    if frac_part.is_empty() {
        format!("{}{}", sign, with_commas)
    } else {
        format!("{}{}.{}", sign, with_commas, frac_part)
    }
}

pub fn build_env() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_loader(minijinja::path_loader("templates"));
    env.add_global("app_version", Value::from(env!("CARGO_PKG_VERSION")));

    env.add_filter("money", |v: f64| fmt_with_thousands(v, 2, false));
    env.add_filter("signed_money", |v: f64| fmt_with_thousands(v, 2, true));
    env.add_filter("pct2", |v: f64| {
        if v >= 0.0 {
            format!("+{:.2}", v)
        } else {
            format!("{:.2}", v)
        }
    });
    env.add_filter("fixed2", |v: f64| format!("{:.2}", v));
    env
}
