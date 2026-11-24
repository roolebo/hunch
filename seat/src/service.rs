//! The service and its clients. A client hands the service its stdin,
//! stdout and stderr over the socket, then its arguments; the
//! service runs the command on those, so it reads and writes them
//! directly, and answers with the command's exit status.

use crate::error::{Result, failed};
use std::fs::{DirBuilder, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

/// Set for the commands the service runs, so they run instead of asking it
pub const SERVED: &str = "HUNCH_SEAT_SERVED";
/// stdin, stdout and stderr
const FDS: usize = 3;

pub fn socket() -> PathBuf {
  let dir = ["XDG_RUNTIME_DIR", "XDG_CACHE_HOME"]
    .into_iter()
    .filter_map(std::env::var_os)
    .find(|dir| !dir.is_empty())
    .map(PathBuf::from)
    .unwrap_or_else(|| {
      PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
    });
  dir.join("hunch").join("sock").join("seat.sock")
}

/// A command the service answers in its own process, as one that keeps
/// state must: its arguments, the client's stdin and stdout, and whether
/// the client is gone
pub type Here = fn(&[String], &mut File, &mut File, &AtomicBool) -> Result<()>;

/// here picks the commands the service answers itself; start runs once
/// before the service takes clients
pub fn serve(here: fn(&[String]) -> Option<Here>, start: fn()) -> Result<()> {
  let path = socket();
  let shown = path.display();
  let dir = path.parent().expect("the socket is inside hunch/");
  DirBuilder::new()
    .recursive(true)
    .mode(0o700)
    .create(dir)
    .map_err(|e| failed(format!("can't create {}: {e}", dir.display())))?;
  if UnixStream::connect(&path).is_ok() {
    return Err(failed(format!("a service already runs at {shown}")));
  }
  // A service that ended without cleaning up leaves its socket behind
  let _ = std::fs::remove_file(&path);
  let listener = UnixListener::bind(&path)
    .map_err(|e| failed(format!("can't listen at {shown}: {e}")))?;
  eprintln!("hunch-seat: serving at {shown}");
  start();
  for stream in listener.incoming() {
    match stream {
      Ok(stream) => {
        thread::spawn(move || {
          if let Err(e) = answer(stream, here) {
            eprintln!("hunch-seat: {e}");
          }
        });
      }
      Err(e) => eprintln!("hunch-seat: can't accept a client: {e}"),
    }
  }
  Ok(())
}

fn answer(
  stream: UnixStream,
  here: fn(&[String]) -> Option<Here>,
) -> Result<()> {
  let fds = match receive_fds(&stream) {
    Ok(fds) => fds,
    // serve checks for a service this way, and sends nothing
    Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
    Err(e) => {
      return Err(failed(format!("can't take a client's fds: {e}")));
    }
  };
  let [stdin, stdout, stderr]: [OwnedFd; FDS] =
    fds.try_into().map_err(|fds: Vec<_>| {
      failed(format!("a client sent {} fds, not {FDS}", fds.len()))
    })?;
  let mut line = String::new();
  BufReader::new(&stream)
    .read_line(&mut line)
    .map_err(|e| failed(format!("can't read a client's arguments: {e}")))?;
  let args: Vec<String> = serde_json::from_str(&line).map_err(|e| {
    failed(format!(
      "a client's arguments aren't a JSON array of strings: {e}"
    ))
  })?;

  if let Some(command) = here(&args) {
    return answer_here(stream, command, &args, [stdin, stdout, stderr]);
  }

  let exe = std::env::current_exe()
    .map_err(|e| failed(format!("can't find hunch-seat: {e}")))?;
  let mut child = Command::new(exe)
    .args(&args)
    .env(SERVED, "1")
    .stdin(stdin)
    .stdout(stdout)
    .stderr(stderr)
    .spawn()
    .map_err(|e| failed(format!("can't run {args:?}: {e}")))?;

  // A client that's gone, e.g. on ^C, takes its command along
  let pid = child.id() as libc::pid_t;
  let done = Arc::new(Mutex::new(false));
  let watcher = {
    let done = Arc::clone(&done);
    let stream = stream
      .try_clone()
      .map_err(|e| failed(format!("can't watch a client: {e}")))?;
    thread::spawn(move || {
      let _ = (&stream).read(&mut [0]);
      if !*done.lock().expect("no thread panics holding it") {
        // SAFETY: kill only sends a signal
        unsafe { libc::kill(pid, libc::SIGTERM) };
      }
    })
  };
  let status = child.wait();
  *done.lock().expect("no thread panics holding it") = true;
  // Ends the watcher's read
  let _ = stream.shutdown(std::net::Shutdown::Read);
  let _ = watcher.join();

  let status =
    status.map_err(|e| failed(format!("can't wait for {args:?}: {e}")))?;
  let code = status.code().map_or(1, |code| code as u8);
  match (&stream).write_all(&[code]) {
    // The client is gone and took its command along
    Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
    written => {
      written.map_err(|e| failed(format!("can't answer a client: {e}")))
    }
  }
}

fn answer_here(
  stream: UnixStream,
  command: Here,
  args: &[String],
  fds: [OwnedFd; 3],
) -> Result<()> {
  let [stdin, stdout, stderr] = fds;
  let gone = Arc::new(AtomicBool::new(false));
  // The client sends nothing more, so the read ends when it closes: it
  // does once it has the exit status, or when it's gone. Not joined:
  // macOS doesn't wake a read a shutdown of the socket races with
  {
    let gone = Arc::clone(&gone);
    let stream = stream
      .try_clone()
      .map_err(|e| failed(format!("can't watch a client: {e}")))?;
    thread::spawn(move || {
      let _ = (&stream).read(&mut [0]);
      gone.store(true, Ordering::Relaxed);
    });
  }
  let mut input = File::from(stdin);
  let mut out = File::from(stdout);
  let code = match command(&args[1..], &mut input, &mut out, &gone) {
    Ok(()) => 0,
    Err(e) => {
      let _ = writeln!(File::from(stderr), "hunch-seat: {e}");
      e.status()
    }
  };
  drop(out);
  match (&stream).write_all(&[code]) {
    Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
    written => {
      written.map_err(|e| failed(format!("can't answer a client: {e}")))
    }
  }
}

/// Has the service run the command, and returns its exit status
pub fn ask(args: &[String]) -> Result<u8> {
  let path = socket();
  let stream = UnixStream::connect(&path).map_err(|e| {
    failed(format!(
      "no service at {}: {e}; start it with hunch-seat serve",
      path.display()
    ))
  })?;
  let lost = |e: io::Error| failed(format!("can't reach the service: {e}"));
  send_fds(&stream, &[0, 1, 2]).map_err(lost)?;
  writeln!(&stream, "{}", serde_json::json!(args)).map_err(lost)?;
  let mut code = [0];
  match (&stream).read(&mut code) {
    Ok(1) => Ok(code[0]),
    Ok(_) => Err(failed("the service ended before the command did")),
    Err(e) => Err(lost(e)),
  }
}

fn cmsg_space(fds: usize) -> usize {
  // SAFETY: CMSG_SPACE only computes a size
  unsafe { libc::CMSG_SPACE((fds * size_of::<RawFd>()) as u32) as usize }
}

/// Sends fds with one byte, since a message needs data to carry them
fn send_fds(stream: &UnixStream, fds: &[RawFd]) -> io::Result<()> {
  let mut byte = [0u8];
  let mut iov = libc::iovec {
    iov_base: byte.as_mut_ptr().cast(),
    iov_len: byte.len(),
  };
  let mut control = vec![0u8; cmsg_space(fds.len())];
  // SAFETY: msghdr is plain data; the pointers set below outlive sendmsg
  unsafe {
    let mut msg: libc::msghdr = std::mem::zeroed();
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = control.len() as _;
    let cmsg = libc::CMSG_FIRSTHDR(&msg);
    (*cmsg).cmsg_level = libc::SOL_SOCKET;
    (*cmsg).cmsg_type = libc::SCM_RIGHTS;
    (*cmsg).cmsg_len = libc::CMSG_LEN(size_of_val(fds) as u32) as _;
    std::ptr::copy_nonoverlapping(
      fds.as_ptr(),
      libc::CMSG_DATA(cmsg).cast(),
      fds.len(),
    );
    if libc::sendmsg(stream.as_raw_fd(), &msg, 0) < 0 {
      return Err(io::Error::last_os_error());
    }
  }
  Ok(())
}

fn receive_fds(stream: &UnixStream) -> io::Result<Vec<OwnedFd>> {
  let mut byte = [0u8];
  let mut iov = libc::iovec {
    iov_base: byte.as_mut_ptr().cast(),
    iov_len: byte.len(),
  };
  let mut control = vec![0u8; cmsg_space(FDS)];
  // Close-on-exec at once, so no command another thread starts gets them
  #[cfg(target_os = "linux")]
  let flags = libc::MSG_CMSG_CLOEXEC;
  #[cfg(not(target_os = "linux"))]
  let flags = 0;
  let mut fds = Vec::new();
  // SAFETY: msghdr is plain data; the pointers set below outlive recvmsg,
  // and the kernel gave the fds it put in the control data to us alone
  unsafe {
    let mut msg: libc::msghdr = std::mem::zeroed();
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = control.len() as _;
    match libc::recvmsg(stream.as_raw_fd(), &mut msg, flags) {
      n if n < 0 => return Err(io::Error::last_os_error()),
      0 => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
      _ => {}
    }
    let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
    while !cmsg.is_null() {
      if (*cmsg).cmsg_level == libc::SOL_SOCKET
        && (*cmsg).cmsg_type == libc::SCM_RIGHTS
      {
        let data = libc::CMSG_DATA(cmsg).cast::<RawFd>();
        let count = ((*cmsg).cmsg_len as usize - libc::CMSG_LEN(0) as usize)
          / size_of::<RawFd>();
        for i in 0..count {
          let fd = data.add(i).read_unaligned();
          #[cfg(not(target_os = "linux"))]
          libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
          fds.push(OwnedFd::from_raw_fd(fd));
        }
      }
      cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
    }
    if msg.msg_flags & libc::MSG_CTRUNC != 0 {
      return Err(io::Error::other("more fds than expected"));
    }
  }
  Ok(fds)
}
