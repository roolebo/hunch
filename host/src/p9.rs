//! The 9P2000.L messages hunch-host serves and hunch-9p sends, as Linux's
//! include/net/9p/9p.h numbers them.

use std::io::{self, Read, Write};

pub const VERSION: &str = "9P2000.L";
pub const NOTAG: u16 = 0xffff;
pub const NOFID: u32 = 0xffff_ffff;
/// n_uname when the client sends only a name
pub const NONUNAME: u32 = 0xffff_ffff;
/// st_mode's file types
pub const S_IFMT: u32 = 0o170000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFREG: u32 = 0o100000;
/// Rgetattr's valid: the basic fields, as Linux's P9_STATS_BASIC
pub const STATS_BASIC: u64 = 0x7ff;

// Each reply is its request + 1
pub const RLERROR: u8 = 7;
pub const TSTATFS: u8 = 8;
pub const TLOPEN: u8 = 12;
pub const TGETATTR: u8 = 24;
pub const TSETATTR: u8 = 26;
pub const TREADDIR: u8 = 40;
pub const TFSYNC: u8 = 50;
pub const TVERSION: u8 = 100;
pub const TAUTH: u8 = 102;
pub const TATTACH: u8 = 104;
pub const TFLUSH: u8 = 108;
pub const TWALK: u8 = 110;
pub const TREAD: u8 = 116;
pub const TWRITE: u8 = 118;
pub const TCLUNK: u8 = 120;
pub const TREMOVE: u8 = 122;

pub const QTDIR: u8 = 0x80;
pub const QTFILE: u8 = 0;

/// Tlopen's flags: the low two bits say read, write, or both
pub const O_ACCMODE: u32 = 3;
pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;

/// size[4] type[1] tag[2]
pub const HEADER: u32 = 7;
/// HEADER, then count[4] before an Rread's data
pub const RREAD_HEADER: u32 = HEADER + 4;
/// HEADER, then fid[4] offset[8] count[4] before a Twrite's data
pub const TWRITE_HEADER: u32 = HEADER + 16;

/// Linux's errno values, which 9P2000.L carries whatever the host's are
pub mod errno {
  pub const EPERM: u32 = 1;
  pub const ENOENT: u32 = 2;
  pub const EIO: u32 = 5;
  pub const EBADF: u32 = 9;
  pub const EACCES: u32 = 13;
  pub const EBUSY: u32 = 16;
  pub const ENOTDIR: u32 = 20;
  pub const EISDIR: u32 = 21;
  pub const EINVAL: u32 = 22;
  pub const EROFS: u32 = 30;
  pub const EPROTO: u32 = 71;
  pub const EOPNOTSUPP: u32 = 95;

