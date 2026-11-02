//! Export self-contained public headers without modifying the pinned source.

use std::{fs, io, path::Path};

pub fn qsbr(source: &str) -> io::Result<String> {
    let source = source.replace("\r\n", "\n");
    let declaration =
        "mi_decl_externc mi_decl_nodiscard mi_decl_export mi_heap_t* mi_heap_new_qsbr(void);";
    if source.matches(declaration).count() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "upstream QSBR declaration changed; review the public header correction",
        ));
    }
    // mi_decl_externc is private to mimalloc/decl.h. Use language linkage
    // directly so default C and C++ consumers need no internal headers/macros.
    if source.matches("#include <stddef.h>").count() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "QSBR include anchor changed",
        ));
    }
    let source = source.replacen(
        "#include <stddef.h>",
        "#include \"../mimalloc.h\"\n#include <stddef.h>",
        1,
    );
    Ok(source.replacen(
        declaration,
        "#ifdef __cplusplus\nextern \"C\" {\n#endif\nmi_decl_nodiscard mi_decl_export mi_heap_t* mi_heap_new_qsbr(void);\n#ifdef __cplusplus\n}\n#endif",
        1,
    ))
}

/// Prepare a disjoint export tree. Validate the correction before touching output,
/// then preserve unchanged files so downstream compiler caches remain valid.
pub fn prepare(source: &Path, destination: &Path) -> io::Result<()> {
    let source = fs::canonicalize(source)?;
    let destination = resolve_destination(destination)?;
    if destination.starts_with(&source) || source.starts_with(&destination) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "header source and destination must not overlap",
        ));
    }
    let original = source.join("mimalloc/qsbr.h");
    let corrected = qsbr(&fs::read_to_string(&original)?)?;
    copy_tree(&source, &destination, &original, corrected.as_bytes())
}

fn resolve_destination(path: &Path) -> io::Result<std::path::PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut ancestor = absolute.as_path();
    loop {
        match fs::canonicalize(ancestor) {
            Ok(mut resolved) => {
                let tail = absolute.strip_prefix(ancestor).map_err(io::Error::other)?;
                for component in tail.components() {
                    match component {
                        std::path::Component::Normal(name) => resolved.push(name),
                        std::path::Component::CurDir => {}
                        _ => {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "unresolved parent traversal in header destination",
                            ));
                        }
                    }
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or(error)?;
            }
            Err(error) => return Err(error),
        }
    }
}

fn copy_tree(
    source: &Path,
    destination: &Path,
    original: &Path,
    corrected: &[u8],
) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&path, &target, original, corrected)?;
        } else if path == original {
            write_if_changed(&target, corrected)?;
        } else {
            write_if_changed(&target, &fs::read(path)?)?;
        }
    }
    Ok(())
}

fn write_if_changed(path: &Path, content: &[u8]) -> io::Result<()> {
    match fs::read(path) {
        Ok(existing) if existing == content => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::write(path, content)
}
