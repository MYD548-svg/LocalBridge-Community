//! Current-user, local-only transport. Win32 framing and ACL handling follow
//! LocalBridge's existing privileged IPC; this endpoint never routes Broker control.
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_SHARE_READ, OPEN_EXISTING, ReadFile, WriteFile,
};
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};

struct OwnedHandle(HANDLE);
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct PipeInner {
    handle: OwnedHandle,
    closed: AtomicBool,
    writer: Mutex<()>,
    read_deadline: Mutex<Option<std::time::Instant>>,
    server: bool,
    peer: Mutex<Option<(OwnedHandle, File)>>,
}

#[derive(Clone)]
pub struct Pipe(Arc<PipeInner>);

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn user_sid(process: HANDLE) -> io::Result<String> {
    let mut token = null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedHandle(token);
    let mut length = 0;
    unsafe {
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut length);
    }
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    // usize alignment satisfies TOKEN_USER's pointer fields.
    let mut buffer = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
    let mut sid = null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut count = 0;
    while unsafe { *sid.add(count) } != 0 {
        count += 1;
    }
    let result = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(sid, count) });
    unsafe {
        LocalFree(sid.cast());
    }
    Ok(result)
}

pub fn pipe_name(install_id: &str) -> io::Result<String> {
    if install_id.len() != 64 || !install_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid installation identity",
        ));
    }
    Ok(format!(
        r"\\.\pipe\LocalBridge-MCP-{}-{install_id}",
        user_sid(unsafe { GetCurrentProcess() })?
    ))
}

