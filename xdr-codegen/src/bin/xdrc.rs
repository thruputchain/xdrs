//! `xdrc`: compiles a `.x` file to Rust.
//!
//! Accepts the options `x23c --rust` does, so it can replace it in build
//! scripts.

use std::path::PathBuf;
use std::process::ExitCode;

use xdr_codegen::diag::{LineIndex, Severity};
use xdr_codegen::rust::Options;

const USAGE: &str = "\
usage: xdrc [OPTIONS] [-o FILE] file.x
  --help                      print usage and exit
  --version                   print version and exit
  --strict                    fail if there are any warnings
  --rust                      generate Rust (the default and only output)
  -o FILE                     write to FILE (`-` for standard output);
                              by default, file.rs in the current directory
  --recursive_type TYPE       box TYPE wherever it appears
  --rust-feature FEATURE      treat FEATURE as on, instead of gating on it
  --rust-no-feature FEATURE   treat FEATURE as off, leaving out what it gates
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("{message}");
            }
            ExitCode::FAILURE
        }
    }
}

fn usage_error(message: &str) -> String {
    format!("xdrc: {message}\n{USAGE}")
}

fn run() -> Result<(), String> {
    let mut options = Options::default();
    let mut strict = false;
    let mut output: Option<PathBuf> = None;
    let mut input: Option<PathBuf> = None;

    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let Some(text) = arg.to_str() else {
            set_input(&mut input, arg.into())?;
            continue;
        };
        let mut value = |name: &str| {
            args.next()
                .and_then(|v| v.into_string().ok())
                .ok_or_else(|| usage_error(&format!("{name} needs a value")))
        };
        match text {
            "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            "--version" => {
                println!("xdrc {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--strict" => strict = true,
            "--rust" => {}
            "--recursive_type" => {
                options.recursive_types.insert(value(text)?);
            }
            "--rust-feature" => {
                options.features_on.insert(value(text)?);
            }
            "--rust-no-feature" => {
                options.features_off.insert(value(text)?);
            }
            "-o" => output = Some(value(text)?.into()),
            "--" => {
                for rest in args.by_ref() {
                    set_input(&mut input, rest.into())?;
                }
            }
            _ if text.starts_with("-o") => output = Some(text[2..].into()),
            _ if text.starts_with('-') && text != "-" => {
                return Err(usage_error(&format!("unknown option `{text}`")));
            }
            _ => set_input(&mut input, arg.into())?,
        }
    }
    let input = input.ok_or_else(|| usage_error("no input file"))?;
    if let Some(both) = options
        .features_on
        .intersection(&options.features_off)
        .next()
    {
        return Err(usage_error(&format!("feature `{both}` is both on and off")));
    }

    let file = input.display().to_string();
    let src = std::fs::read_to_string(&input).map_err(|e| format!("{file}: {e}"))?;
    let lines = LineIndex::new(&src);
    let report = |diags: &[xdr_codegen::diag::Diagnostic]| {
        for d in diags {
            eprintln!("{}", d.display(&file, &lines));
        }
    };

    let parsed = xdr_codegen::parse(&src).map_err(|diags| {
        report(&diags);
        String::new()
    })?;
    report(&parsed.warnings);
    if strict
        && parsed
            .warnings
            .iter()
            .any(|d| d.severity == Severity::Warning)
    {
        return Err("xdrc: --strict: warnings are treated as errors".into());
    }
    let code = xdr_codegen::rust::generate(&parsed.spec, &options).map_err(|diags| {
        report(&diags);
        String::new()
    })?;

    let output = output.unwrap_or_else(|| {
        let mut name = PathBuf::from(input.file_name().unwrap_or_default());
        if name.extension().is_some_and(|e| e == "x") {
            name.set_extension("rs");
        } else {
            name.as_mut_os_string().push(".rs");
        }
        name
    });
    if output.as_os_str() == "-" {
        print!("{code}");
        return Ok(());
    }
    std::fs::write(&output, code).map_err(|e| {
        // Leave no truncated output behind for a build to take as current.
        let _ = std::fs::remove_file(&output);
        format!("{}: {e}", output.display())
    })
}

fn set_input(input: &mut Option<PathBuf>, path: PathBuf) -> Result<(), String> {
    if input.replace(path).is_some() {
        return Err(usage_error("only one input file is allowed"));
    }
    Ok(())
}
