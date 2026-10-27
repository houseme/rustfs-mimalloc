use crate::{
    api,
    util::{self, Args},
};
use anyhow::{Result, bail, ensure};
use std::{
    fs,
    process::{Command, Output},
};
use syn::{FnArg, Item, Pat, ReturnType, Type, Visibility};

const LAYOUTS: [&str; 3] = [
    "mi_heap_area_t",
    "mi_profiler_sample_data_t",
    "mi_profiler_t",
];

fn c_type(kind: &Type) -> Result<String> {
    match kind {
        Type::Ptr(pointer) => {
            let pointee = c_type(&pointer.elem)?;
            if matches!(pointer.mutability, syn::PointerMutability::Mut(_)) {
                Ok(format!("{pointee}*"))
            } else if pointee.ends_with('*') {
                Ok(format!("{pointee} const*"))
            } else {
                Ok(format!("const {pointee}*"))
            }
        }
        Type::Path(path) => {
            let segment = path
                .path
                .segments
                .last()
                .ok_or_else(|| anyhow::anyhow!("missing type path"))?;
            let name = segment.ident.to_string();
            if name == "Option"
                && let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments
                && arguments.args.len() == 1
                && let syn::GenericArgument::Type(Type::Path(callback)) = &arguments.args[0]
            {
                let name = callback
                    .path
                    .get_ident()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                ensure!(
                    name.starts_with("mi_") && name.ends_with("_fun"),
                    "unsupported optional ABI type: {name}"
                );
                return Ok(format!("{name}*"));
            }
            ensure!(
                path.path.segments.len() == 1
                    && matches!(segment.arguments, syn::PathArguments::None),
                "unsupported qualified ABI type"
            );
            let mapped = match name.as_str() {
                "c_void" => "void",
                "c_char" => "char",
                "c_int" => "int",
                "c_long" => "long",
                "usize" | "size_t" => "size_t",
                "u64" => "uint64_t",
                "bool" => "bool",
                _ if name.starts_with("mi_") && name.ends_with("_t") => &name,
                _ => bail!("unsupported Rust FFI type: {name}"),
            };
            Ok(mapped.into())
        }
        _ => bail!("unsupported Rust FFI type"),
    }
}

fn result(kind: &ReturnType) -> Result<String> {
    match kind {
        ReturnType::Default => Ok("void".into()),
        ReturnType::Type(_, kind) => c_type(kind),
    }
}

fn convention(abi: Option<&syn::Abi>) -> Result<()> {
    ensure!(
        abi.and_then(|a| a.name.as_ref())
            .is_some_and(|name| name.value() == "C"),
        "unsupported calling convention"
    );
    Ok(())
}

fn parameter(kind: &Type, name: &str, callback: &str) -> Result<String> {
    if callback == "mi_deferred_free_fun" && name == "heartbeat" {
        ensure!(
            matches!(kind, Type::Path(p) if p.path.is_ident("u64")),
            "unsupported heartbeat type"
        );
        Ok("unsigned long long".into())
    } else {
        c_type(kind)
    }
}

