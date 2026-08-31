use std::fs;
use std::io::{ErrorKind, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct StateFile {
    directory: PathBuf,
    path: PathBuf,
}

impl StateFile {
    fn new(contents: &[u8]) -> Self {
        let state = Self::missing();
        fs::write(&state.path, contents).unwrap();
        state
    }

    fn missing() -> Self {
        let directory = create_test_dir();
        let path = directory.join("state");
        Self { directory, path }
    }

    fn arg(&self) -> &str {
        self.path.to_str().unwrap()
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> Vec<u8> {
        fs::read(&self.path).unwrap()
    }
}

impl Drop for StateFile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn create_test_dir() -> PathBuf {
    loop {
        let path = std::env::temp_dir().join(format!(
            "anewer-test-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        builder.mode(0o700);
        match builder.create(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("could not create test directory: {error}"),
        }
    }
}

fn run_anewer(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_anewer"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn assert_success(result: &Output) {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn appends_only_new_complete_lines() {
    let state = StateFile::new(b"one\ntwo");

    let result = run_anewer(&[state.arg()], b"two\nthree");

    assert_success(&result);
    assert_eq!(result.stdout, b"three\n");
    assert_eq!(state.read(), b"one\ntwo\nthree\n");
}

#[test]
fn handles_lines_across_buffer_boundaries() {
    let long_line = vec![b'x'; 16 * 1024];
    let state = StateFile::new(&long_line);
    let mut input = long_line.clone();
    input.extend_from_slice(b"\nshort");

    let result = run_anewer(&[state.arg()], &input);

    let mut expected = long_line;
    expected.extend_from_slice(b"\nshort\n");
    assert_success(&result);
    assert_eq!(result.stdout, b"short\n");
    assert_eq!(state.read(), expected);
}

#[test]
fn null_delimits_lines_and_preserves_embedded_newlines() {
    let state = StateFile::new(b"dir/a\nb\0plain");

    let result = run_anewer(&["-0", state.arg()], b"dir/a\nb\0new\nname");

    assert_success(&result);
    assert_eq!(result.stdout, b"new\nname\0");
    assert_eq!(state.read(), b"dir/a\nb\0plain\0new\nname\0");

    let long_flag = run_anewer(&["--null"], b"same\0same");
    assert_success(&long_flag);
    assert_eq!(long_flag.stdout, b"same\0");
}

#[test]
fn skip_fields_compares_stored_keys_and_prints_full_lines() {
    let state = StateFile::new(b"ERROR disk full\n");

    let result = run_anewer(
        &["--skip-fields", "1", state.arg()],
        b"2026-08-21T12:00:00Z ERROR disk full\n2026-08-21T12:01:00Z WARN cpu hot",
    );

    assert_success(&result);
    assert_eq!(result.stdout, b"2026-08-21T12:01:00Z WARN cpu hot\n");
    assert_eq!(state.read(), b"ERROR disk full\nWARN cpu hot\n");

    let repeated = run_anewer(
        &["--skip-fields", "1", state.arg()],
        b"2026-08-21T13:00:00Z WARN cpu hot",
    );
    assert_success(&repeated);
    assert!(repeated.stdout.is_empty());
}

#[test]
fn skip_fields_honors_invert() {
    let state = StateFile::new(b"ERROR disk full\n");

    let result = run_anewer(
        &["--skip-fields", "1", "-v", state.arg()],
        b"ts ERROR disk full\nts INFO fresh",
    );

    assert_success(&result);
    assert_eq!(result.stdout, b"ts ERROR disk full\n");
    assert_eq!(state.read(), b"ERROR disk full\nINFO fresh\n");
}

#[test]
fn skip_fields_supports_null_delimiter() {
    let result = run_anewer(
        &["-0", "--skip-fields", "1"],
        b"ts path\npart\0ts path\npart",
    );

    assert_success(&result);
    assert_eq!(result.stdout, b"ts path\npart\0");
}

#[test]
fn field_delimiter_extracts_exact_byte_delimited_keys() {
    let state = StateFile::new(b"payload\n");

    let result = run_anewer(
        &["--skip-fields", "2", "-F", "\t", state.arg()],
        b"ts\tINFO\tpayload\nts\tWARN\tnew payload",
    );

    assert_success(&result);
    assert_eq!(result.stdout, b"ts\tWARN\tnew payload\n");
    assert_eq!(state.read(), b"payload\nnew payload\n");
}

#[test]
fn field_delimiter_rejects_invalid_values() {
    assert!(!run_anewer(&["-F", "\t"], b"").status.success());
    assert!(!run_anewer(&["--skip-fields", "1", "-F", "::"], b"")
        .status
        .success());
    assert!(!run_anewer(&["--skip-fields", "1", "-F", "\n"], b"")
        .status
        .success());
}

#[test]
fn large_skip_count_is_bounded() {
    let count = usize::MAX.to_string();

    let result = run_anewer(&["--skip-fields", &count], b"line");

    assert_success(&result);
    assert_eq!(result.stdout, b"line\n");
}

#[test]
fn trim_applies_to_comparison_stdout_and_appended_lines() {
    let state = StateFile::new(b"  existing  \n");

    let result = run_anewer(&["-t", state.arg()], b"\texisting\t\n  new value  ");

    assert_success(&result);
    assert_eq!(result.stdout, b"new value\n");
    assert_eq!(state.read(), b"  existing  \nnew value\n");
}

#[test]
fn trim_supports_null_delimiter() {
    let result = run_anewer(&["--null", "--trim"], b"  a\nb \0a\nb");

    assert_success(&result);
    assert_eq!(result.stdout, b"a\nb\0");
}

#[test]
fn dry_run_does_not_create_file() {
    let state = StateFile::missing();

    let result = run_anewer(&["-d", state.arg()], b"new");

    assert_success(&result);
    assert_eq!(result.stdout, b"new\n");
    assert!(!state.path().exists());
}

#[test]
fn dry_run_prints_without_modifying_existing_file() {
    let state = StateFile::new(b"known\n");

    let result = run_anewer(&["-d", state.arg()], b"known\nnew");

    assert_success(&result);
    assert_eq!(result.stdout, b"new\n");
    assert_eq!(state.read(), b"known\n");
}

#[test]
fn line_buffered_flushes_before_input_ends() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_anewer"))
        .arg("--line-buffered")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut line = [0; 4];
        let result = stdout.read_exact(&mut line).map(|()| line);
        let _ = sender.send(result);
    });

    stdin.write_all(b"new\n").unwrap();
    stdin.flush().unwrap();
    let line = match receiver.recv_timeout(Duration::from_secs(5)) {
        Ok(result) => result.unwrap(),
        Err(error) => {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
            reader.join().unwrap();
            panic!("stdout was not flushed: {error}");
        }
    };

    drop(stdin);
    assert!(child.wait().unwrap().success());
    reader.join().unwrap();
    assert_eq!(line, *b"new\n");
}

#[test]
fn line_buffered_stops_at_a_closed_stdout() {
    let state = StateFile::missing();
    let mut child = Command::new(env!("CARGO_BIN_EXE_anewer"))
        .args(["--line-buffered", state.arg()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());

    let mut input = Vec::new();
    for index in 0..10_000 {
        writeln!(input, "line-{index}").unwrap();
    }
    let _ = child.stdin.take().unwrap().write_all(&input);

    assert!(child.wait().unwrap().success());
    assert_eq!(state.read(), b"line-0\n");
}
