//! Process creation and supervision using Windows Job Objects.
//!
//! Every monitored child is started with
//! `CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP` and immediately assigned to a
//! Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. This binds
//! the child's entire process tree to the core's lifetime: if Kandel exits, all
//! monitored processes are guaranteed to terminate with it.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, SetHandleInformation, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT, STILL_ACTIVE,
    WAIT_OBJECT_0,
};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::ReadFile;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, ResumeThread, TerminateProcess, WaitForSingleObject,
    CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTF_USESTDHANDLES, STARTUPINFOW,
};

use kandel_config::App;

use super::cmdline;
use super::SendHandle;

/// Sink invoked for each captured output line: `(stream, text)`. The stream is
/// one of the `STREAM_*` names in [`crate::logs`].
pub type LineSink = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Longest single captured line before it is flushed as-is. Guards against a
/// process that emits an enormous newline-free run.
const MAX_CAPTURE_LINE: usize = 64 * 1024;

/// A Job Object handle. Dropping it closes the handle, which — because the job
/// was created with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` — terminates the
/// entire process tree bound to it.
#[derive(Debug)]
pub struct JobObject(HANDLE);

impl JobObject {
    /// Creates a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
    pub fn new() -> Result<Self> {
        let handle =
            unsafe { CreateJobObjectW(None, PCWSTR::null()) }.context("CreateJobObject")?;
        let job = JobObject(handle);

        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
        .context("SetInformationJobObject")?;

        Ok(job)
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

// SAFETY: kernel object handles are valid across threads within a process.
unsafe impl Send for JobObject {}
// SAFETY: see above.
unsafe impl Sync for JobObject {}

/// A running child process assigned to a Job Object.
#[derive(Debug)]
pub struct ChildProcess {
    /// Process ID of the root process.
    pub pid: u32,
    process_handle: HANDLE,
    /// The job owning this process tree. Dropping it kills the tree.
    pub job: JobObject,
    /// Reader threads draining the child's captured output, if any.
    capture_threads: Vec<JoinHandle<()>>,
}

impl ChildProcess {
    /// Waits (bounded) for the capture reader threads to drain and exit.
    ///
    /// A no-op when output is not being captured. Called after the process has
    /// exited or been stopped, so the UI log is complete before the next
    /// session. The bounded wait means a grandchild keeping the pipe open can
    /// never hang the monitor thread.
    pub fn join_capture(&mut self) {
        let deadline = Instant::now() + Duration::from_millis(500);
        while !self.capture_threads.is_empty() && Instant::now() < deadline {
            self.capture_threads.retain(|t| !t.is_finished());
            if !self.capture_threads.is_empty() {
                thread::sleep(Duration::from_millis(10));
            }
        }
        // Any thread still running is left to finish on its own; it holds only
        // the pipe handle and its sink, both of which are safe to outlive the
        // monitor.
        self.capture_threads.clear();
    }

    /// Waits up to `timeout` for the process to exit.
    ///
    /// Returns `true` if the process exited within the window. A `None` timeout
    /// waits indefinitely.
    pub fn wait_for_exit(&self, timeout: Option<Duration>) -> bool {
        let millis = match timeout {
            None => u32::MAX, // INFINITE
            Some(d) => d.as_millis().min(u32::MAX as u128) as u32,
        };
        // WAIT_OBJECT_0 means the process exited; WAIT_TIMEOUT means it is still
        // running; any other value is treated as not-yet-exited.
        unsafe { WaitForSingleObject(self.process_handle, millis) == WAIT_OBJECT_0 }
    }

    /// Terminates the process with exit code 1, tolerating an already-exited
    /// process, then waits for it to be fully reaped.
    pub fn terminate(&self) -> Result<()> {
        unsafe {
            let _ = TerminateProcess(self.process_handle, 1);
        }
        let _ = self.wait_for_exit(None);
        Ok(())
    }

    /// Returns the process's exit code once it has exited.
    ///
    /// Returns `None` while the process is still running (Windows reports
    /// `STILL_ACTIVE` until then). The caller normally `wait_for_exit` first.
    ///
    /// This is what distinguishes a graceful stop from a forced one:
    /// [`terminate`](Self::terminate) always records exit code 1, so any other
    /// code means the process ended on its own terms.
    pub fn exit_code(&self) -> Result<Option<u32>> {
        let mut code = 0u32;
        unsafe { GetExitCodeProcess(self.process_handle, &mut code) }
            .context("GetExitCodeProcess")?;
        // `STILL_ACTIVE` is a sentinel, not a real exit code, so map it away.
        if code == STILL_ACTIVE.0 as u32 {
            Ok(None)
        } else {
            Ok(Some(code))
        }
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.process_handle);
        }
    }
}