impl Pipe {
    pub fn same_connection(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn receive_with_tick(&mut self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if self.0.closed.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "connection closed",
                ));
            }
            let mut available = 0;
            if unsafe {
                PeekNamedPipe(
                    self.0.handle.0,
                    null_mut(),
                    0,
                    null_mut(),
                    &mut available,
                    null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if available > 0 {
                return self.receive_frame(Duration::from_secs(10)).map(Some);
            }
            if std::time::Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    pub fn receive_frame(&mut self, timeout: Duration) -> io::Result<Vec<u8>> {
        *self
            .0
            .read_deadline
            .lock()
            .map_err(|_| io::Error::other("reader lock failed"))? =
            Some(std::time::Instant::now() + timeout);
        let result = super::codec::read_frame(self);
        *self
            .0
            .read_deadline
            .lock()
            .map_err(|_| io::Error::other("reader lock failed"))? = None;
        if result.is_err() {
            self.close();
        }
        result
    }
    fn from_handle(handle: HANDLE, server: bool) -> Self {
        Self(Arc::new(PipeInner {
            handle: OwnedHandle(handle),
            closed: AtomicBool::new(false),
            writer: Mutex::new(()),
            read_deadline: Mutex::new(None),
            server,
            peer: Mutex::new(None),
        }))
    }

    pub fn listen(name: &str, first: bool) -> io::Result<Self> {
        let sid = user_sid(unsafe { GetCurrentProcess() })?;
        let sddl = wide(std::ffi::OsStr::new(&format!("D:P(A;;GA;;;{sid})")));
        let mut descriptor: *mut c_void = null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut security: SECURITY_ATTRIBUTES = unsafe { zeroed() };
        security.nLength = size_of::<SECURITY_ATTRIBUTES>() as u32;
        security.lpSecurityDescriptor = descriptor;
        let name = wide(std::ffi::OsStr::new(name));
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                3 | if first {
                    FILE_FLAG_FIRST_PIPE_INSTANCE
                } else {
                    0
                },
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
                32,
                65536,
                65536,
                0,
                &security,
            )
        };
        unsafe {
            LocalFree(descriptor);
        }
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(Self::from_handle(handle, true))
    }

    pub fn accept(&self, expected: &Path, stopping: &AtomicBool) -> io::Result<()> {
        while !stopping.load(Ordering::Acquire) {
            let result = unsafe { ConnectNamedPipe(self.0.handle.0, null_mut()) };
            let code = unsafe { GetLastError() };
            if result != 0 || code == ERROR_PIPE_CONNECTED {
                self.verify_peer(expected)?;
                let mode = PIPE_READMODE_BYTE | PIPE_NOWAIT;
                if unsafe { SetNamedPipeHandleState(self.0.handle.0, &mode, null(), null()) } == 0 {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            if code == ERROR_NO_DATA {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "client disconnected",
                ));
            }
            if code != ERROR_PIPE_LISTENING {
                return Err(io::Error::last_os_error());
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "service stopped",
        ))
    }

    pub fn connect(name: &str, expected: &Path) -> io::Result<Self> {
        Self::connect_with_wait(name, expected, Duration::from_secs(5), || {})
    }

    fn connect_with_wait(
        name: &str,
        expected: &Path,
        timeout: Duration,
        mut busy: impl FnMut(),
    ) -> io::Result<Self> {
        let name = wide(std::ffi::OsStr::new(name));
        let deadline = Instant::now() + timeout;
        let handle = loop {
            let handle = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    null(),
                    OPEN_EXISTING,
                    0,
                    null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                break handle;
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_PIPE_BUSY as i32) {
                return Err(error);
            }
            busy();
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                // Retain the observed busy code for redacted Adapter diagnostics.
                return Err(io::Error::new(io::ErrorKind::TimedOut, error));
            }
            let wait_ms = remaining.as_millis().clamp(1, 200) as u32;
            if unsafe { WaitNamedPipeW(name.as_ptr(), wait_ms) } == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(ERROR_SEM_TIMEOUT as i32) {
                    return Err(error);
                }
            }
            // Another client may acquire an available instance first. Only a
            // subsequent ERROR_PIPE_BUSY opens another bounded wait; no message
            // has been sent and identity failures below are never retried.
        };
        let pipe = Self::from_handle(handle, false);
        pipe.verify_peer(expected)?;
        let mode = PIPE_READMODE_BYTE | PIPE_NOWAIT;
        if unsafe { SetNamedPipeHandleState(pipe.0.handle.0, &mode, null(), null()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(pipe)
    }

    fn verify_peer(&self, expected: &Path) -> io::Result<()> {
        let mut pid = 0;
        let ok = unsafe {
            if self.0.server {
                GetNamedPipeClientProcessId(self.0.handle.0, &mut pid)
            } else {
                GetNamedPipeServerProcessId(self.0.handle.0, &mut pid)
            }
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        let process = OwnedHandle(process);
        if user_sid(process.0)? != user_sid(unsafe { GetCurrentProcess() })? {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "unauthorized user",
            ));
        }
        let mut image = vec![0u16; 32768];
        let mut length = image.len() as u32;
        if unsafe { QueryFullProcessImageNameW(process.0, 0, image.as_mut_ptr(), &mut length) } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let image =
            PathBuf::from(String::from_utf16_lossy(&image[..length as usize])).canonicalize()?;
        let expected = expected.canonicalize()?;
        if image != expected {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "unauthorized process",
            ));
        }
        // Pin both the process and its executable; the image cannot be rewritten
        // or replaced while this peer owns a connection.
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(expected)?;
        *self
            .0
            .peer
            .lock()
            .map_err(|_| io::Error::other("peer lock failed"))? = Some((process, file));
        Ok(())
    }

    pub fn close(&self) {
        self.0.closed.store(true, Ordering::Release);
        if self.0.server {
            unsafe {
                DisconnectNamedPipe(self.0.handle.0);
            }
        }
    }

    pub fn send(&self, bytes: &[u8]) -> io::Result<()> {
        let _guard = self
            .0
            .writer
            .lock()
            .map_err(|_| io::Error::other("writer lock failed"))?;
        let mut writer = self.clone();
        let result = super::codec::write_frame(&mut writer, bytes);
        if result.is_err() {
            self.close();
        }
        result
    }
}

