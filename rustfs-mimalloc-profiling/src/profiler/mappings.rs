use crate::memory_pprof::{AllocationSample, MemoryMapping};

/// Take a fresh image snapshot at report time, never from allocator hooks.
pub(super) fn collect(samples: &[AllocationSample]) -> Vec<MemoryMapping> {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
    {
        supported::collect(samples)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "ios")))]
    {
        let _ = samples;
        Vec::new()
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
mod supported {
    use std::{collections::HashSet, path::PathBuf};

    use findshlibs::{Segment, SharedLibrary, SharedLibraryId, TargetSharedLibrary};

    use super::*;

    struct CodeSegment {
        start: u64,
        limit: u64,
        #[cfg(target_os = "linux")]
        stated_address: u64,
        file_offset: u64,
    }

    struct Image {
        #[cfg(target_os = "linux")]
        is_main: bool,
        path: PathBuf,
        build_id: String,
        segments: Vec<CodeSegment>,
    }

    pub(super) fn collect(samples: &[AllocationSample]) -> Vec<MemoryMapping> {
        let addresses: HashSet<_> = samples
            .iter()
            .flat_map(|sample| &sample.frames)
            .map(|frame| frame.address)
            .filter(|address| *address != 0)
            .collect();
        if addresses.is_empty() {
            return Vec::new();
        }
        let mut addresses: Vec<_> = addresses.into_iter().collect();
        addresses.sort_unstable();

        let mut images = Vec::new();
        let mut first = true;
        TargetSharedLibrary::each(|library| {
            let is_main = first;
            first = false;
            let segments: Vec<_> = library
                .segments()
                .filter(|segment| segment.is_code())
                .filter_map(|segment| {
                    // Older ELF loaders and negative dyld slides use wrapping biases.
                    let start = segment
                        .stated_virtual_memory_address()
                        .0
                        .wrapping_add(library.virtual_memory_bias().0)
                        as u64;
                    let limit = start.checked_add(segment.len() as u64)?;
                    if start >= limit {
                        return None;
                    }
                    let index = addresses.partition_point(|address| *address < start);
                    if !is_main && addresses.get(index).is_none_or(|address| *address >= limit) {
                        return None;
                    }
                    Some(CodeSegment {
                        start,
                        limit,
                        #[cfg(target_os = "linux")]
                        stated_address: segment.stated_virtual_memory_address().0 as u64,
                        #[cfg(target_os = "linux")]
                        file_offset: 0,
                        #[cfg(any(target_os = "macos", target_os = "ios"))]
                        file_offset: match segment {
                            findshlibs::macos::Segment::Segment32(header) => header.fileoff as u64,
                            findshlibs::macos::Segment::Segment64(header) => header.fileoff,
                        },
                    })
                })
                .collect();
            if !segments.is_empty() {
                images.push(Image {
                    #[cfg(target_os = "linux")]
                    is_main,
                    path: PathBuf::from(library.name()),
                    build_id: library.id().map(format_build_id).unwrap_or_default(),
                    segments,
                });
            }
        });

        let mut mappings = Vec::new();
        for mut image in images {
            // Disk reads stay outside the loader's enumeration lock. Verify the
            // ELF identity before borrowing offsets from a possibly replaced file.
            #[cfg(target_os = "linux")]
            if !load_elf_offsets(&mut image) {
                // pprof reserves mapping[0] for the main binary. Do not silently
                // put a shared library there when the executable is unverifiable.
                if image.is_main {
                    return Vec::new();
                }
                continue;
            }
            for segment in image.segments.drain(..) {
                mappings.push(MemoryMapping {
                    memory_start: segment.start,
                    memory_limit: segment.limit,
                    file_offset: segment.file_offset,
                    filename: image.path.to_string_lossy().into_owned(),
                    build_id: image.build_id.clone(),
                });
            }
        }
        mappings
    }

    fn format_build_id(id: SharedLibraryId) -> String {
        use std::fmt::Write;
        let bytes = id.as_bytes();
        if bytes.is_empty() {
            return id.to_string();
        }
        let mut hex = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            let _ = write!(hex, "{byte:02x}");
        }
        hex
    }

    #[cfg(target_os = "linux")]
    fn load_elf_offsets(image: &mut Image) -> bool {
        use object::{Object, ObjectSegment};

        // Images never change identity while loaded, so verified segment file
        // offsets are cached per (path, build id) across reports. A repeated
        // report then skips re-opening and re-parsing every involved image.
        if let Some(matched) = verified_offsets::apply(image) {
            return matched;
        }

        let Ok(file) = std::fs::File::open(&image.path) else {
            return false;
        };
        let cache = object::read::ReadCache::new(file);
        let Ok(object) = object::File::parse(&cache) else {
            return false;
        };
        let disk_id = object
            .build_id()
            .ok()
            .flatten()
            .map(|bytes| format_build_id(SharedLibraryId::GnuBuildId(bytes.to_vec())));
        if image.build_id.is_empty() || disk_id.as_deref() != Some(&image.build_id) {
            return false;
        }
        // The current report may reference only a subset of this image's
        // segments. Cache the verified file's complete segment offsets.
        let mut offsets = std::collections::HashMap::new();
        for segment in object.segments() {
            offsets
                .entry(segment.address())
                .or_insert_with(|| segment.file_range().0);
        }
        apply_elf_offsets(&mut image.segments, &offsets);
        if !image.segments.is_empty() {
            verified_offsets::store(image.path.clone(), image.build_id.clone(), offsets);
        }
        !image.segments.is_empty()
    }

    #[cfg(target_os = "linux")]
    fn apply_elf_offsets(
        segments: &mut Vec<CodeSegment>,
        offsets: &std::collections::HashMap<u64, u64>,
    ) {
        segments.retain_mut(|segment| {
            let Some(&offset) = offsets.get(&segment.stated_address) else {
                return false;
            };
            segment.file_offset = offset;
            true
        });
    }

    /// Process-lifetime cache of build-id-verified ELF segment file offsets.
    ///
    /// A same-build-id file replacement has an identical segment layout —
    /// GNU build ids are content hashes — so identity re-verification on
    /// disk is only required for cache misses. Entries are never evicted:
    /// growth is bounded by the number of distinct library *paths* mapped
    /// during the process lifetime (rebuilding a path overwrites its
    /// entry), so only continuous `dlopen` of unique-path libraries grows
    /// it meaningfully. A vendor that hardcodes one build id across
    /// different releases would defeat the identity assumption; entries
    /// are never stored for images without a build id, so that case always
    /// re-verifies on disk.
    #[cfg(target_os = "linux")]
    mod verified_offsets {
        use std::{collections::HashMap, path::PathBuf, sync::Mutex};

        use super::{Image, apply_elf_offsets};

        struct VerifiedImage {
            build_id: String,
            offsets: HashMap<u64, u64>,
        }

        fn entries() -> &'static Mutex<HashMap<PathBuf, VerifiedImage>> {
            use std::sync::OnceLock;
            static ENTRIES: OnceLock<Mutex<HashMap<PathBuf, VerifiedImage>>> = OnceLock::new();
            ENTRIES.get_or_init(|| Mutex::new(HashMap::new()))
        }

        pub(super) fn apply(image: &mut Image) -> Option<bool> {
            let entries = entries().lock().unwrap_or_else(|err| err.into_inner());
            let cached = entries
                .get(&image.path)
                .filter(|cached| cached.build_id == image.build_id)?;
            apply_elf_offsets(&mut image.segments, &cached.offsets);
            Some(!image.segments.is_empty())
        }

        pub(super) fn store(path: PathBuf, build_id: String, offsets: HashMap<u64, u64>) {
            let mut entries = entries().lock().unwrap_or_else(|err| err.into_inner());
            entries.insert(path, VerifiedImage { build_id, offsets });
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn build_ids_are_lowercase_unseparated_hex() {
            assert_eq!(
                format_build_id(SharedLibraryId::GnuBuildId(vec![0, 0xab, 0xff])),
                "00abff"
            );
            assert_eq!(
                format_build_id(SharedLibraryId::Uuid([0xab; 16])),
                "ab".repeat(16)
            );
        }

        #[cfg(target_os = "linux")]
        #[test]
        fn elf_cache_covers_segments_first_seen_in_later_reports() {
            use object::{Object, ObjectSegment};

            let path = std::env::current_exe().unwrap();
            let cache = object::read::ReadCache::new(std::fs::File::open(&path).unwrap());
            let object = object::File::parse(&cache).unwrap();
            let mut segments = object.segments();
            let first = segments.next().unwrap().address();
            let second = segments.find(|segment| segment.address() != first).unwrap();
            let mut image = Image {
                is_main: true,
                path,
                build_id: format_build_id(SharedLibraryId::GnuBuildId(
                    object.build_id().unwrap().unwrap().to_vec(),
                )),
                segments: vec![CodeSegment {
                    start: 0,
                    limit: 1,
                    stated_address: first,
                    file_offset: u64::MAX,
                }],
            };
            assert!(load_elf_offsets(&mut image));
            assert_ne!(image.segments[0].file_offset, u64::MAX);
            image.segments[0].stated_address = second.address();
            image.segments[0].file_offset = u64::MAX;
            assert!(load_elf_offsets(&mut image));
            assert_eq!(image.segments[0].file_offset, second.file_range().0);
            image.build_id = "not-the-loaded-build-id".into();
            assert!(!load_elf_offsets(&mut image));
        }
    }
}
