use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, SetHandleInformation, GENERIC_READ, HANDLE, HANDLE_FLAG_INHERIT,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Globalization::{MultiByteToWideChar, CP_OEMCP};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, ResumeThread, WaitForSingleObject, CREATE_NEW_CONSOLE,
    CREATE_NO_WINDOW, CREATE_SUSPENDED, INFINITE, PROCESS_INFORMATION, STARTF_USESTDHANDLES,
    STARTUPINFOW,
};

use super::shell::Launch;
use crate::core::wide;

/// The output kept for one command; older lines are dropped.
pub const MAX_LINES: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    /// Written to stderr.
    pub error: bool,
}

/// What a command has done so far, shared with its reader threads.
#[derive(Debug)]
pub struct Run {
    pub command: String,
    pub started: Instant,
    /// Exit code and run time, once it has ended.
    pub ended: Option<(u32, Duration)>,
    pub stopped: bool,
    pub lines: VecDeque<Line>,
    pub news: bool,
}

impl Run {
    fn push(&mut self, line: Line) {
        if self.lines.len() == MAX_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
        self.news = true;
    }
}

/// A command started with no window. Its process and every process it
/// starts belong to a job that is killed when this is dropped, or when
/// WinCraft exits, so nothing is left running behind.
pub struct Process {
    job: isize,
    pub run: Arc<Mutex<Run>>,
}

impl Process {
    pub fn start(command: &str, launch: &Launch) -> Result<Process, String> {
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err("could not create a job object".to_string());
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        let run = Arc::new(Mutex::new(Run {
            command: command.to_string(),
            started: Instant::now(),
            ended: None,
            stopped: false,
            lines: VecDeque::new(),
            news: true,
        }));
        match spawn(launch, job, &run) {
            Ok(()) => Ok(Process {
                job: job as isize,
                run,
            }),
            Err(err) => {
                unsafe { CloseHandle(job) };
                Err(err)
            }
        }
    }

    pub fn running(&self) -> bool {
        self.run
            .lock()
            .map(|run| run.ended.is_none())
            .unwrap_or(false)
    }

    pub fn stop(&self) {
        if let Ok(mut run) = self.run.lock() {
            if run.ended.is_some() {
                return;
            }
            run.stopped = true;
        }
        unsafe { TerminateJobObject(self.job as HANDLE, 1) };
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.job as HANDLE) };
    }
}

fn spawn(launch: &Launch, job: HANDLE, run: &Arc<Mutex<Run>>) -> Result<(), String> {
    let inherit = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: 1,
    };
    let (out_read, out_write) = pipe(&inherit)?;
    let (err_read, err_write) = match pipe(&inherit) {
        Ok(ends) => ends,
        Err(err) => {
            close_all(&[out_read, out_write]);
            return Err(err);
        }
    };
    let null = unsafe {
        CreateFileW(
            wide("NUL").as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &inherit,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };

    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdInput = null;
    startup.hStdOutput = out_write;
    startup.hStdError = err_write;
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let mut line = wide(&launch.command_line);
    let folder = launch.current_dir.as_deref().map(wide);
    // Suspended until it is in the job, so not even its first child can
    // start outside it.
    let created = unsafe {
        CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            CREATE_NO_WINDOW | CREATE_SUSPENDED,
            std::ptr::null(),
            folder.as_ref().map_or(std::ptr::null(), |dir| dir.as_ptr()),
            &startup,
            &mut info,
        )
    } != 0;
    // The child has its own copies now; while these stay open the pipes
    // would never report the end of the output.
    close_all(&[out_write, err_write, null]);
    if !created {
        close_all(&[out_read, err_read]);
        let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        return Err(format!(
            "could not start {} (error {code})",
            launch.command_line
        ));
    }
    unsafe {
        AssignProcessToJobObject(job, info.hProcess);
        ResumeThread(info.hThread);
        CloseHandle(info.hThread);
    }

    read_in_background(out_read, false, Arc::clone(run));
    read_in_background(err_read, true, Arc::clone(run));
    let process = info.hProcess as isize;
    let waiter = Arc::clone(run);
    let started = std::thread::Builder::new()
        .name("wincraft-terminal-wait".to_string())
        .spawn(move || {
            let process = process as HANDLE;
            unsafe { WaitForSingleObject(process, INFINITE) };
            let mut code = 0u32;
            unsafe {
                GetExitCodeProcess(process, &mut code);
                CloseHandle(process);
            }
            if let Ok(mut run) = waiter.lock() {
                run.ended = Some((code, run.started.elapsed()));
                run.news = true;
            }
        });
    if started.is_err() {
        unsafe { CloseHandle(info.hProcess) };
    }
    Ok(())
}