/// The fully resolved launch parameters for an app.
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    /// Absolute path to the executable.
    pub program: String,
    /// Resolved arguments.
    pub args: Vec<String>,
    /// Working directory, or `None` for the process default.
    pub cwd: Option<String>,
    /// Whether to launch with a hidden window.
    pub hide_window: bool,
    /// Environment variables to set on the child, as resolved `(name, value)`
    /// pairs. Empty means the child inherits the core's environment unchanged.
    pub env: Vec<(String, String)>,
    /// Whether to redirect the child's stdout/stderr into the log.
    ///
    /// This is purely a launch concern and never alters `program`, `args`,
    /// `env`, or `cwd`.
    pub capture_output: bool,
}

impl LaunchSpec {
    /// Resolves an [`App`] into a concrete launch spec.
    pub fn from_app(app: &App) -> Result<Self> {
        let program = app
            .resolved_command()
            .map_err(|e| anyhow!("{e}"))?
            .to_string_lossy()
            .into_owned();
        let args = app.resolved_args().map_err(|e| anyhow!("{e}"))?;
        let cwd = app.resolved_cwd().map_err(|e| anyhow!("{e}"))?;
        let cwd = if cwd.as_os_str().is_empty() {
            None
        } else {
            Some(cwd.to_string_lossy().into_owned())
        };
        let env = app.resolved_env().map_err(|e| anyhow!("{e}"))?;
        Ok(LaunchSpec {
            program,
            args,
            cwd,
            hide_window: app.hide_window,
            env,
            capture_output: app.capture_output,
        })
    }
}