pub fn generate(source: &str) -> Result<(String, usize, usize)> {
    let file = syn::parse_file(source)?;
    for (name, expected) in [("size_t", "size_t"), ("mi_arena_id_t", "void*")] {
        let alias = file.items.iter().find_map(|i| match i {
            Item::Type(t) if t.ident == name => Some(&t.ty),
            _ => None,
        });
        ensure!(
            alias.is_some_and(|kind| c_type(kind).is_ok_and(|kind| kind == expected)),
            "unsupported ABI alias: {name}"
        );
        if name == "size_t" {
            ensure!(
                matches!(alias.unwrap().as_ref(), Type::Path(p) if p.path.is_ident("usize")),
                "unsupported ABI alias: size_t"
            );
        }
    }
    let mut lines = vec![
        "#include \"mimalloc.h\"".to_owned(), "#include \"mimalloc-profile.h\"".into(),
        "#include \"mimalloc-stats.h\"".into(), "#include \"mimalloc/qsbr.h\"".into(), "#include <stddef.h>".into(),
        "_Static_assert(sizeof(unsigned long long) == sizeof(uint64_t), \"heartbeat width\");".into(),
        "_Static_assert(_Alignof(unsigned long long) == _Alignof(uint64_t), \"heartbeat alignment\");".into(),
    ];
    let mut callbacks = 0;
    let mut functions = Vec::new();
    for item in &file.items {
        match item {
            Item::Type(item) if item.ident.to_string().ends_with("_fun") => {
                let name = item.ident.to_string();
                let Type::FnPtr(callback) = item.ty.as_ref() else {
                    bail!("unparsed callback {name}")
                };
                convention(callback.abi.as_ref())?;
                ensure!(
                    callback.variadic.is_none(),
                    "variadic callback is unsupported"
                );
                let args: Result<Vec<_>> = callback
                    .inputs
                    .iter()
                    .map(|p| {
                        parameter(
                            &p.ty,
                            &p.name
                                .as_ref()
                                .map(|(n, _)| n.to_string())
                                .unwrap_or_default(),
                            &name,
                        )
                    })
                    .collect();
                let args = args?;
                let args = if args.is_empty() {
                    "void".into()
                } else {
                    args.join(", ")
                };
                lines.push(format!(
                    "typedef {} (*rust_{name})({args});",
                    result(&callback.output)?
                ));
                lines.push(format!("static void check_{name}({name}* actual) {{ rust_{name} expected = actual; (void)expected; }}"));
                callbacks += 1;
            }
            Item::ForeignMod(block) => {
                convention(Some(&block.abi))?;
                for item in &block.items {
                    let syn::ForeignItem::Fn(item) = item else {
                        bail!("unsupported foreign item")
                    };
                    ensure!(
                        matches!(item.vis, Visibility::Public(_)) && item.sig.variadic.is_none(),
                        "unsupported foreign declaration"
                    );
                    let mut args = Vec::new();
                    for p in &item.sig.inputs {
                        let FnArg::Typed(p) = p else {
                            bail!("foreign receiver unsupported")
                        };
                        let Pat::Ident(_) = p.pat.as_ref() else {
                            bail!("unsupported argument pattern")
                        };
                        args.push(c_type(&p.ty)?);
                    }
                    functions.push(format!(
                        "  {} (*check_{})({}) = &{}; (void)check_{};",
                        result(&item.sig.output)?,
                        item.sig.ident,
                        if args.is_empty() {
                            "void".into()
                        } else {
                            args.join(", ")
                        },
                        item.sig.ident,
                        item.sig.ident
                    ));
                }
            }
            _ => {}
        }
    }
    lines.push("static void check_functions(void) {".into());
    lines.extend(functions.iter().cloned());
    lines.push("}".into());
    for name in LAYOUTS {
        let item = file
            .items
            .iter()
            .find_map(|item| match item {
                Item::Struct(s) if s.ident == name => Some(s),
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("missing repr(C) layout: {name}"))?;
        let mut repr_c = false;
        for attribute in &item.attrs {
            if attribute.path().is_ident("repr") {
                attribute.parse_nested_meta(|meta| {
                    if meta.path.is_ident("C") {
                        repr_c = true;
                    }
                    Ok(())
                })?;
            }
        }
        ensure!(repr_c, "missing repr(C) layout: {name}");
        lines.push(format!("struct rust_{name} {{"));
        for field in &item.fields {
            let field_name = field
                .ident
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("unnamed ABI field"))?;
            if let Type::Array(array) = &field.ty {
                let syn::Expr::Lit(length) = &array.len else {
                    bail!("unsupported array length")
                };
                let syn::Lit::Int(length) = &length.lit else {
                    bail!("unsupported array length")
                };
                lines.push(format!(
                    "  {} {field_name}[{}];",
                    c_type(&array.elem)?,
                    length.base10_parse::<usize>()?
                ));
            } else {
                lines.push(format!("  {} {field_name};", c_type(&field.ty)?));
            }
        }
        lines.push("};".into());
        for measure in ["sizeof", "_Alignof"] {
            lines.push(format!("_Static_assert({measure}(struct rust_{name}) == {measure}({name}), \"{name} {measure}\");"));
        }
        for field in &item.fields {
            let field = field.ident.as_ref().unwrap();
            lines.push(format!("_Static_assert(offsetof(struct rust_{name}, {field}) == offsetof({name}, {field}), \"{name}.{field}\");"));
        }
    }
    ensure!(
        !functions.is_empty() && callbacks > 0,
        "no FFI declarations parsed"
    );
    Ok((lines.join("\n") + "\n", functions.len(), callbacks))
}