fn pipe(inherit: &SECURITY_ATTRIBUTES) -> Result<(HANDLE, HANDLE), String> {
    let mut read: HANDLE = std::ptr::null_mut();
    let mut write: HANDLE = std::ptr::null_mut();
    if unsafe { CreatePipe(&mut read, &mut write, inherit, 0) } == 0 {
        return Err("could not create a pipe".to_string());
    }
    // Only the child's end is passed on.
    unsafe { SetHandleInformation(read, HANDLE_FLAG_INHERIT, 0) };
    Ok((read, write))
}

fn close_all(handles: &[HANDLE]) {
    for handle in handles {
        if !handle.is_null() && *handle != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(*handle) };
        }
    }
}

fn read_in_background(pipe: HANDLE, error: bool, run: Arc<Mutex<Run>>) {
    let pipe = pipe as isize;
    let started = std::thread::Builder::new()
        .name("wincraft-terminal-read".to_string())
        .spawn(move || {
            let pipe = pipe as HANDLE;
            let mut pending: Vec<u8> = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let mut got = 0u32;
                let ok = unsafe {
                    ReadFile(
                        pipe,
                        buffer.as_mut_ptr(),
                        buffer.len() as u32,
                        &mut got,
                        std::ptr::null_mut(),
                    )
                } != 0;
                if !ok || got == 0 {
                    break;
                }
                pending.extend_from_slice(&buffer[..got as usize]);
                let mut lines = Vec::new();
                while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                    let raw: Vec<u8> = pending.drain(..=end).collect();
                    lines.push(clean(&decode(&raw[..raw.len() - 1])));
                }
                push_all(&run, lines, error);
            }
            if !pending.is_empty() {
                push_all(&run, vec![clean(&decode(&pending))], error);
            }
            unsafe { CloseHandle(pipe) };
        });
    if started.is_err() {
        unsafe { CloseHandle(pipe as HANDLE) };
    }
}

fn push_all(run: &Arc<Mutex<Run>>, lines: Vec<String>, error: bool) {
    if lines.is_empty() {
        return;
    }
    if let Ok(mut run) = run.lock() {
        for text in lines {
            run.push(Line { text, error });
        }
    }
}

/// Bash and PowerShell 7 write UTF-8; cmd and Windows PowerShell write the
/// console's OEM code page when their output goes to a pipe.
fn decode(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    let needed = unsafe {
        MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        )
    };
    if needed <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide_text = vec![0u16; needed as usize];
    unsafe {
        MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            wide_text.as_mut_ptr(),
            needed,
        )
    };
    String::from_utf16_lossy(&wide_text)
}