/// Spawns `spec` suspended, assigns it to a fresh Job Object, then resumes it.
///
/// When `spec.capture_output` is set and a `sink` is supplied, the child's
/// stdout and stderr are redirected into pipes and drained by reader threads
/// that invoke `sink(stream, line)`. Capture only touches the output handles:
/// `program`, `args`, `env`, and `cwd` are launched exactly as configured.
pub fn spawn_with_job_object(spec: &LaunchSpec, sink: Option<LineSink>) -> Result<ChildProcess> {
    let job = JobObject::new()?;

    let command_line = cmdline::build_command_line(&spec.program, &spec.args);
    let mut command_wide: Vec<u16> = command_line
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let cwd_wide = spec.cwd.as_deref().filter(|s| !s.is_empty()).map(wide_null);

    // A configured env block is applied *on top of* the inherited environment,
    // so the child keeps its normal `PATH`, `SystemRoot`, and so on while gaining
    // the variables the user set. `CreateProcessW` wants a double-NUL-terminated
    // `NAME=VALUE\0` block plus `CREATE_UNICODE_ENVIRONMENT`.
    let env_block = if spec.env.is_empty() {
        None
    } else {
        let entries = merge_env(parent_env_entries(), &spec.env);
        Some(build_env_block(&entries))
    };
    let mut creation_flags = CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP;
    let env_ptr = match env_block.as_deref().filter(|b| !b.is_empty()) {
        Some(block) => {
            creation_flags |= CREATE_UNICODE_ENVIRONMENT;
            block.as_ptr()
        }
        None => std::ptr::null(),
    };

    let mut si = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    if spec.hide_window {
        si.dwFlags |= STARTF_USESHOWWINDOW;
        // SW_HIDE == 0
        si.wShowWindow = 0;
    }

    // The core is a GUI-subsystem process with no console, so without this a
    // child inherits "no console" — and any program that needs one refuses to
    // run (`timeout.exe`, for instance, prints "Input redirection is not
    // supported" and exits at once). `CREATE_NO_WINDOW` gives the child its own
    // console with **no visible window**, which is exactly right for a
    // background supervisor: console tools work, and a GUI app still shows its
    // own windows normally.
    creation_flags |= CREATE_NO_WINDOW;

    let capture = if spec.capture_output { sink } else { None };
    let mut capture_ctx = None;
    let mut inherits = false;

    if let Some(sink) = capture {
        // Only stdout and stderr are redirected; stdin is left **unset** in
        // `STARTUPINFOW` (a NULL handle). That is deliberate and load-bearing:
        //
        //   * Console tools reject a *piped* stdin (`timeout.exe`: "Input
        //     redirection is not supported"). A NULL stdin is not a pipe, so
        //     with the console from `CREATE_NO_WINDOW` they run normally.
        //   * A program that reads stdin does not see a handler that reports an
        //     immediate EOF the way an explicit `NUL` file would; it inherits the
        //     child's console input, so an interactive reader is not torn down.
        //
        // Redirecting stdin to a private pipe broke console tools; redirecting
        // it to `NUL` broke stdin readers. Leaving it NULL is the middle path
        // that keeps both working while stdout/stderr are still captured.
        let (out_read, out_write) = create_capture_pipe()?;
        let (err_read, err_write) = create_capture_pipe()?;
        let handles = CaptureHandles {
            out_write: SendHandle(out_write),
            err_write: SendHandle(err_write),
        };

        si.dwFlags |= STARTF_USESTDHANDLES;
        si.hStdInput = HANDLE::default();
        si.hStdOutput = out_write;
        si.hStdError = err_write;
        inherits = true;
        capture_ctx = Some((handles, out_read, err_read, sink));
    }

    let mut pi = PROCESS_INFORMATION::default();
    let spawn_result = unsafe {
        CreateProcessW(
            PCWSTR::null(),
            Some(PWSTR(command_wide.as_mut_ptr())),
            None,
            None,
            inherits,
            creation_flags,
            Some(env_ptr as *const std::ffi::c_void),
            cwd_wide
                .as_ref()
                .map_or(PCWSTR::null(), |w| PCWSTR(w.as_ptr())),
            &si,
            &mut pi,
        )
    };

    if let Err(e) = spawn_result {
        // Close the parent-side handles we opened, so a failed launch leaks
        // nothing.
        if let Some((handles, out_read, err_read, _)) = capture_ctx.take() {
            close_parent_handles(&handles);
            unsafe {
                let _ = CloseHandle(out_read);
                let _ = CloseHandle(err_read);
            }
        }
        return Err(anyhow::Error::new(e).context("CreateProcess"));
    }

    // In the parent, the child's write ends are no longer needed. Closing them
    // here is what lets the reader threads observe EOF once the child (and any
    // descendant sharing the handles) exits.
    let mut capture_threads = Vec::new();
    if let Some((handles, out_read, err_read, sink)) = capture_ctx.take() {
        let out_sink = Arc::clone(&sink);
        let err_sink = Arc::clone(&sink);
        // `HANDLE` is not `Send`; the `SendHandle` wrapper carries it across the
        // thread boundary, which is sound because kernel handles are
        // process-wide. Each thread owns its read end and closes it on exit.
        let out_read = SendHandle(out_read);
        let err_read = SendHandle(err_read);
        capture_threads.push(
            thread::Builder::new()
                .name("kandel-capture-out".into())
                .spawn(move || {
                    read_pipe_lines(out_read.raw(), crate::logs::STREAM_STDOUT, &out_sink);
                    unsafe {
                        let _ = CloseHandle(out_read.raw());
                    }
                })
                .context("spawning stdout capture thread")?,
        );
        capture_threads.push(
            thread::Builder::new()
                .name("kandel-capture-err".into())
                .spawn(move || {
                    read_pipe_lines(err_read.raw(), crate::logs::STREAM_STDERR, &err_sink);
                    unsafe {
                        let _ = CloseHandle(err_read.raw());
                    }
                })
                .context("spawning stderr capture thread")?,
        );
        unsafe {
            let _ = CloseHandle(handles.out_write.raw());
            let _ = CloseHandle(handles.err_write.raw());
        }
    }

    // Assign to the job before resuming, so children spawned after resume are
    // also captured by the job.
    if let Err(e) = unsafe { AssignProcessToJobObject(job.0, pi.hProcess) } {
        unsafe {
            let _ = TerminateProcess(pi.hProcess, 1);
            let _ = CloseHandle(pi.hThread);
            let _ = CloseHandle(pi.hProcess);
        }
        return Err(anyhow::Error::new(e).context("AssignProcessToJobObject"));
    }

    if unsafe { ResumeThread(pi.hThread) } == u32::MAX {
        unsafe {
            let _ = TerminateProcess(pi.hProcess, 1);
            let _ = CloseHandle(pi.hThread);
            let _ = CloseHandle(pi.hProcess);
        }
        return Err(anyhow!("ResumeThread failed"));
    }

    unsafe {
        let _ = CloseHandle(pi.hThread);
    }

    Ok(ChildProcess {
        pid: pi.dwProcessId,
        process_handle: pi.hProcess,
        job,
        capture_threads,
    })
}

