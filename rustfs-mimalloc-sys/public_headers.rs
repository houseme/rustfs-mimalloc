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

pub fn prepare(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            prepare(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    let header = source.join("mimalloc/qsbr.h");
    // Only the include-tree root contains this path.
    if header.is_file() {
        fs::write(
            destination.join("mimalloc/qsbr.h"),
            qsbr(&fs::read_to_string(header)?)?,
        )?;
    }
    Ok(())
}
