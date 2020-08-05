//! [![github]](https://github.com/dtolnay/cargo-llvm-lines)&ensp;[![crates-io]](https://crates.io/crates/cargo-llvm-lines)&ensp;[![docs-rs]](https://docs.rs/cargo-llvm-lines)
//!
//! [github]: https://img.shields.io/badge/github-8da0cb?style=for-the-badge&labelColor=555555&logo=github
//! [crates-io]: https://img.shields.io/badge/crates.io-fc8d62?style=for-the-badge&labelColor=555555&logo=rust
//! [docs-rs]: https://img.shields.io/badge/docs.rs-66c2a5?style=for-the-badge&labelColor=555555&logoColor=white&logo=data:image/svg+xml;base64,PHN2ZyByb2xlPSJpbWciIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyIgdmlld0JveD0iMCAwIDUxMiA1MTIiPjxwYXRoIGZpbGw9IiNmNWY1ZjUiIGQ9Ik00ODguNiAyNTAuMkwzOTIgMjE0VjEwNS41YzAtMTUtOS4zLTI4LjQtMjMuNC0zMy43bC0xMDAtMzcuNWMtOC4xLTMuMS0xNy4xLTMuMS0yNS4zIDBsLTEwMCAzNy41Yy0xNC4xIDUuMy0yMy40IDE4LjctMjMuNCAzMy43VjIxNGwtOTYuNiAzNi4yQzkuMyAyNTUuNSAwIDI2OC45IDAgMjgzLjlWMzk0YzAgMTMuNiA3LjcgMjYuMSAxOS45IDMyLjJsMTAwIDUwYzEwLjEgNS4xIDIyLjEgNS4xIDMyLjIgMGwxMDMuOS01MiAxMDMuOSA1MmMxMC4xIDUuMSAyMi4xIDUuMSAzMi4yIDBsMTAwLTUwYzEyLjItNi4xIDE5LjktMTguNiAxOS45LTMyLjJWMjgzLjljMC0xNS05LjMtMjguNC0yMy40LTMzLjd6TTM1OCAyMTQuOGwtODUgMzEuOXYtNjguMmw4NS0zN3Y3My4zek0xNTQgMTA0LjFsMTAyLTM4LjIgMTAyIDM4LjJ2LjZsLTEwMiA0MS40LTEwMi00MS40di0uNnptODQgMjkxLjFsLTg1IDQyLjV2LTc5LjFsODUtMzguOHY3NS40em0wLTExMmwtMTAyIDQxLjQtMTAyLTQxLjR2LS42bDEwMi0zOC4yIDEwMiAzOC4ydi42em0yNDAgMTEybC04NSA0Mi41di03OS4xbDg1LTM4Ljh2NzUuNHptMC0xMTJsLTEwMiA0MS40LTEwMi00MS40di0uNmwxMDItMzguMiAxMDIgMzguMnYuNnoiPjwvcGF0aD48L3N2Zz4K

use atty::Stream::Stderr;
use clap::arg_enum;
use rustc_demangle::demangle;
use std::collections::HashMap as Map;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Child, Command, Stdio};
use structopt::StructOpt;
use tempdir::TempDir;

#[derive(StructOpt, Debug)]
#[structopt(name = "cargo-llvm-lines", bin_name = "cargo")]
enum Opt {
    #[structopt(
        name = "llvm-lines",
        author,
        about = "Print amount of lines of LLVM IR that is generated for the current project",
        setting = structopt::clap::AppSettings::AllowExternalSubcommands,
    )]
    LLVMLines {
        /// Run in a different mode that just filters some Cargo output and
        /// does nothing else.
        #[structopt(long, hidden = true)]
        filter_cargo: bool,

        /// Set the sort order to number of instantiations.
        #[structopt(
            short,
            long,
            possible_values = &SortOrder::variants(),
            case_insensitive = true,
            default_value = "lines",
        )]
        sort: SortOrder,

        // All these options are passed through to the `rustc` invocation.
        #[structopt(long, hidden = true)]
        all_features: bool,
        #[structopt(long, hidden = true)]
        bin: Option<String>,
        #[structopt(long, hidden = true)]
        features: Option<String>,
        #[structopt(long, hidden = true)]
        lib: bool,
        #[structopt(long, hidden = true)]
        manifest_path: Option<String>,
        #[structopt(long, hidden = true)]
        no_default_features: bool,
        #[structopt(short, long, hidden = true)]
        package: Option<String>,
        #[structopt(long, hidden = true)]
        profile: Option<String>,
        #[structopt(long, hidden = true)]
        release: bool,
    },
}

fn main() {
    let Opt::LLVMLines {
        filter_cargo, sort, ..
    } = Opt::from_args();

    let result = cargo_llvm_lines(filter_cargo, sort);

    process::exit(match result {
        Ok(code) => code,
        Err(err) => {
            let _ = writeln!(&mut io::stderr(), "{}", err);
            1
        }
    });
}

