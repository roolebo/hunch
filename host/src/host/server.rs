//! Serves a Tree over 9P2000.L, a Connection per client; TREE.md says
//! what reads and writes mean.

use crate::host::control;
use crate::p9::{self, Body, Fields, Qid, errno};
use crate::tree::{Error, Kind, Result, Tree};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, Read, Write};

/// Large enough that a client rarely splits a text it writes
pub const MAX_MSIZE: u32 = 1 << 20;
/// Room for any reply but a read's data
const MIN_MSIZE: u32 = 4096;
const ERRSTR: &str = "errstr";
/// What Linux statfs reports for 9P
const V9FS_MAGIC: u32 = 0x0102_1997;
const NAME_MAX: u32 = 255;

struct Open {
  kind: Kind,
  /// What was written, once anything was
  input: Option<Vec<u8>>,
  /// What a read returns: made at open for an observation, at the first
  /// read for a file that answers
  output: Option<Vec<u8>>,
}

struct Fid {
  path: Vec<String>,
  open: Option<Open>,
}

pub struct Connection<'a> {
  tree: &'a dyn Tree,
  /// Files' owner, as getattr reports it
  uid: u32,
  activity: Option<String>,
  msize: u32,
  fids: HashMap<u32, Fid>,
  errstr: String,
}

fn qid(path: &[String], kind: Kind) -> Qid {
  let mut h = DefaultHasher::new();
  path.hash(&mut h);
  Qid {
    kind: if kind == Kind::Dir {
      p9::QTDIR
    } else {
      p9::QTFILE
    },
    version: 0,
    path: h.finish(),
  }
}

fn refs(path: &[String]) -> Vec<&str> {
  path.iter().map(String::as_str).collect()
}

fn malformed(e: io::Error) -> Error {
  Error::new(errno::EPROTO, e.to_string())
}

impl<'a> Connection<'a> {
  pub fn new(tree: &'a dyn Tree, uid: u32) -> Self {
    Connection {
      tree,
      uid,
      activity: None,
      msize: MAX_MSIZE,
      fids: HashMap::new(),
      errstr: String::new(),
    }
  }

  /// Answers the peer's messages until it closes the connection
  pub fn serve(
    &mut self,
    r: &mut impl Read,
    w: &mut impl Write,
  ) -> io::Result<()> {
    while let Some(m) = p9::receive(r, self.msize)? {
      let (kind, body) = match self.answer(m.kind, &m.body) {
        Ok(body) => (m.kind + 1, body),
        Err(e) => {
          self.errstr = e.message;
          (p9::RLERROR, Body::default().u32(e.errno))
        }
      };
      p9::send(w, kind, m.tag, &body)?;
    }
    Ok(())
  }

  fn activity(&self) -> Result<&str> {
    self
      .activity
      .as_deref()
      .ok_or_else(|| Error::new(errno::EPROTO, "attach first"))
  }

  fn fid(&mut self, fid: u32) -> Result<&mut Fid> {
    self
      .fids
      .get_mut(&fid)
      .ok_or_else(|| Error::new(errno::EBADF, format!("no fid {fid}")))
  }

  fn kind(&self, path: &[String]) -> Result<Kind> {
    if path == [ERRSTR] {
      return Ok(crate::tree::READ);
    }
    self.tree.kind(self.activity()?, &refs(path))
  }

  fn list(&self, path: &[String]) -> Result<Vec<(String, Kind)>> {
    let mut entries = self.tree.list(self.activity()?, &refs(path))?;
    if path.is_empty() {
      entries.insert(0, (ERRSTR.into(), crate::tree::READ));
    }
    Ok(entries)
  }

  fn read(&self, path: &[String], input: Option<&[u8]>) -> Result<Vec<u8>> {
    if path == [ERRSTR] {
      return Ok(if self.errstr.is_empty() {
        Vec::new()
      } else {
        format!("{}\n", self.errstr).into_bytes()
      });
    }
    self.tree.read(self.activity()?, &refs(path), input)
  }

