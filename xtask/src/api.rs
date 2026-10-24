use crate::util::{self, Args};
use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};
use syn::{Expr, ImplItem, Item, Lit, Visibility};

pub fn uncomment(source: &str) -> String {
    Regex::new(r"(?s)//[^\n]*|/\*.*?\*/")
        .unwrap()
        .replace_all(source, "")
        .into_owned()
}

pub fn headers() -> Result<String> {
    let include = util::root().join("rustfs-mimalloc-sys/c_src/mimalloc/include");
    let parts: Result<Vec<_>> = [
        "mimalloc.h",
        "mimalloc-profile.h",
        "mimalloc-stats.h",
        "mimalloc/qsbr.h",
    ]
    .iter()
    .map(|name| fs::read_to_string(include.join(name)).with_context(|| format!("read {name}")))
    .collect();
    Ok(uncomment(&parts?.join("\n")))
}

pub fn bindings() -> Result<String> {
    util::read_text(&util::root().join("rustfs-mimalloc-sys/src/lib.rs"))
}

fn integer(expr: &Expr) -> Result<i64> {
    if let Expr::Lit(value) = expr
        && let Lit::Int(number) = &value.lit
    {
        return Ok(number.base10_parse()?);
    }
    bail!("expected an integer option discriminant")
}

pub fn options(header: &str, bindings: &str) -> Result<BTreeMap<String, i64>> {
    let pattern = Regex::new(r"(?s)typedef enum mi_option_e\s*\{(.*?)\}")?;
    let body = pattern.captures(header).context("missing C option enum")?;
    let mut c_options = BTreeMap::new();
    let mut next = 0_i64;
    for entry in body[1].split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (name, expression) = entry
            .split_once('=')
            .map_or((entry, None), |(n, v)| (n.trim(), Some(v.trim())));
        if let Some(value) = expression {
            next = if let Some(number) = c_options.get(value) {
                *number
            } else {
                value.parse()?
            };
        }
        c_options.insert(name.to_owned(), next);
        next += 1;
    }
    let file = syn::parse_file(bindings)?;
    let mut rust = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    for item in file.items {
        match item {
            Item::Enum(item) if item.ident == "mi_option_t" => {
                for variant in item.variants {
                    let (_, value) = variant.discriminant.context("missing option ID")?;
                    rust.insert(variant.ident.to_string(), integer(&value)?);
                }
            }
            Item::Impl(item) => {
                if !matches!(&*item.self_ty, syn::Type::Path(p) if p.path.is_ident("mi_option_t")) {
                    continue;
                }
                for item in item.items {
                    if let ImplItem::Const(item) = item
                        && let Expr::Path(target) = item.expr
                    {
                        let target = target
                            .path
                            .segments
                            .last()
                            .context("missing alias target")?
                            .ident
                            .to_string();
                        aliases.insert(item.ident.to_string(), target);
                    }
                }
            }
            _ => {}
        }
    }
    ensure!(!rust.is_empty(), "missing Rust option enum");
    for (alias, target) in &aliases {
        let value = rust
            .get(target)
            .with_context(|| format!("unknown option alias target: {alias} -> {target}"))?;
        ensure!(
            c_options.get(alias).is_none_or(|actual| actual == value),
            "option alias ID drift: {alias}"
        );
    }
    for (name, value) in &rust {
        ensure!(
            c_options.get(name).is_none_or(|actual| actual == value),
            "option ID drift: {name}"
        );
        ensure!(
            c_options.get(name) == Some(value)
                || aliases
                    .iter()
                    .any(|(alias, target)| target == name && c_options.get(alias) == Some(value)),
            "option ID drift: {name}"
        );
    }
    let missing: Vec<_> = c_options
        .iter()
        .filter(|(name, value)| {
            name.starts_with("mi_option_")
                && !name.contains("deprecated")
                && !rust.contains_key(*name)
                && !aliases.contains_key(*name)
                && !rust.values().any(|v| v == *value)
        })
        .map(|(name, _)| name)
        .collect();
    ensure!(missing.is_empty(), "unbound active options: {missing:?}");
    Ok(rust)
}

pub fn inventory(header: &str, bindings: &str) -> Result<serde_json::Value> {
    let mut bound = BTreeSet::new();
    for item in syn::parse_file(bindings)?.items {
        match item {
            Item::Fn(item) if matches!(item.vis, Visibility::Public(_)) => {
                bound.insert(item.sig.ident.to_string());
            }
            Item::ForeignMod(item) => {
                for item in item.items {
                    if let syn::ForeignItem::Fn(item) = item
                        && matches!(item.vis, Visibility::Public(_))
                    {
                        bound.insert(item.sig.ident.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    bound.retain(|name| name.starts_with("mi_"));
    let names = Regex::new(r"\b(mi_\w+)\s*\(")?;
    let header_names: BTreeSet<_> = names
        .captures_iter(header)
        .map(|c| c[1].to_owned())
        .collect();
    let missing: Vec<_> = bound.difference(&header_names).collect();
    ensure!(
        missing.is_empty(),
        "bindings absent from public headers: {missing:?}"
    );
    let mut exported = BTreeSet::new();
    for declaration in Regex::new(r"\bmi_decl_export\b([^;{}]+);")?.captures_iter(header) {
        for name in names.captures_iter(&declaration[1]) {
            if !name[1].starts_with("mi_attr_") && !name[1].starts_with("mi_decl_") {
                exported.insert(name[1].to_owned());
            }
        }
    }
    let version = Regex::new(r"#define\s+MI_MALLOC_VERSION\s+(\d+)")?
        .captures(header)
        .context("missing MI_MALLOC_VERSION")?[1]
        .parse::<u32>()?;
    Ok(json!({
        "source_version": version,
        "bound_functions": bound,
        "unbound_exports": exported.difference(&bound).collect::<Vec<_>>(),
        "option_ids": options(header, bindings)?,
        "scope": "names and numeric option IDs; ABI and link behavior require runtime tests"
    }))
}

pub fn run(args: &mut Args) -> Result<()> {
    let destination = args.option("--json")?.map(util::path);
    args.finish()?;
    let report = inventory(&headers()?, &bindings()?)?;
    if let Some(path) = destination {
        util::write_json(&path, &report)?;
    }
    println!(
        "Public header parity passed: {} Rust functions, {} option IDs; {} C exports remain outside the binding scope",
        report["bound_functions"].as_array().unwrap().len(),
        report["option_ids"].as_object().unwrap().len(),
        report["unbound_exports"].as_array().unwrap().len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn option_alias_preserves_legacy_variant_and_rejects_numeric_drift() -> Result<()> {
        let header = headers()?;
        let source = bindings()?;
        assert_eq!(options(&header, &source)?["mi_option_guarded_precise"], 31);
        let wrong = source.replacen(
            "mi_option_guarded_precise = 31",
            "mi_option_guarded_precise = 32",
            1,
        );
        assert!(
            options(&header, &wrong)
                .unwrap_err()
                .to_string()
                .contains("option alias ID drift")
        );
        Ok(())
    }
    #[test]
    fn absent_public_header_function_fails_inventory() -> Result<()> {
        let source = format!("{}\npub unsafe fn mi_nonexistent() {{}}", bindings()?);
        assert!(
            inventory(&headers()?, &source)
                .unwrap_err()
                .to_string()
                .contains("absent from public headers")
        );
        Ok(())
    }
}