fn cargo_llvm_lines(filter_cargo: bool, sort_order: SortOrder) -> io::Result<i32> {
    // If `--filter-cargo` was specified, just filter the output and exit
    // early.
    if filter_cargo {
        filter_err(ignore_cargo_err);
    }

    let outdir = TempDir::new("cargo-llvm-lines").expect("failed to create tmp file");
    let outfile = outdir.path().join("crate");

    if let Err(err) = run_cargo_rustc(outfile) {
        if cfg!(windows) {
            // Running on Windows tends to fail with "System cannot find the
            // file specified. (os error 2)" even when the original command
            // succeeded and the output IR has been written to the outfile. Just
            // log the error and try to read IR anyway. Let read_llvm_ir fail in
            // the case that something really went wrong.
            eprintln!("Unsuccessful `cargo rustc` invocation: {}", err);
        } else {
            return Err(err);
        }
    }

    let ir = read_llvm_ir(outdir)?;
    count_lines(ir, sort_order);

    Ok(0)
}

fn run_cargo_rustc(outfile: PathBuf) -> io::Result<()> {
    let mut cmd = Command::new("cargo");

    // Strip out options that are for cargo-llvm-lines itself.
    let args: Vec<_> = env::args_os()
        .filter(|s| {
            !["--sort", "-s", "lines", "Lines", "copies", "Copies"]
                .contains(&s.to_string_lossy().as_ref())
        })
        .collect();
    cmd.args(&wrap_args(args.clone(), outfile.as_ref()));
    cmd.env("CARGO_INCREMENTAL", "");

    // Duplicate the original command, and insert `--filter-cargo` just after
    // the `cargo-llvm-lines` and `llvm-lines` arguments.
    //
    // Note: the `--filter-cargo` must be inserted there, rather than appended
    // to the end, so that it comes before a possible `--` arguments. Otherwise
    // it will be ignored by the recursive invocation.
    let mut filter_cargo = Vec::new();
    filter_cargo.extend(args.iter().map(OsString::as_os_str));
    filter_cargo.insert(2, OsStr::new("--filter-cargo"));

    // Filter stderr through a second invocation of `cargo-llvm-lines` that has
    // `--filter-cargo` specified so that it just does the filtering in
    // `filter_err()` above.
    let _wait = {
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stderr = child.stderr.take().ok_or(io::ErrorKind::BrokenPipe)?;

        let mut errcmd = Command::new(filter_cargo[0]);
        errcmd.args(&filter_cargo[1..]);
        errcmd.stdin(stderr);
        errcmd.stdout(Stdio::null());
        errcmd.stderr(Stdio::inherit());
        let spawn = errcmd.spawn()?;
        Wait(vec![spawn, child])
    };
    drop(_wait);

    Ok(())
}

fn read_llvm_ir(outdir: TempDir) -> io::Result<String> {
    for file in fs::read_dir(&outdir)? {
        let path = file?.path();
        if let Some(ext) = path.extension() {
            if ext == "ll" {
                let mut content = String::new();
                File::open(&path)?.read_to_string(&mut content)?;
                return Ok(content);
            }
        }
    }

    let msg = "Ran --emit=llvm-ir but did not find output IR";
    Err(io::Error::new(ErrorKind::Other, msg))
}

#[derive(Default)]
struct Instantiations {
    copies: usize,
    total_lines: usize,
}

impl Instantiations {
    fn record_lines(&mut self, lines: usize) {
        self.copies += 1;
        self.total_lines += lines;
    }
}

arg_enum! {
    #[derive(Debug)]
    enum SortOrder {
        Lines,
        Copies,
    }
}

fn count_lines(content: String, sort_order: SortOrder) {
    let mut instantiations = Map::<String, Instantiations>::new();
    let mut current_function = None;
    let mut count = 0;

    for line in content.lines() {
        if line.starts_with("define ") {
            current_function = parse_function_name(line);
        } else if line == "}" {
            if let Some(name) = current_function.take() {
                instantiations
                    .entry(name)
                    .or_insert_with(Default::default)
                    .record_lines(count);
            }
            count = 0;
        } else if line.starts_with("  ") && !line.starts_with("   ") {
            count += 1;
        }
    }

    let mut data = instantiations.into_iter().collect::<Vec<_>>();

    let mut total = Instantiations {
        copies: 0,
        total_lines: 0,
    };
    for row in data.iter() {
        total.copies += row.1.copies;
        total.total_lines += row.1.total_lines;
    }

    match sort_order {
        SortOrder::Lines => {
            data.sort_by(|a, b| {
                let key_lo = (b.1.total_lines, b.1.copies, &a.0);
                let key_hi = (a.1.total_lines, a.1.copies, &b.0);
                key_lo.cmp(&key_hi)
            });
        }
        SortOrder::Copies => {
            data.sort_by(|a, b| {
                let key_lo = (b.1.copies, b.1.total_lines, &a.0);
                let key_hi = (a.1.copies, a.1.total_lines, &b.0);
                key_lo.cmp(&key_hi)
            });
        }
    }

    let lines_width = total.total_lines.to_string().len();
    let copies_width = total.copies.to_string().len();

    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let _ = writeln!(
        handle,
        "  Lines{0:1$}    Copies{0:2$}   Function name",
        "", lines_width, copies_width,
    );
    let _ = writeln!(
        handle,
        "  -----{0:1$}    ------{0:2$}   -------------",
        "", lines_width, copies_width,
    );
    let _ = writeln!(
        handle,
        "  {0:1$} (100%)  {2:3$} (100%)  (TOTAL)",
        total.total_lines, lines_width, total.copies, copies_width,
    );
    let perc = |m, n| format!("({:3.1}%)", m as f64 / n as f64 * 100f64);
    for row in data {
        let _ = writeln!(
            handle,
            "  {0:1$} {2:<7} {3:4$} {5:<7} {6}",
            row.1.total_lines,
            lines_width,
            perc(row.1.total_lines, total.total_lines),
            row.1.copies,
            copies_width,
            perc(row.1.copies, total.copies),
            row.0,
        );
    }
}

