//! An OpenAI-compatible ENDPOINT, as INFERENCE.md says: hunch-host lists
//! its models and posts speech to its audio/transcriptions through curl,
//! with the key this host keeps for it.

type Result<T> = std::result::Result<T, String>;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

/// How long the server may take to answer, in seconds
const MAX_TIME: u32 = 120;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A file only this user reads, removed when it's dropped
struct Speech(PathBuf);

impl Speech {
  fn new(extension: &str, bytes: &[u8]) -> Result<Speech> {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir()
      .join(format!("hunch-host-{}-{n}.{extension}", std::process::id()));
    OpenOptions::new()
      .write(true)
      .create_new(true)
      .mode(0o600)
      .open(&path)
      .and_then(|mut f| f.write_all(bytes))
      .map_err(|e| format!("can't keep the speech: {e}"))?;
    Ok(Speech(path))
  }
}

impl Drop for Speech {
  fn drop(&mut self) {
    let _ = std::fs::remove_file(&self.0);
  }
}

/// VALUE quoted for curl's config
fn quote(value: &str) -> String {
  format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The JSON the server at ADDR answers to PATH, with OPTIONS for curl and
/// KEY as the bearer token
fn ask(
  addr: &str,
  path: &str,
  key: Option<&str>,
  options: &str,
) -> Result<serde_json::Value> {
  // Options come on stdin, so the key stays off the command line
  let url = format!("{}/{path}", addr.trim_end_matches('/'));
  let mut options = format!(
    "url = {}\nsilent\nshow-error\nfail-with-body\nmax-time = {MAX_TIME}\n\
     {options}",
    quote(&url),
  );
  if let Some(key) = key {
    let header = format!("Authorization: Bearer {key}");
    options += &format!("header = {}\n", quote(&header));
  }
  let mut curl = Command::new("curl")
    .args(["--config", "-"])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|e| format!("can't run curl: {e}"))?;
  curl
    .stdin
    .take()
    .expect("stdin is piped")
    .write_all(options.as_bytes())
    .map_err(|e| format!("can't hand curl its options: {e}"))?;
  let out = curl
    .wait_with_output()
    .map_err(|e| format!("curl didn't finish: {e}"))?;
  let body = String::from_utf8_lossy(&out.stdout);
  if !out.status.success() {
    let said = String::from_utf8_lossy(&out.stderr);
    return Err(format!("{addr}: {} {}", said.trim(), body.trim()));
  }
  serde_json::from_str(&body)
    .map_err(|e| format!("{addr}'s answer isn't JSON: {e}: {body}"))
}

/// The text the server at ADDR hears in SPEECH, a media type and its
/// bytes, with MODEL; KEY goes as its bearer token
pub fn transcribe(
  addr: &str,
  key: Option<&str>,
  model: &str,
  (kind, bytes): (&str, &[u8]),
) -> Result<String> {
  // The server tells the format by the file's name
  let extension = kind.rsplit('/').next().unwrap_or(kind);
  let speech = Speech::new(extension, bytes)?;
  let file = format!("file=@{};type={kind}", speech.0.display());
  let options = format!(
    "form = {}\nform-string = \"response_format=json\"\nform-string = {}\n",
    quote(&file),
    quote(&format!("model={model}")),
  );
  let answer = ask(addr, "audio/transcriptions", key, &options)?;
  answer["text"]
    .as_str()
    .map(|t| t.trim().to_string())
    .ok_or_else(|| format!("{addr} answered without text: {answer}"))
}

/// The server's models, as its models lists them
pub fn models(addr: &str, key: Option<&str>) -> Result<Vec<serde_json::Value>> {
  let answer = ask(addr, "models", key, "")?;
  answer["data"]
    .as_array()
    .cloned()
    .ok_or_else(|| format!("{addr} lists no models: {answer}"))
}

/// What audio/transcriptions takes, as OpenAI's API documents it, Ogg
/// first: the smallest the seat writes
const SPEECH: [&str; 6] = [
  "audio/ogg",
  "audio/flac",
  "audio/wav",
  "audio/mpeg",
  "audio/mp4",
  "audio/webm",
];

