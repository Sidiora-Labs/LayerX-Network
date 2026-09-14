use std::path::Path;
use std::process::Command;

pub fn command(dependencies: &Path) -> Command {
    let mut command = Command::new("rustc");
    if std::env::var("LAYERX_TEST_SANITIZER").as_deref() == Ok("thread") {
        command.arg("-Zsanitizer=thread");
        let entries: Vec<_> = std::fs::read_dir(dependencies)
            .unwrap_or_else(|error| panic!("instrumented standard library directory: {error}"))
            .map(|entry| {
                entry
                    .unwrap_or_else(|error| panic!("instrumented library entry: {error}"))
                    .path()
            })
            .collect();
        for library in [
            "std",
            "core",
            "alloc",
            "panic_unwind",
            "proc_macro",
            "test",
            "compiler_builtins",
        ] {
            let prefix = format!("lib{library}-");
            let libraries: Vec<_> = entries
                .iter()
                .filter(|path| {
                    path.file_name().is_some_and(|name| {
                        let name = name.to_string_lossy();
                        name.starts_with(&prefix) && name.ends_with(".rlib")
                    })
                })
                .collect();
            assert_eq!(
                libraries.len(),
                1,
                "one instrumented {library} library required"
            );
            command
                .arg("--extern")
                .arg(format!("{library}={}", libraries[0].display()));
        }
    }
    command
}