/// Removes colour and cursor codes, and keeps only what a progress bar drew
/// last: a carriage return in the middle of a line means the line was
/// rewritten from its start.
pub fn clean(line: &str) -> String {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let line = line.rsplit('\r').next().unwrap_or(line);
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            if c == '\t' {
                out.push_str("    ");
            } else if !c.is_control() {
                out.push(c);
            }
            continue;
        }
        match chars.next() {
            // CSI: parameters, then one final byte from @ to ~.
            Some('[') => {
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            // OSC: runs to BEL or to ESC \.
            Some(']') => {
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Opens a real terminal for the user: Windows Terminal when it exists, else
/// the shell in an ordinary console window.
pub fn open_terminal(terminal_line: &str, fallback: &Launch) -> Result<(), String> {
    if create_detached(terminal_line, None, 0) {
        return Ok(());
    }
    log::info!("wt.exe did not start; opening a console window instead");
    if create_detached(
        &fallback.command_line,
        fallback.current_dir.as_deref(),
        CREATE_NEW_CONSOLE,
    ) {
        Ok(())
    } else {
        Err(format!("could not start {}", fallback.command_line))
    }
}

fn create_detached(command_line: &str, folder: Option<&str>, flags: u32) -> bool {
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let mut line = wide(command_line);
    let folder = folder.map(wide);
    let created = unsafe {
        CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            flags,
            std::ptr::null(),
            folder.as_ref().map_or(std::ptr::null(), |dir| dir.as_ptr()),
            &startup,
            &mut info,
        )
    } != 0;
    if created {
        unsafe {
            CloseHandle(info.hThread);
            CloseHandle(info.hProcess);
        }
    }
    created
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_codes_are_removed() {
        assert_eq!(clean("\u{1b}[1;32mok\u{1b}[0m done"), "ok done");
        assert_eq!(clean("\u{1b}]0;title\u{7}prompt"), "prompt");
        assert_eq!(
            clean("\u{1b}]8;;http://x\u{1b}\\link\u{1b}]8;;\u{1b}\\"),
            "link"
        );
        assert_eq!(clean("\u{1b}[2K\u{1b}[1Gplain"), "plain");
    }

    #[test]
    fn a_progress_bar_keeps_only_its_last_state() {
        assert_eq!(clean(" 10%\r 50%\r100%"), "100%");
        assert_eq!(clean("windows line\r"), "windows line");
    }

    #[test]
    fn tabs_become_spaces_and_other_controls_vanish() {
        assert_eq!(clean("a\tb\u{7}"), "a    b");
    }

    #[test]
    fn utf8_passes_through_unchanged() {
        assert_eq!(decode("привіт ✓".as_bytes()), "привіт ✓");
    }

    fn wait_for_end(process: &Process) -> Run {
        let deadline = Instant::now() + Duration::from_secs(10);
        while process.running() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        // The readers can finish a moment after the process.
        std::thread::sleep(Duration::from_millis(200));
        let run = process.run.lock().unwrap();
        Run {
            command: run.command.clone(),
            started: run.started,
            ended: run.ended,
            stopped: run.stopped,
            lines: run.lines.clone(),
            news: run.news,
        }
    }

    #[test]
    fn output_and_errors_arrive_as_separate_lines() {
        let launch =
            super::super::shell::Shell::Cmd.run("echo hello& echo oops 1>&2& exit 3", None);
        let process = Process::start("test", &launch).unwrap();
        let run = wait_for_end(&process);
        assert_eq!(run.ended.map(|(code, _)| code), Some(3));
        let texts: Vec<(&str, bool)> = run
            .lines
            .iter()
            .map(|line| (line.text.trim_end(), line.error))
            .collect();
        assert!(texts.contains(&("hello", false)), "{texts:?}");
        assert!(texts.contains(&("oops", true)), "{texts:?}");
    }

    #[test]
    fn stopping_ends_the_command_and_what_it_started() {
        let launch = super::super::shell::Shell::Cmd.run("ping -n 30 127.0.0.1", None);
        let process = Process::start("test", &launch).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let asked = Instant::now();
        process.stop();
        let run = wait_for_end(&process);
        assert!(run.stopped);
        assert!(run.ended.is_some());
        assert!(asked.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn a_full_output_drops_the_oldest_lines() {
        let mut run = Run {
            command: String::new(),
            started: Instant::now(),
            ended: None,
            stopped: false,
            lines: VecDeque::new(),
            news: false,
        };
        for index in 0..MAX_LINES + 5 {
            run.push(Line {
                text: index.to_string(),
                error: false,
            });
        }
        assert_eq!(run.lines.len(), MAX_LINES);
        assert_eq!(run.lines.front().map(|line| line.text.as_str()), Some("5"));
    }
}