/// The model ID as COMPUTE.md's models describes it, from LISTED, its
/// entry in the server's list. OpenAI's list says nothing of what a
/// model takes, so a model is speech to text only where the server says
/// so, as Unsloth does with task or audio_workflows
pub fn describe(id: &str, listed: &serde_json::Value) -> serde_json::Value {
  let transcribes = listed["task"] == "automatic-speech-recognition"
    || listed["audio_workflows"]
      .as_array()
      .is_some_and(|w| w.iter().any(|w| w == "transcribe"));
  let (inputs, outputs): (&[&str], &[&str]) = if transcribes {
    (&SPEECH, &["text/plain"])
  } else {
    (&[], &[])
  };
  serde_json::json!({
    "id": id,
    "name": listed["display_name"].as_str(),
    "inputs": inputs,
    "outputs": outputs,
    "loaded": listed["loaded"].as_bool(),
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;
  use std::io::{BufRead, BufReader, Read};
  use std::net::TcpListener;
  use std::thread;

  /// A server that answers one request with BODY, and hands back the
  /// request it got
  fn server(body: &'static str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("http://{}/v1", listener.local_addr().unwrap());
    let got = thread::spawn(move || {
      let (stream, _) = listener.accept().unwrap();
      let mut reader = BufReader::new(stream.try_clone().unwrap());
      let mut head = String::new();
      let mut length = 0;
      loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if let Some(n) = line.to_lowercase().strip_prefix("content-length:") {
          length = n.trim().parse().unwrap();
        }
        head += &line;
        if line == "\r\n" {
          break;
        }
      }
      let mut request = vec![0; length];
      reader.read_exact(&mut request).unwrap();
      let mut stream = stream;
      write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
      )
      .unwrap();
      head + &String::from_utf8_lossy(&request)
    });
    (addr, got)
  }

  #[test]
  fn the_server_gets_the_speech_with_the_key_and_model() {
    let (addr, got) = server(r#"{"text": " hello there \n"}"#);
    let speech = ("audio/wav", &b"RIFFwav"[..]);
    let text = transcribe(&addr, Some("sk-test"), "whisper-1", speech).unwrap();
    assert_eq!(text, "hello there");
    let request = got.join().unwrap();
    assert!(request.starts_with("POST /v1/audio/transcriptions "));
    assert!(
      request.contains("Authorization: Bearer sk-test"),
      "{request}"
    );
    assert!(
      request.contains("name=\"model\"\r\n\r\nwhisper-1"),
      "{request}"
    );
    assert!(request.contains("RIFFwav"), "{request}");
  }

  #[test]
  fn without_a_key_no_authorization_goes() {
    let (addr, got) = server(r#"{"text": "hi"}"#);
    let speech = ("audio/ogg", &b"OggS"[..]);
    assert_eq!(transcribe(&addr, None, "m", speech).unwrap(), "hi");
    let request = got.join().unwrap();
    assert!(!request.contains("Authorization"), "{request}");
    assert!(
      request.contains(".ogg\"\r\nContent-Type: audio/ogg"),
      "{request}"
    );
  }

  #[test]
  fn a_model_transcribes_only_where_the_server_says_so() {
    let asr = json!({"task": "automatic-speech-recognition"});
    let workflow = json!({"audio_workflows": ["transcribe"]});
    let chat = json!({"display_name": "Qwen", "context_length": 8192});
    for listed in [asr, workflow] {
      let model = describe("parakeet", &listed);
      assert_eq!(model["inputs"][0], "audio/ogg");
      assert_eq!(model["outputs"], json!(["text/plain"]));
    }
    let model = describe("qwen", &chat);
    assert_eq!(model["name"], "Qwen");
    assert_eq!(model["inputs"], json!([]));
    assert_eq!(model["outputs"], json!([]));
  }

  #[test]
  fn the_list_of_models_is_its_data() {
    let (addr, got) = server(r#"{"data": [{"id": "a"}, {"id": "b"}]}"#);
    let models = models(&addr, Some("sk")).unwrap();
    assert_eq!(models.len(), 2);
    assert!(got.join().unwrap().starts_with("GET /v1/models "));
  }

  #[test]
  fn a_refusal_says_what_the_server_said() {
    let (addr, _got) = server(r#"{"detail": "Not authenticated"}"#);
    // A 200 without text is refused as well
    let e =
      transcribe(&addr, None, "m", ("audio/wav", &b"RIFF"[..])).unwrap_err();
    assert!(e.to_string().contains("without text"), "{e}");
  }
}