  fn answer(&mut self, kind: u8, body: &[u8]) -> Result<Body> {
    let mut f = Fields::new(body);
    match kind {
      p9::TVERSION => {
        let msize = f.u32().map_err(malformed)?;
        let version = f.str().map_err(malformed)?;
        self.msize = msize.clamp(MIN_MSIZE, MAX_MSIZE);
        self.fids.clear();
        let answer = if version.starts_with(p9::VERSION) {
          p9::VERSION
        } else {
          "unknown"
        };
        Ok(Body::default().u32(self.msize).str(answer))
      }
      // Authentication will come through Tauth: TREE.md's Control
      p9::TAUTH => Err(Error::new(
        errno::EOPNOTSUPP,
        "no authentication yet: attach without it",
      )),
      p9::TATTACH => {
        let fid = f.u32().map_err(malformed)?;
        let _afid = f.u32().map_err(malformed)?;
        let _uname = f.str().map_err(malformed)?;
        let activity = f.str().map_err(malformed)?;
        if activity.is_empty() {
          return Err(Error::new(
            errno::EINVAL,
            "attach with an ACTIVITY as aname",
          ));
        }
        self.activity = Some(activity);
        self.fids.insert(
          fid,
          Fid {
            path: Vec::new(),
            open: None,
          },
        );
        Ok(Body::default().qid(qid(&[], Kind::Dir)))
      }
      p9::TFLUSH => Ok(Body::default()),
      p9::TWALK => {
        let fid = f.u32().map_err(malformed)?;
        let newfid = f.u32().map_err(malformed)?;
        let n = f.u16().map_err(malformed)?;
        let names = (0..n)
          .map(|_| f.str())
          .collect::<io::Result<Vec<_>>>()
          .map_err(malformed)?;
        let mut path = self.fid(fid)?.path.clone();
        let mut qids = Vec::new();
        for name in &names {
          let mut next = path.clone();
          if name == ".." {
            next.pop();
          } else {
            next.push(name.clone());
          }
          match self.kind(&next) {
            Ok(kind) => qids.push(qid(&next, kind)),
            // Only the first name failing is an error; a later
            // one makes a partial walk, still with its reason
            Err(e) if qids.is_empty() => return Err(e),
            Err(e) => {
              self.errstr = e.message;
              break;
            }
          }
          path = next;
        }
        if qids.len() == names.len() {
          self.fids.insert(newfid, Fid { path, open: None });
        }
        let mut body = Body::default().u16(qids.len() as u16);
        for q in qids {
          body = body.qid(q);
        }
        Ok(body)
      }
      p9::TLOPEN => {
        let fid = f.u32().map_err(malformed)?;
        let flags = f.u32().map_err(malformed)?;
        let path = self.fid(fid)?.path.clone();
        let kind = self.kind(&path)?;
        let (readable, writable) = match kind {
          Kind::Dir => (true, false),
          Kind::File { read, write } => (read, write),
        };
        let (reads, writes) = match flags & p9::O_ACCMODE {
          p9::O_RDONLY => (true, false),
          p9::O_WRONLY => (false, true),
          p9::O_RDWR => (true, true),
          _ => {
            return Err(Error::new(errno::EINVAL, "no such open mode"));
          }
        };
        if (reads && !readable) || (writes && !writable) {
          let can = if readable { "only read" } else { "only write" };
          return Err(Error::new(
            errno::EACCES,
            format!("{}: {can} it", path.join("/")),
          ));
        }
        // An observation is what the file holds when it's opened
        let output = if kind == crate::tree::READ {
          Some(self.read(&path, None)?)
        } else {
          None
        };
        self.fid(fid)?.open = Some(Open {
          kind,
          input: None,
          output,
        });
        Ok(Body::default().qid(qid(&path, kind)).u32(0))
      }
      p9::TREAD => {
        let fid = f.u32().map_err(malformed)?;
        let offset = f.u64().map_err(malformed)?;
        let count = f
          .u32()
          .map_err(malformed)?
          .min(self.msize - p9::RREAD_HEADER);
        let path = self.fid(fid)?.path.clone();
        let open = self.fid(fid)?.open.take();
        let Some(mut open) = open else {
          return Err(Error::new(errno::EBADF, "open the file first"));
        };
        if open.kind == Kind::Dir {
          self.fid(fid)?.open = Some(open);
          return Err(Error::new(
            errno::EISDIR,
            "read a directory with readdir",
          ));
        }
        let made = match open.output {
          Some(_) => Ok(()),
          None => self.read(&path, open.input.as_deref()).map(|out| {
            open.output = Some(out);
          }),
        };
        // A file that failed to answer has done with what was written:
        // the clunk doesn't run it again as an action
        if made.is_err() {
          open.input = None;
        }
        let output = open.output.as_deref().unwrap_or_default();
        let start = (offset as usize).min(output.len());
        let end = (start + count as usize).min(output.len());
        let data = output[start..end].to_vec();
        self.fid(fid)?.open = Some(open);
        made?;
        Ok(Body::default().u32(data.len() as u32).bytes(&data))
      }
      p9::TWRITE => {
        let fid = f.u32().map_err(malformed)?;
        let _offset = f.u64().map_err(malformed)?;
        let count = f.u32().map_err(malformed)?;
        let data = f.bytes(count as usize).map_err(malformed)?;
        let open = self
          .fid(fid)?
          .open
          .as_mut()
          .ok_or_else(|| Error::new(errno::EBADF, "open the file first"))?;
        // An action runs once, on what all the writes gave
        open
          .input
          .get_or_insert_with(Vec::new)
          .extend_from_slice(data);
        Ok(Body::default().u32(count))
      }
      p9::TCLUNK | p9::TREMOVE => {
        let fid = f.u32().map_err(malformed)?;
        let Some(Fid { path, open }) = self.fids.remove(&fid) else {
          return Err(Error::new(errno::EBADF, format!("no fid {fid}")));
        };
        if kind == p9::TREMOVE {
          return Err(Error::new(errno::EPERM, "files here can't be removed"));
        }
        // A write is an action that runs when the file is closed,
        // unless a read already took its answer
        if let Some(Open {
          input: Some(input),
          output: None,
          ..
        }) = open
        {
          let activity = self.activity()?;
          control::allow(activity, &refs(&path))?;
          self.tree.write(activity, &refs(&path), &input)?;
        }
        Ok(Body::default())
      }
      p9::TREADDIR => {
        let fid = f.u32().map_err(malformed)?;
        let offset = f.u64().map_err(malformed)?;
        let count = f
          .u32()
          .map_err(malformed)?
          .min(self.msize - p9::RREAD_HEADER);
        let path = self.fid(fid)?.path.clone();
        // An entry's offset is the index of the one after it
        let mut entries = Body::default();
        for (i, (name, kind)) in self
          .list(&path)?
          .into_iter()
          .enumerate()
          .skip(offset as usize)
        {
          let mut child = path.clone();
          child.push(name.clone());
          let kind_byte = if kind == Kind::Dir { 4 } else { 8 };
          let entry = Body::default()
            .qid(qid(&child, kind))
            .u64(i as u64 + 1)
            .u8(kind_byte)
            .str(&name);
          if entries.0.len() + entry.0.len() > count as usize {
            break;
          }
          entries = entries.bytes(&entry.0);
        }
        Ok(
          Body::default()
            .u32(entries.0.len() as u32)
            .bytes(&entries.0),
        )
      }
      p9::TGETATTR => {
        let fid = f.u32().map_err(malformed)?;
        let path = self.fid(fid)?.path.clone();
        let kind = self.kind(&path)?;
        let mode = match kind {
          Kind::Dir => p9::S_IFDIR | 0o500,
          Kind::File { read, write } => {
            p9::S_IFREG
              | if read { 0o400 } else { 0 }
              | if write { 0o200 } else { 0 }
          }
        };
        let mut body = Body::default()
          .u64(p9::STATS_BASIC)
          .qid(qid(&path, kind))
          .u32(mode)
          .u32(self.uid)
          .u32(self.uid)
          // nlink, rdev, size, blksize, blocks
          .u64(1)
          .u64(0)
          .u64(0)
          .u64(4096)
          .u64(0);
        // atime, mtime, ctime, btime, gen, data_version
        for _ in 0..10 {
          body = body.u64(0);
        }
        Ok(body)
      }
      // A shell's > truncates and editors fsync; neither means
      // anything here
      p9::TSETATTR | p9::TFSYNC => Ok(Body::default()),
      p9::TSTATFS => Ok(
        Body::default()
          .u32(V9FS_MAGIC)
          .u32(4096)
          .u64(0)
          .u64(0)
          .u64(0)
          .u64(0)
          .u64(0)
          .u64(0)
          .u32(NAME_MAX),
      ),
      _ => Err(Error::new(
        errno::EOPNOTSUPP,
        format!("9P message type {kind} isn't served here"),
      )),
    }
  }
}
