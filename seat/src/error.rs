use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
  /// Wrong arguments; the usage to print with the message
  Usage(String, &'static str),
  Failed(String),
}

impl Error {
  /// The exit status the protocols give each error
  pub fn status(&self) -> u8 {
    match self {
      Error::Usage(..) | Error::Failed(_) => 1,
    }
  }
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      Error::Usage(message, usage) => write!(f, "{message}\n\n{usage}"),
      Error::Failed(message) => write!(f, "{message}"),
    }
  }
}

pub fn failed(message: impl Into<String>) -> Error {
  Error::Failed(message.into())
}
