//! Reads the metadata keys of a GGUF file, which is all compute needs to
//! tell a speech model from others.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

const MAGIC: &[u8; 4] = b"GGUF";
const STRING: u32 = 8;
const ARRAY: u32 = 9;

/// The byte size of each GGUF value type that has a fixed one
fn fixed_size(kind: u32) -> Option<i64> {
  match kind {
    0 | 1 | 7 => Some(1),
    2 | 3 => Some(2),
    4..=6 => Some(4),
    10..=12 => Some(8),
    _ => None,
  }
}

struct Reader(BufReader<File>);

impl Reader {
  fn u32(&mut self) -> io::Result<u32> {
    let mut b = [0; 4];
    self.0.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
  }

  fn u64(&mut self) -> io::Result<u64> {
    let mut b = [0; 8];
    self.0.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
  }

  fn string(&mut self) -> io::Result<String> {
    let mut b = vec![0; self.u64()? as usize];
    self.0.read_exact(&mut b)?;
    Ok(String::from_utf8_lossy(&b).into_owned())
  }

  fn skip(&mut self, kind: u32) -> io::Result<()> {
    match kind {
      STRING => self.string().map(drop),
      ARRAY => {
        let item = self.u32()?;
        let count = self.u64()?;
        if item == STRING || item == ARRAY {
          (0..count).try_for_each(|_| self.skip(item))
        } else {
          let size = fixed_size(item)
            .ok_or_else(|| io::Error::other("unknown GGUF type"))?;
          self
            .0
            .seek(SeekFrom::Current(size * count as i64))
            .map(drop)
        }
      }
      kind => {
        let size = fixed_size(kind)
          .ok_or_else(|| io::Error::other("unknown GGUF type"))?;
        self.0.seek(SeekFrom::Current(size)).map(drop)
      }
    }
  }
}

/// The metadata keys, and the value of each string key
pub fn metadata(path: &Path) -> io::Result<Vec<(String, Option<String>)>> {
  let mut r = Reader(BufReader::new(File::open(path)?));
  let mut magic = [0; 4];
  r.0.read_exact(&mut magic)?;
  if &magic != MAGIC {
    return Err(io::Error::other("not a GGUF file"));
  }
  let _version = r.u32()?;
  let _tensors = r.u64()?;
  let count = r.u64()?;
  (0..count)
    .map(|_| {
      let key = r.string()?;
      let kind = r.u32()?;
      let value = if kind == STRING {
        Some(r.string()?)
      } else {
        r.skip(kind)?;
        None
      };
      Ok((key, value))
    })
    .collect()
}