pub fn compile(probe: &str, compiler: &str) -> Result<Output> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("check.c");
    fs::write(&path, probe)?;
    let words =
        shlex::split(compiler).ok_or_else(|| anyhow::anyhow!("invalid compiler command"))?;
    let (program, args) = words
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty compiler command"))?;
    util::output(
        Command::new(program)
            .args(args)
            .args(["-std=c11", "-Werror", "-fsyntax-only", "-I"])
            .arg(util::root().join("rustfs-mimalloc-sys/c_src/mimalloc/include"))
            .arg(path),
    )
}

pub fn run(args: &mut Args) -> Result<()> {
    let cc = args.value("--cc", "cc")?;
    args.finish()?;
    let (probe, functions, callbacks) = generate(&api::bindings()?)?;
    let result = compile(&probe, &cc)?;
    ensure!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    println!(
        "C ABI projection passed: {functions} FFI functions, {callbacks} callbacks, {} repr(C) layouts",
        LAYOUTS.len()
    );
    Ok(())
}

pub fn controls(args: &mut Args) -> Result<()> {
    let cc = args.value("--cc", "cc")?;
    args.finish()?;
    let original = api::bindings()?;
    let (probe, _, _) = generate(&original)?;
    ensure!(
        compile(&probe, &cc)?.status.success(),
        "matching bindings must compile"
    );
    for (before, after, diagnostic) in [
        (
            "pub fn mi_malloc(size: size_t) -> *mut c_void;",
            "pub fn mi_malloc(size: size_t) -> c_int;",
            "check_mi_malloc",
        ),
        (
            "fn(msg: *const c_char, arg: *mut c_void);",
            "fn(msg: *mut c_char, arg: *mut c_void);",
            "check_mi_output_fun",
        ),
        (
            "pub on_alloc: Option<mi_profiler_on_alloc_fun>,\n    pub on_free: Option<mi_profiler_on_free_fun>,",
            "pub on_free: Option<mi_profiler_on_free_fun>,\n    pub on_alloc: Option<mi_profiler_on_alloc_fun>,",
            "mi_profiler_t.on_",
        ),
    ] {
        let changed = original.replacen(before, after, 1);
        ensure!(changed != original, "mutation did not match");
        let (probe, _, _) = generate(&changed)?;
        let result = compile(&probe, &cc)?;
        ensure!(
            !result.status.success()
                && String::from_utf8_lossy(&result.stderr).contains(diagnostic),
            "ABI mutation escaped: {diagnostic}"
        );
    }
    ensure!(
        generate(&original.replacen("pub type size_t = usize;", "pub type size_t = u32;", 1))
            .is_err(),
        "changed size alias escaped"
    );
    let headers = api::headers()?;
    ensure!(
        api::options(&headers, &original)?["mi_option_guarded_precise"] == 31,
        "legacy option changed"
    );
    ensure!(
        api::options(
            &headers,
            &original.replacen(
                "mi_option_guarded_precise = 31",
                "mi_option_guarded_precise = 32",
                1
            )
        )
        .is_err(),
        "option ID mutation escaped"
    );
    println!("All 7 ABI/option controls passed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crlf_checkout_preserves_multiline_mutation_controls() -> Result<()> {
        let source = api::bindings()?;
        let directory = tempfile::tempdir()?;
        let file = directory.path().join("bindings.rs");
        for newline in ["\n", "\r\n"] {
            fs::write(&file, source.replace('\n', newline))?;
            let normalized = util::read_text(&file)?;
            assert_eq!(normalized, source);
            let changed = normalized.replacen(
                "#[repr(C)]\npub struct mi_profiler_t",
                "#[repr(Rust)]\npub struct mi_profiler_t",
                1,
            );
            assert_ne!(changed, normalized, "layout mutation must actually match");
            assert!(generate(&changed).is_err());
        }
        Ok(())
    }

    #[test]
    fn abi_parser_rejects_unprojectable_signatures() -> Result<()> {
        let source = api::bindings()?;
        assert!(
            generate(&source.replacen("unsafe extern \"C\"", "unsafe extern \"system\"", 1))
                .is_err()
        );
        assert!(
            generate(&source.replacen("pub type size_t = usize;", "pub type size_t = u32;", 1))
                .is_err()
        );
        assert!(
            generate(&source.replacen(
                "#[repr(C)]\npub struct mi_profiler_t",
                "#[repr(Rust)]\npub struct mi_profiler_t",
                1
            ))
            .is_err()
        );
        Ok(())
    }
}