impl Read for Pipe {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        loop {
            if self.0.closed.load(Ordering::Acquire) {
                return Ok(0);
            }
            if self
                .0
                .read_deadline
                .lock()
                .map_err(|_| io::Error::other("reader lock failed"))?
                .is_some_and(|deadline| std::time::Instant::now() >= deadline)
            {
                self.close();
                return Err(io::Error::new(io::ErrorKind::TimedOut, "incomplete frame"));
            }
            let mut available = 0;
            if unsafe {
                PeekNamedPipe(
                    self.0.handle.0,
                    null_mut(),
                    0,
                    null_mut(),
                    &mut available,
                    null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if available > 0 {
                let mut received = 0;
                if unsafe {
                    ReadFile(
                        self.0.handle.0,
                        bytes.as_mut_ptr().cast(),
                        (bytes.len().min(available as usize)) as u32,
                        &mut received,
                        null_mut(),
                    )
                } == 0
                {
                    return Err(io::Error::last_os_error());
                }
                return Ok(received as usize);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Write for Pipe {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.closed.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "connection closed",
            ));
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if self.0.closed.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "connection closed",
                ));
            }
            let mut written = 0;
            let ok = unsafe {
                WriteFile(
                    self.0.handle.0,
                    bytes.as_ptr().cast(),
                    bytes.len().min(u32::MAX as usize) as u32,
                    &mut written,
                    null_mut(),
                )
            };
            if ok != 0 && written > 0 {
                return Ok(written as usize);
            }
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            if std::time::Instant::now() >= deadline {
                self.close();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "peer stopped reading",
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn occupied_instance() -> (String, PathBuf, Pipe, Pipe) {
        let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
        let name = pipe_name(&crate::security::random_hex(32).unwrap()).unwrap();
        let listener = Pipe::listen(&name, true).unwrap();
        let client = Pipe::connect(&name, &executable).unwrap();
        listener
            .accept(&executable, &AtomicBool::new(false))
            .unwrap();
        (name, executable, listener, client)
    }

    #[test]
    fn busy_connection_waits_for_a_new_real_instance() {
        let (name, executable, listener, first) = occupied_instance();
        let (busy_tx, busy_rx) = std::sync::mpsc::channel();
        let expected = executable.clone();
        let waiting_name = name.clone();
        let waiting = thread::spawn(move || {
            let mut first_busy = Some(busy_tx);
            Pipe::connect_with_wait(&waiting_name, &expected, Duration::from_secs(5), || {
                if let Some(sender) = first_busy.take() {
                    sender.send(()).unwrap();
                }
            })
        });
        // The second instance is withheld until the actual CreateFileW busy
        // branch has been reached; timing guesses cannot make this pass.
        busy_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let next = Pipe::listen(&name, false).unwrap();
        let stopping = Arc::new(AtomicBool::new(false));
        let server_stop = stopping.clone();
        let server_pipe = next.clone();
        let accepting = thread::spawn(move || {
            server_pipe.accept(&executable, &server_stop)?;
            server_pipe.send(b"retained-response")
        });
        let connected = waiting.join().unwrap();
        if connected.is_err() {
            stopping.store(true, Ordering::Release);
            next.close();
        }
        let accepted = accepting.join().unwrap();
        let mut second = connected.unwrap();
        accepted.unwrap();
        assert_eq!(
            second.receive_frame(Duration::from_secs(5)).unwrap(),
            b"retained-response"
        );
        second.close();
        first.close();
        next.close();
        listener.close();
    }

    #[test]
    fn persistent_busy_is_bounded_and_retains_the_observed_os_code() {
        let (name, executable, listener, first) = occupied_instance();
        let started = Instant::now();
        let error = match Pipe::connect(&name, &executable) {
            Ok(_) => panic!("occupied instance unexpectedly connected"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() >= Duration::from_secs(5));
        assert!(started.elapsed() < Duration::from_secs(8));
        assert_eq!(
            error
                .get_ref()
                .and_then(|error| error.downcast_ref::<io::Error>())
                .and_then(io::Error::raw_os_error),
            Some(ERROR_PIPE_BUSY as i32)
        );
        first.close();
        listener.close();
    }

    #[test]
    fn missing_service_and_identity_rejection_never_enter_busy_retry() {
        let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
        let missing = pipe_name(&crate::security::random_hex(32).unwrap()).unwrap();
        let mut busy_calls = 0;
        assert!(
            Pipe::connect_with_wait(&missing, &executable, Duration::from_secs(5), || {
                busy_calls += 1
            })
            .is_err()
        );
        assert_eq!(busy_calls, 0);
        let name = pipe_name(&crate::security::random_hex(32).unwrap()).unwrap();
        let listener = Pipe::listen(&name, true).unwrap();
        let rejected = executable.with_file_name("synthetic-wrong-installation.exe");
        assert!(
            Pipe::connect_with_wait(&name, &rejected, Duration::from_secs(5), || busy_calls += 1)
                .is_err()
        );
        assert_eq!(busy_calls, 0);
        listener.close();
    }
}