/// Handles the parent owns while a capture child is being launched.
struct CaptureHandles {
    out_write: SendHandle,
    err_write: SendHandle,
}

/// Closes the parent's copy of the launch handles after `CreateProcessW` has
/// been attempted. Used on the failure path to avoid leaking handles.
fn close_parent_handles(handles: &CaptureHandles) {
    unsafe {
        let _ = CloseHandle(handles.out_write.raw());
        let _ = CloseHandle(handles.err_write.raw());
    }
}

/// Creates an anonymous pipe whose read end is private to the parent.
///
/// Both ends are created inheritable (required for `STARTF_USESTDHANDLES`), then
/// the read end is marked non-inheritable so only the child's write end leaks
/// into the child.
fn create_capture_pipe() -> Result<(HANDLE, HANDLE)> {
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    let mut read = HANDLE::default();
    let mut write = HANDLE::default();
    unsafe { CreatePipe(&mut read, &mut write, Some(&sa), 0) }.context("CreatePipe")?;
    unsafe { SetHandleInformation(read, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)) }
        .context("clearing inherit on pipe read end")?;
    Ok((read, write))
}

/// Drains a pipe until EOF, invoking `sink` once per line.
fn read_pipe_lines(handle: HANDLE, stream: &'static str, sink: &LineSink) {
    let mut buf = [0u8; 8192];
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let mut read = 0u32;
        let result = unsafe { ReadFile(handle, Some(&mut buf), Some(&mut read), None) };
        if let Err(e) = result {
            // `ERROR_BROKEN_PIPE` (109) is the normal end of a pipe; anything
            // else is unexpected but still ends the drain.
            if e.code().0 as u32 & 0xFFFF != 109 {
                tracing::debug!(component = "capture", stream, error = %e, "pipe read ended");
            }
            break;
        }
        if read == 0 {
            break;
        }
        pending.extend_from_slice(&buf[..read as usize]);
        flush_complete_lines(&mut pending, stream, sink);
    }
    // Flush a trailing line that had no newline.
    if !pending.is_empty() {
        let text = String::from_utf8_lossy(&pending)
            .trim_end_matches(['\r', '\n'])
            .to_string();
        if !text.is_empty() {
            sink(stream, &text);
        }
    }
}

/// Emits every complete (newline-terminated) line currently in `pending`,
/// leaving any trailing partial line in place. A run with no newline longer
/// than [`MAX_CAPTURE_LINE`] is flushed as a line of its own.
fn flush_complete_lines(pending: &mut Vec<u8>, stream: &str, sink: &LineSink) {
    let mut start = 0usize;
    let mut i = 0usize;
    while i < pending.len() {
        if pending[i] == b'\n' {
            let line = String::from_utf8_lossy(&pending[start..i])
                .trim_end_matches('\r')
                .to_string();
            sink(stream, &line);
            start = i + 1;
        }
        i += 1;
    }
    if start > 0 {
        pending.drain(..start);
    }
    if pending.len() > MAX_CAPTURE_LINE {
        let line = String::from_utf8_lossy(pending).to_string();
        sink(stream, &line);
        pending.clear();
    }
}