fn parse_function_name(line: &str) -> Option<String> {
    let start = line.find('@')? + 1;
    let end = line[start..].find('(')?;
    let mangled = line[start..start + end].trim_matches('"');
    let mut name = demangle(mangled).to_string();
    if has_hash(&name) {
        let len = name.len() - 19;
        name.truncate(len);
    }
    Some(name)
}

fn has_hash(name: &str) -> bool {
    let mut bytes = name.bytes().rev();
    for _ in 0..16 {
        if !bytes.next().map(is_ascii_hexdigit).unwrap_or(false) {
            return false;
        }
    }
    bytes.next() == Some(b'h') && bytes.next() == Some(b':') && bytes.next() == Some(b':')
}

fn is_ascii_hexdigit(byte: u8) -> bool {
    byte >= b'0' && byte <= b'9' || byte >= b'a' && byte <= b'f'
}

struct Wait(Vec<Child>);

impl Drop for Wait {
    fn drop(&mut self) {
        for child in &mut self.0 {
            if let Err(err) = child.wait() {
                let _ = writeln!(&mut io::stderr(), "{}", err);
            }
        }
    }
}

// Based on https://github.com/rsolomo/cargo-check
fn wrap_args<I>(it: I, outfile: &Path) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = vec!["rustc".into()];
    let mut has_color = false;

    // Skip the `cargo-llvm-lines` and `llvm-lines` arguments.
    let mut it = it.into_iter().skip(2);
    for arg in &mut it {
        if arg == *"--" {
            break;
        }
        has_color |= arg.to_str().unwrap_or("").starts_with("--color");
        args.push(arg.into());
    }

    if !has_color {
        let color = atty::is(Stderr);
        let setting = if color { "always" } else { "never" };
        args.push(format!("--color={}", setting).into());
    }

    // The `-Cno-prepopulate-passes` means we skip LLVM optimizations, which is
    // good because (a) we count the LLVM IR lines are sent to LLVM, not how
    // many there are after optimizations run, and (b) it's faster.
    //
    // The `-Cpasses=name-anon-globals` follows on: it's required to avoid the
    // following error on some programs: "The current compilation is going to
    // use thin LTO buffers without running LLVM's NameAnonGlobals pass. This
    // will likely cause errors in LLVM. Consider adding -C
    // passes=name-anon-globals to the compiler command line."
    args.push("--".into());
    args.push("--emit=llvm-ir".into());
    args.push("-Cno-prepopulate-passes".into());
    args.push("-Cpasses=name-anon-globals".into());
    args.push("-o".into());
    args.push(outfile.into());
    args.extend(it);
    args
}

/// Print lines from stdin to stderr, skipping lines that `ignore` succeeds on.
fn filter_err(ignore: fn(&str) -> bool) -> ! {
    let mut line = String::new();
    while let Ok(n) = io::stdin().read_line(&mut line) {
        if n == 0 {
            break;
        }
        if !ignore(&line) {
            let _ = write!(&mut io::stderr(), "{}", line);
        }
        line.clear();
    }
    process::exit(0);
}

/// Match Cargo output lines that we don't want to be printed.
fn ignore_cargo_err(line: &str) -> bool {
    if line.trim().is_empty() {
        return true;
    }

    let discarded_lines = [
        "warnings emitted",
        "ignoring specified output filename because multiple outputs were \
         requested",
        "ignoring specified output filename for 'link' output because multiple \
         outputs were requested",
        "ignoring --out-dir flag due to -o flag",
        "due to multiple output types requested, the explicitly specified \
         output file name will be adapted for each output type",
        "ignoring -C extra-filename flag due to -o flag",
    ];
    for s in &discarded_lines {
        if line.contains(s) {
            return true;
        }
    }

    false
}
