//! A 9P2000.L client for one activity's workspace: what hunch-9p does from
//! the shell.

use crate::host::MAX_MSIZE;
use crate::p9::{self, Body, Fields, errno};
use crate::tree::{Error, Kind, Result};
use std::io;
use std::os::unix::net::UnixStream;

const ROOT: u32 = 0;
const FILE: u32 = 1;
const ERRSTR: u32 = 2;

pub struct Client {
  stream: UnixStream,
  msize: u32,
  /// Whether FILE is walked and not yet clunked
  holding: bool,
}

fn broken(e: io::Error) -> Error {
  Error::new(errno::EIO, format!("hunch-host: {e}"))
}

impl Client {
  /// Attaches to ACTIVITY's workspace on a connection to hunch-host
  pub fn attach(stream: UnixStream, activity: &str) -> Result<Client> {
    let mut c = Client {
      stream,
      msize: MAX_MSIZE,
      holding: false,
    };
    let mut r = c.rpc(
      p9::TVERSION,
      Body::default().u32(MAX_MSIZE).str(p9::VERSION),
    )?;
    c.msize = Fields::new(&r).u32().map_err(broken)?;
    r = c.rpc(
      p9::TATTACH,
      Body::default()
        .u32(ROOT)
        .u32(p9::NOFID)
        .str(&std::env::var("USER").unwrap_or_default())
        .str(activity)
        .u32(p9::NONUNAME),
    )?;
    drop(r);
    Ok(c)
  }

  fn rpc(&mut self, kind: u8, body: Body) -> Result<Vec<u8>> {
    // One request at a time, so one tag does
    p9::send(&mut self.stream, kind, 1, &body).map_err(broken)?;
    let m = p9::receive(&mut self.stream, self.msize)
      .map_err(broken)?
      .ok_or_else(|| {
        Error::new(errno::EIO, "hunch-host closed the connection")
      })?;
    if m.kind == p9::RLERROR {
      let e = Fields::new(&m.body).u32().map_err(broken)?;
      return Err(Error::new(e, String::new()));
    }
    if m.kind != kind + 1 {
      return Err(Error::new(
        errno::EPROTO,
        format!("reply {} to request {kind}", m.kind),
      ));
    }
    Ok(m.body)
  }

  fn walk(&mut self, fid: u32, path: &str) -> Result<()> {
    let names: Vec<&str> = path.split('/').filter(|n| !n.is_empty()).collect();
    let mut body = Body::default().u32(ROOT).u32(fid).u16(names.len() as u16);
    for name in &names {
      body = body.str(name);
    }
    let r = self.rpc(p9::TWALK, body)?;
    if Fields::new(&r).u16().map_err(broken)? as usize != names.len() {
      return Err(Error::new(errno::ENOENT, String::new()));
    }
    self.holding |= fid == FILE;
    Ok(())
  }

  fn open(&mut self, fid: u32, path: &str, mode: u32) -> Result<()> {
    self.walk(fid, path)?;
    self
      .rpc(p9::TLOPEN, Body::default().u32(fid).u32(mode))
      .map(drop)
  }