/// Converts a path to a NUL-terminated UTF-16 buffer.
fn wide_null(s: &str) -> Vec<u16> {
    Path::new(s)
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// The parent process's environment as ordered `(name, value)` pairs.
fn parent_env_entries() -> Vec<(String, String)> {
    std::env::vars_os()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .collect()
}

/// Overlays `overrides` onto `base`, returning the merged set.
///
/// Windows environment variable names are case-insensitive, so an override
/// replaces any existing entry that matches ignoring case, keeping the base
/// position rather than appending a duplicate. New names append in order.
pub fn merge_env(
    base: Vec<(String, String)>,
    overrides: &[(String, String)],
) -> Vec<(String, String)> {
    let mut merged = base;
    for (name, value) in overrides {
        if name.is_empty() {
            continue;
        }
        match merged
            .iter()
            .position(|(k, _)| k.eq_ignore_ascii_case(name))
        {
            Some(pos) => merged[pos] = (name.clone(), value.clone()),
            None => merged.push((name.clone(), value.clone())),
        }
    }
    merged
}

/// Encodes `entries` as a `CREATE_UNICODE_ENVIRONMENT` block.
///
/// The result is a sequence of `NAME=VALUE` entries, each UTF-16 and
/// NUL-terminated, closed by a second NUL. A name or value containing an
/// interior NUL is skipped, since it cannot be represented.
fn build_env_block(entries: &[(String, String)]) -> Vec<u16> {
    let mut block: Vec<u16> = Vec::new();
    for (name, value) in entries {
        if name.is_empty() {
            continue;
        }
        let entry = format!("{name}={value}");
        let encoded: Vec<u16> = entry.encode_utf16().collect();
        if encoded.contains(&0) {
            continue;
        }
        block.extend_from_slice(&encoded);
        block.push(0);
    }
    block.push(0);
    block
}

/// Waits for either the process to exit or `stop` to be signalled.
///
/// Implemented with a polling loop so it can observe a shared atomic stop flag
/// without needing a Win32 event handle per wait.
///
/// Returns `true` if the process exited on its own, `false` if `stop` was set.
pub fn wait_for_exit_or_stop(child: &ChildProcess, stop: &std::sync::atomic::AtomicBool) -> bool {
    use std::sync::atomic::Ordering;

    loop {
        if stop.load(Ordering::SeqCst) {
            return false;
        }
        // Poll in short slices to stay responsive to the stop flag.
        if child.wait_for_exit(Some(Duration::from_millis(100))) {
            return true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_env_block_double_terminates() {
        let block = build_env_block(&[("A".into(), "1".into())]);
        // `A=1` then a NUL terminator, then the closing NUL.
        assert_eq!(block, vec![65, 61, 49, 0, 0]);
    }

    #[test]
    fn merge_env_overrides_case_insensitively_and_appends() {
        let base = vec![
            ("Path".to_string(), "C:\\Windows".to_string()),
            ("HOME".to_string(), "C:\\Users\\me".to_string()),
        ];
        let overrides = vec![
            ("PATH".to_string(), "C:\\tools".to_string()),
            ("PORT".to_string(), "3000".to_string()),
        ];
        let merged = merge_env(base, &overrides);
        assert_eq!(merged[0], ("PATH".to_string(), "C:\\tools".to_string()));
        assert_eq!(merged[1], ("HOME".to_string(), "C:\\Users\\me".to_string()));
        assert_eq!(merged[2], ("PORT".to_string(), "3000".to_string()));
    }

    #[test]
    fn build_env_block_skips_interior_nul() {
        let block = build_env_block(&[("BAD".into(), "a\0b".into()), ("OK".into(), "1".into())]);
        let text = String::from_utf16_lossy(&block);
        assert_eq!(text, "OK=1\0\0");
    }

    #[test]
    fn capture_on_runs_a_console_tool_that_rejects_piped_stdin() {
        // The regression this guards. `timeout.exe` refuses to run without a
        // console and with a piped stdin (`ERROR: Input redirection is not
        // supported`), exiting 1. The core is a GUI process with no console, so
        // the child must be given its own console (`CREATE_NO_WINDOW`) and a
        // non-pipe stdin. With both, `timeout` runs and exits 0.
        let spec = LaunchSpec {
            program: "cmd.exe".into(),
            args: vec!["/c".into(), r"C:\Windows\System32\timeout.exe /t 1".into()],
            cwd: None,
            hide_window: true,
            env: Vec::new(),
            capture_output: true,
        };
        let sink: LineSink = Arc::new(|_, _| {});
        let mut child = spawn_with_job_object(&spec, Some(sink)).expect("spawn");
        assert!(child.wait_for_exit(Some(Duration::from_secs(10))));
        child.join_capture();
        assert_eq!(
            child.exit_code().unwrap(),
            Some(0),
            "timeout must run under capture, not fail on a redirected stdin"
        );
    }

    #[test]
    fn spawn_captures_stdout_and_stderr() {
        use std::sync::{Arc, Mutex};

        let lines = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
        let sink_lines = Arc::clone(&lines);
        let sink: LineSink = Arc::new(move |stream: &str, text: &str| {
            sink_lines
                .lock()
                .unwrap()
                .push((stream.to_string(), text.to_string()));
        });

        let spec = LaunchSpec {
            program: "cmd.exe".into(),
            args: vec![
                "/c".into(),
                "echo out-line & echo err-line 1>&2 & exit 0".into(),
            ],
            cwd: None,
            hide_window: true,
            env: Vec::new(),
            capture_output: true,
        };
        let mut child = spawn_with_job_object(&spec, Some(sink)).expect("spawn");
        assert!(child.wait_for_exit(Some(Duration::from_secs(10))));
        child.join_capture();

        let got = lines.lock().unwrap();
        assert!(
            got.iter()
                .any(|(s, t)| s == crate::logs::STREAM_STDOUT && t.trim() == "out-line"),
            "stdout line missing: {got:?}"
        );
        assert!(
            got.iter()
                .any(|(s, t)| s == crate::logs::STREAM_STDERR && t.trim() == "err-line"),
            "stderr line missing: {got:?}"
        );
    }

    #[test]
    fn spawn_injects_custom_environment() {
        // `cmd /c exit %VAR%` returns the value of the injected variable. The
        // child only sees "7" if the override reached it.
        let spec = LaunchSpec {
            program: "cmd.exe".into(),
            args: vec!["/c".into(), "exit %KANDEL_INJECTED%".into()],
            cwd: None,
            hide_window: true,
            env: vec![("KANDEL_INJECTED".into(), "7".into())],
            capture_output: false,
        };
        let child = spawn_with_job_object(&spec, None).expect("spawn");
        assert!(child.wait_for_exit(Some(Duration::from_secs(10))));
        assert_eq!(child.exit_code().unwrap(), Some(7));
    }

    #[test]
    fn spawn_inherits_parent_environment_when_overriding() {
        // A configured variable must not wipe the inherited environment: the
        // child still sees `SystemRoot`, which only the parent provides. A
        // replace-not-merge implementation would drop it and exit 5.
        assert!(std::env::var_os("SystemRoot").is_some());
        let spec = LaunchSpec {
            program: "cmd.exe".into(),
            args: vec![
                "/c".into(),
                "if defined SystemRoot (exit 0) else (exit 5)".into(),
            ],
            cwd: None,
            hide_window: true,
            env: vec![("KANDEL_INJECTED".into(), "0".into())],
            capture_output: false,
        };
        let child = spawn_with_job_object(&spec, None).expect("spawn");
        assert!(child.wait_for_exit(Some(Duration::from_secs(10))));
        assert_eq!(child.exit_code().unwrap(), Some(0));
    }

    #[test]
    fn launch_spec_resolves_relative_paths() {
        // Use a command that exists on the test host.
        let exe = std::env::current_exe().unwrap();
        let app = App {
            command: exe.to_string_lossy().into_owned(),
            ..App::default()
        };
        let spec = LaunchSpec::from_app(&app).unwrap();
        assert!(spec.program.ends_with(".exe"));
        assert!(Path::new(&spec.program).is_absolute());
    }

    #[test]
    fn launch_spec_resolves_env_pairs() {
        let app = App {
            command: std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            env: vec![
                kandel_config::EnvVar {
                    key: "FOO".into(),
                    value: "bar".into(),
                },
                kandel_config::EnvVar {
                    key: "BAZ".into(),
                    value: "qux".into(),
                },
            ],
            ..App::default()
        };
        let spec = LaunchSpec::from_app(&app).unwrap();
        assert_eq!(
            spec.env,
            [
                ("FOO".to_string(), "bar".to_string()),
                ("BAZ".to_string(), "qux".to_string())
            ]
        );
    }

    #[test]
    fn spawn_and_wait_for_fast_process() {
        // `cmd.exe /c exit 0` should exit immediately.
        let spec = LaunchSpec {
            program: "cmd.exe".into(),
            args: vec!["/c".into(), "exit 0".into()],
            cwd: None,
            hide_window: true,
            env: Vec::new(),
            capture_output: false,
        };
        let child = spawn_with_job_object(&spec, None).expect("spawn");
        assert!(child.pid > 0);
        let exited = child.wait_for_exit(Some(Duration::from_secs(10)));
        assert!(exited, "child should exit on its own");
    }

    #[test]
    fn kill_on_job_close_terminates_tree() {
        // Start a long-running process, then drop the ChildProcess (closing the
        // job) and confirm it is gone.
        let spec = LaunchSpec {
            program: "cmd.exe".into(),
            args: vec!["/c".into(), "ping -n 60 127.0.0.1 > nul".into()],
            cwd: None,
            hide_window: true,
            env: Vec::new(),
            capture_output: false,
        };
        let child = spawn_with_job_object(&spec, None).expect("spawn");
        let pid = child.pid;
        assert!(!child.wait_for_exit(Some(Duration::from_millis(200))));

        // Dropping the job handle with KILL_ON_JOB_CLOSE must terminate the tree.
        drop(child);

        // Give the kernel a moment, then assert the process is not running.
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            !process_is_running(pid),
            "process {pid} should be terminated"
        );
    }

    #[test]
    fn exit_code_is_readable_and_terminate_reports_one() {
        // Pins the contract the graceful-stop tests rely on: a process that runs
        // to completion reports its own code, while `terminate` records 1.
        let spec = LaunchSpec {
            program: "cmd.exe".into(),
            args: vec!["/c".into(), "exit 7".into()],
            cwd: None,
            hide_window: true,
            env: Vec::new(),
            capture_output: false,
        };
        let child = spawn_with_job_object(&spec, None).expect("spawn");
        assert_eq!(child.exit_code().unwrap(), None, "still running");
        assert!(child.wait_for_exit(Some(Duration::from_secs(10))));
        assert_eq!(child.exit_code().unwrap(), Some(7));
    }

    #[test]
    fn terminate_records_exit_code_one() {
        let spec = LaunchSpec {
            program: "cmd.exe".into(),
            args: vec!["/c".into(), "ping -n 60 127.0.0.1 > nul".into()],
            cwd: None,
            hide_window: true,
            env: Vec::new(),
            capture_output: false,
        };
        let child = spawn_with_job_object(&spec, None).expect("spawn");
        child.terminate().expect("terminate");
        assert_eq!(child.exit_code().unwrap(), Some(1));
    }

    /// Returns true if a process with `pid` is currently running.
    fn process_is_running(pid: u32) -> bool {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        unsafe {
            match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                Ok(handle) => {
                    let _ = CloseHandle(handle);
                    true
                }
                Err(_) => false,
            }
        }
    }
}
