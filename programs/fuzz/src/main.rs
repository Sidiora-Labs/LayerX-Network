use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use layerx_programs_runtime::{programs_fuzz_observation, FuzzTarget};

/// A heap ceiling far above the runtime's metered module memory bound. A lawful
/// validation, instantiation or execution of a committed corpus seed (and its
/// bounded mutations) stays well under it, so tripping the ceiling can only mean
/// an unbounded-allocation defect - which [`bounded_alloc`] turns into a
/// process abort, i.e. a build-breaking failure of the fuzz gate.
mod bounded_alloc {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Declared upper bound on the live heap the fuzz harness may hold at once.
    pub const MAX_LIVE_HEAP_BYTES: usize = 256 * 1024 * 1024;

    static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

    /// System allocator wrapper that aborts the process the moment the live heap
    /// would exceed the declared ceiling, making unbounded allocation a
    /// build-breaking defect rather than a silent memory blow-up.
    pub struct BoundingAllocator;

    #[allow(unsafe_code)]
    unsafe impl GlobalAlloc for BoundingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let previous = LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            if previous.saturating_add(layout.size()) > MAX_LIVE_HEAP_BYTES {
                std::process::abort();
            }
            System.alloc(layout)
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
            System.dealloc(ptr, layout);
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let previous = LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            if previous.saturating_add(layout.size()) > MAX_LIVE_HEAP_BYTES {
                std::process::abort();
            }
            System.alloc_zeroed(layout)
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            if new_size > layout.size() {
                let growth = new_size - layout.size();
                let previous = LIVE_BYTES.fetch_add(growth, Ordering::Relaxed);
                if previous.saturating_add(growth) > MAX_LIVE_HEAP_BYTES {
                    std::process::abort();
                }
            } else {
                LIVE_BYTES.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
            System.realloc(ptr, layout, new_size)
        }
    }
}

#[global_allocator]
static GLOBAL: bounded_alloc::BoundingAllocator = bounded_alloc::BoundingAllocator;

fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    let compact: Vec<u8> = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if !compact.len().is_multiple_of(2) {
        return Err("corpus hex has odd length".to_string());
    }
    compact
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char)
                .to_digit(16)
                .ok_or_else(|| "invalid corpus hex".to_string())?;
            let low = (pair[1] as char)
                .to_digit(16)
                .ok_or_else(|| "invalid corpus hex".to_string())?;
            u8::try_from((high << 4) | low).map_err(|error| error.to_string())
        })
        .collect()
}

fn target(name: &str) -> Result<FuzzTarget, String> {
    match name {
        "validation" => Ok(FuzzTarget::Validation),
        "instantiation" => Ok(FuzzTarget::Instantiation),
        "execution" => Ok(FuzzTarget::Execution),
        _ => Err(format!("unknown fuzz target {name}")),
    }
}

fn corpus_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(root).map_err(|error| error.to_string())?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "hex"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("empty corpus {}", root.display()));
    }
    Ok(files)
}

fn mutate(seed: &[u8]) -> Vec<Vec<u8>> {
    let mut cases = vec![seed.to_vec(), Vec::new()];
    if !seed.is_empty() {
        for index in 0..seed.len().min(64) {
            let mut flipped = seed.to_vec();
            flipped[index] ^= 0xff;
            cases.push(flipped);
        }
        cases.push(seed[..seed.len() / 2].to_vec());
    }
    cases
}

/// Runs one input through the surface twice, rejecting a non-deterministic
/// outcome. A panic or hang inside the surface fails the process directly; an
/// unbounded allocation aborts through [`bounded_alloc`]; a byte-level
/// divergence between the two observations is reported here as a build-breaking
/// non-determinism defect.
fn check(selected: FuzzTarget, input: &[u8], path: &Path) -> Result<(), String> {
    let first = programs_fuzz_observation(selected, input);
    let second = programs_fuzz_observation(selected, input);
    if first == second {
        Ok(())
    } else {
        Err(format!(
            "non-deterministic {selected:?} outcome for input derived from {}",
            path.display()
        ))
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    let name = arguments
        .next()
        .ok_or_else(|| "missing fuzz target".to_string())?;
    let corpus = arguments
        .next()
        .ok_or_else(|| "missing corpus directory".to_string())?;
    if arguments.next().is_some() {
        return Err("unexpected fuzz argument".to_string());
    }
    let selected = target(&name)?;
    for path in corpus_files(Path::new(&corpus))? {
        let source = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        let seed = decode_hex(&source)?;
        for input in mutate(&seed) {
            check(selected, &input, &path)?;
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("programs fuzz failed: {error}");
        std::process::exit(1);
    }
}