  /// What strerror says for it on Linux
  pub fn describe(errno: u32) -> String {
    match errno {
      EPERM => "Operation not permitted".into(),
      ENOENT => "No such file or directory".into(),
      EIO => "Input/output error".into(),
      EBADF => "Bad file descriptor".into(),
      EACCES => "Permission denied".into(),
      EBUSY => "Device or resource busy".into(),
      ENOTDIR => "Not a directory".into(),
      EISDIR => "Is a directory".into(),
      EINVAL => "Invalid argument".into(),
      EROFS => "Read-only file system".into(),
      EPROTO => "Protocol error".into(),
      EOPNOTSUPP => "Operation not supported".into(),
      _ => format!("errno {errno}"),
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Qid {
  pub kind: u8,
  pub version: u32,
  pub path: u64,
}

/// A message's body, built field by field
#[derive(Default)]
pub struct Body(pub Vec<u8>);

impl Body {
  pub fn u8(mut self, v: u8) -> Self {
    self.0.push(v);
    self
  }
  pub fn u16(mut self, v: u16) -> Self {
    self.0.extend_from_slice(&v.to_le_bytes());
    self
  }
  pub fn u32(mut self, v: u32) -> Self {
    self.0.extend_from_slice(&v.to_le_bytes());
    self
  }
  pub fn u64(mut self, v: u64) -> Self {
    self.0.extend_from_slice(&v.to_le_bytes());
    self
  }
  pub fn str(self, s: &str) -> Self {
    let len = u16::try_from(s.len()).expect("9P strings are under 64K");
    self.u16(len).bytes(s.as_bytes())
  }
  pub fn bytes(mut self, b: &[u8]) -> Self {
    self.0.extend_from_slice(b);
    self
  }
  pub fn qid(self, q: Qid) -> Self {
    self.u8(q.kind).u32(q.version).u64(q.path)
  }
}

/// A message's body, taken field by field
pub struct Fields<'a> {
  data: &'a [u8],
}

fn short() -> io::Error {
  io::Error::new(io::ErrorKind::InvalidData, "9P message too short")
}

impl<'a> Fields<'a> {
  pub fn new(data: &'a [u8]) -> Self {
    Fields { data }
  }
  pub fn bytes(&mut self, n: usize) -> io::Result<&'a [u8]> {
    if self.data.len() < n {
      return Err(short());
    }
    let (head, rest) = self.data.split_at(n);
    self.data = rest;
    Ok(head)
  }
  pub fn u8(&mut self) -> io::Result<u8> {
    Ok(self.bytes(1)?[0])
  }
  pub fn u16(&mut self) -> io::Result<u16> {
    Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
  }
  pub fn u32(&mut self) -> io::Result<u32> {
    Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
  }
  pub fn u64(&mut self) -> io::Result<u64> {
    Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
  }
  pub fn str(&mut self) -> io::Result<String> {
    let n = self.u16()? as usize;
    String::from_utf8(self.bytes(n)?.to_vec()).map_err(|_| {
      io::Error::new(io::ErrorKind::InvalidData, "9P string isn't UTF-8")
    })
  }
  pub fn qid(&mut self) -> io::Result<Qid> {
    Ok(Qid {
      kind: self.u8()?,
      version: self.u32()?,
      path: self.u64()?,
    })
  }
}

pub struct Message {
  pub kind: u8,
  pub tag: u16,
  pub body: Vec<u8>,
}

/// The next message, or None when the peer has closed the connection
pub fn receive(r: &mut impl Read, msize: u32) -> io::Result<Option<Message>> {
  let mut size = [0; 4];
  match r.read_exact(&mut size) {
    Ok(()) => {}
    Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
    Err(e) => return Err(e),
  }
  let size = u32::from_le_bytes(size);
  if !(HEADER..=msize).contains(&size) {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      format!(
        "9P message of {size} bytes, more than msize {msize} or less than \
         a header"
      ),
    ));
  }
  let mut rest = vec![0; size as usize - 4];
  r.read_exact(&mut rest)?;
  Ok(Some(Message {
    kind: rest[0],
    tag: u16::from_le_bytes([rest[1], rest[2]]),
    body: rest.split_off(3),
  }))
}

pub fn send(
  w: &mut impl Write,
  kind: u8,
  tag: u16,
  body: &Body,
) -> io::Result<()> {
  let size = HEADER + body.0.len() as u32;
  let mut message = Vec::with_capacity(size as usize);
  message.extend_from_slice(&size.to_le_bytes());
  message.push(kind);
  message.extend_from_slice(&tag.to_le_bytes());
  message.extend_from_slice(&body.0);
  w.write_all(&message)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_message_comes_back_as_sent() {
    let q = Qid {
      kind: QTDIR,
      version: 1,
      path: 42,
    };
    let mut wire = Vec::new();
    send(
      &mut wire,
      TWALK,
      7,
      &Body::default().u32(1).str("editor").qid(q),
    )
    .unwrap();
    let m = receive(&mut wire.as_slice(), 8192).unwrap().unwrap();
    assert_eq!((m.kind, m.tag), (TWALK, 7));
    let mut f = Fields::new(&m.body);
    assert_eq!(f.u32().unwrap(), 1);
    assert_eq!(f.str().unwrap(), "editor");
    assert_eq!(f.qid().unwrap(), q);
    assert!(f.u8().is_err());
  }

  #[test]
  fn a_message_over_msize_is_refused() {
    let mut wire = Vec::new();
    send(&mut wire, TWRITE, 1, &Body::default().bytes(&[0; 100])).unwrap();
    assert!(receive(&mut wire.as_slice(), 64).is_err());
  }
}