  fn read_all(&mut self, fid: u32) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    loop {
      let r = self.rpc(
        p9::TREAD,
        Body::default()
          .u32(fid)
          .u64(data.len() as u64)
          .u32(self.msize - p9::RREAD_HEADER),
      )?;
      let mut f = Fields::new(&r);
      let n = f.u32().map_err(broken)? as usize;
      if n == 0 {
        return Ok(data);
      }
      data.extend_from_slice(f.bytes(n).map_err(broken)?);
    }
  }

  fn write_all(&mut self, fid: u32, data: &[u8]) -> Result<()> {
    let chunk = (self.msize - p9::TWRITE_HEADER) as usize;
    // Even an empty text is an action, so at least one write
    let mut chunks: Vec<&[u8]> = data.chunks(chunk).collect();
    if chunks.is_empty() {
      chunks.push(&[]);
    }
    let mut offset = 0;
    for c in chunks {
      let body = Body::default()
        .u32(fid)
        .u64(offset)
        .u32(c.len() as u32)
        .bytes(c);
      self.rpc(p9::TWRITE, body)?;
      offset += c.len() as u64;
    }
    Ok(())
  }

  fn clunk(&mut self, fid: u32) -> Result<()> {
    if fid == FILE {
      self.holding = false;
    }
    self.rpc(p9::TCLUNK, Body::default().u32(fid)).map(drop)
  }

  /// Runs op on FILE; on an error, reads errstr before clunking FILE, as
  /// a failed clunk would replace it
  fn with_file<R>(
    &mut self,
    op: impl FnOnce(&mut Self) -> Result<R>,
  ) -> Result<R> {
    op(self).map_err(|mut e| {
      if e.message.is_empty() {
        e.message = self.errstr();
      }
      if self.holding {
        let _ = self.clunk(FILE);
      }
      e
    })
  }

  pub fn errstr(&mut self) -> String {
    let text = (|| {
      self.open(ERRSTR, "errstr", p9::O_RDONLY)?;
      let text = self.read_all(ERRSTR);
      self.clunk(ERRSTR)?;
      text
    })();
    String::from_utf8_lossy(&text.unwrap_or_default())
      .trim()
      .to_string()
  }

  pub fn read(&mut self, path: &str) -> Result<Vec<u8>> {
    self.with_file(|c| {
      c.open(FILE, path, p9::O_RDONLY)?;
      let data = c.read_all(FILE)?;
      c.clunk(FILE)?;
      Ok(data)
    })
  }

  pub fn write(&mut self, path: &str, data: &[u8]) -> Result<()> {
    self.with_file(|c| {
      c.open(FILE, path, p9::O_WRONLY)?;
      c.write_all(FILE, data)?;
      c.clunk(FILE)
    })
  }

  /// Writes to a file that answers, then reads its answer
  pub fn rdwr(&mut self, path: &str, data: &[u8]) -> Result<Vec<u8>> {
    self.with_file(|c| {
      c.open(FILE, path, p9::O_RDWR)?;
      c.write_all(FILE, data)?;
      let answer = c.read_all(FILE)?;
      c.clunk(FILE)?;
      Ok(answer)
    })
  }

  /// What PATH is: a directory, or a file to read, write or both
  pub fn kind(&mut self, path: &str) -> Result<Kind> {
    self.with_file(|c| {
      c.walk(FILE, path)?;
      let r =
        c.rpc(p9::TGETATTR, Body::default().u32(FILE).u64(p9::STATS_BASIC))?;
      let mut f = Fields::new(&r);
      f.u64().map_err(broken)?;
      f.qid().map_err(broken)?;
      let mode = f.u32().map_err(broken)?;
      c.clunk(FILE)?;
      Ok(if mode & p9::S_IFMT == p9::S_IFDIR {
        Kind::Dir
      } else {
        Kind::File {
          read: mode & 0o400 != 0,
          write: mode & 0o200 != 0,
        }
      })
    })
  }

  /// The names in a directory, a directory's with a / after it
  pub fn ls(&mut self, path: &str) -> Result<Vec<String>> {
    self.with_file(|c| {
      c.open(FILE, path, p9::O_RDONLY)?;
      let mut names = Vec::new();
      let mut offset = 0;
      loop {
        let r = c.rpc(
          p9::TREADDIR,
          Body::default()
            .u32(FILE)
            .u64(offset)
            .u32(c.msize - p9::RREAD_HEADER),
        )?;
        let mut f = Fields::new(&r);
        let n = f.u32().map_err(broken)? as usize;
        if n == 0 {
          break;
        }
        let mut entries = Fields::new(f.bytes(n).map_err(broken)?);
        while let Ok(qid) = entries.qid() {
          offset = entries.u64().map_err(broken)?;
          let _type = entries.u8().map_err(broken)?;
          let name = entries.str().map_err(broken)?;
          names.push(if qid.kind == p9::QTDIR {
            format!("{name}/")
          } else {
            name
          });
        }
      }
      c.clunk(FILE)?;
      Ok(names)
    })
  }
}
