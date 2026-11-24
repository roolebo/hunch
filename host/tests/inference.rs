//! INFERENCE.md through 9P2000.L: local: as a fake of the tree hunch-seat
//! serves, a server as a fake on a local port, keys in memory, and the
//! namespace that mounts one of them.

mod common;
mod conform;

use hunch::client::Client;
use hunch::device::{Inference, Keys};
use hunch::p9::errno;
use hunch::tree::{ANSWER, Error, Kind, READ, Result, Tree, no_such};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

/// local:, with one model; it answers what it got
struct FakeLocal;

impl Tree for FakeLocal {
  fn kind(&self, _: &str, path: &[&str]) -> Result<Kind> {
    match path {
      [] => Ok(Kind::Dir),
      ["models"] => Ok(READ),
      ["transcribe"] => Ok(ANSWER),
      _ => Err(no_such()),
    }
  }

  fn list(&self, _: &str, _: &[&str]) -> Result<Vec<(String, Kind)>> {
    Ok(vec![("models".into(), READ), ("transcribe".into(), ANSWER)])
  }

  fn read(
    &self,
    _: &str,
    path: &[&str],
    input: Option<&[u8]>,
  ) -> Result<Vec<u8>> {
    match path {
      ["models"] => Ok(
        json!([{"id": "parakeet", "inputs": ["audio/wav"],
                "outputs": ["text/plain"], "loaded": true}])
        .to_string()
        .into_bytes(),
      ),
      ["transcribe"] => Ok([b"heard ", input.unwrap_or_default()].concat()),
      _ => Err(no_such()),
    }
  }

  fn write(&self, _: &str, _: &[&str], _: &[u8]) -> Result<()> {
    Err(Error::new(errno::EACCES, "only read it"))
  }
}

#[derive(Clone, Default)]
struct FakeKeys(Arc<Mutex<HashMap<String, String>>>);

impl Keys for FakeKeys {
  fn lookup(&self, endpoint: &str) -> Option<String> {
    self.0.lock().unwrap().get(endpoint).cloned()
  }

  fn store(
    &self,
    endpoint: &str,
    key: &str,
  ) -> std::result::Result<(), String> {
    self.0.lock().unwrap().insert(endpoint.into(), key.into());
    Ok(())
  }
}

/// A server that answers each request with the body for its path, and
/// keeps the requests it got
fn server(
  bodies: HashMap<&'static str, &'static str>,
) -> (String, Arc<Mutex<Vec<String>>>) {
  let listener = TcpListener::bind("127.0.0.1:0").unwrap();
  let url = format!("http://{}/v1", listener.local_addr().unwrap());
  let got = Arc::new(Mutex::new(Vec::new()));
  let kept = got.clone();
  thread::spawn(move || {
    for stream in listener.incoming() {
      let stream = stream.unwrap();
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
      let mut body = vec![0; length];
      reader.read_exact(&mut body).unwrap();
      let path = head.split(' ').nth(1).unwrap_or_default();
      let answer = bodies[path.strip_prefix("/v1/").unwrap_or(path)];
      let mut stream = stream;
      write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{answer}",
        answer.len()
      )
      .unwrap();
      kept
        .lock()
        .unwrap()
        .push(head + &String::from_utf8_lossy(&body));
    }
  });
  (url, got)
}

struct World {
  client: Client,
  keys: FakeKeys,
  namespace: PathBuf,
}

/// inference/ as NAMESPACE mounts it
fn world(namespace: &str) -> World {
  let dir = std::env::temp_dir().join(format!(
    "hunch-inference-{}-{:?}",
    std::process::id(),
    thread::current().id()
  ));
  std::fs::create_dir_all(&dir).unwrap();
  let path = dir.join("namespace");
  std::fs::write(&path, namespace).unwrap();
  let keys = FakeKeys::default();
  let tree =
    Inference::new(Box::new(FakeLocal), Arc::new(keys.clone()), path.clone());
  World {
    client: common::attach(Arc::new(tree), ""),
    keys,
    namespace: path,
  }
}

fn read_json(c: &mut Client, path: &str) -> Value {
  serde_json::from_slice(&c.read(path).unwrap()).unwrap()
}

fn request(model: &str, kind: &str, speech: &[u8]) -> Vec<u8> {
  let header = json!({"model": model, "type": kind});
  [format!("{header}\n").as_bytes(), speech].concat()
}

const MODELS: &str = r#"{"data": [
  {"id": "parakeet-tdt", "task": "automatic-speech-recognition"},
  {"id": "qwen", "display_name": "Qwen"}
]}"#;

#[test]
fn without_a_mount_inference_is_local() {
  let mut w = world("");
  assert_eq!(read_json(&mut w.client, "models")[0]["id"], "parakeet");
  let speech = request("parakeet", "audio/wav", b"RIFF");
  let heard = w.client.rdwr("transcribe", &speech).unwrap();
  assert_eq!(heard, [b"heard ".as_slice(), &speech].concat());
}

#[test]
fn a_mounted_server_says_what_its_models_take() {
  let (url, _) = server(HashMap::from([("models", MODELS)]));
  let mut w = world(&format!("# the workstation\nmount {url} /inference\n"));
  let models = read_json(&mut w.client, "models");
  assert_eq!(models[0]["id"], "parakeet-tdt");
  assert_eq!(models[0]["inputs"][0], "audio/ogg");
  assert_eq!(models[0]["outputs"], json!(["text/plain"]));
  assert_eq!(models[1]["name"], "Qwen");
  assert_eq!(models[1]["outputs"], json!([]));
}

#[test]
fn a_server_transcribes_with_this_hosts_key() {
  let (url, got) = server(HashMap::from([
    ("models", MODELS),
    ("audio/transcriptions", r#"{"text": " hello \n"}"#),
  ]));
  let mut w = world(&format!("mount {url} /inference\n"));
  w.client.write("login", b"sk-test\n").unwrap();
  assert_eq!(w.keys.lookup(&url).as_deref(), Some("sk-test"));
  let text = w
    .client
    .rdwr("transcribe", &request("parakeet-tdt", "audio/ogg", b"OggS"))
    .unwrap();
  assert_eq!(text, b"hello");
  let request = got.lock().unwrap().last().unwrap().clone();
  assert!(
    request.contains("Authorization: Bearer sk-test"),
    "{request}"
  );
  assert!(request.contains("OggS"), "{request}");
}

#[test]
fn the_namespace_is_read_each_time() {
  let (url, _) = server(HashMap::from([("models", MODELS)]));
  let mut w = world("");
  assert_eq!(read_json(&mut w.client, "models")[0]["id"], "parakeet");
  std::fs::write(&w.namespace, format!("mount {url} /inference\n")).unwrap();
  assert_eq!(read_json(&mut w.client, "models")[0]["id"], "parakeet-tdt");
}

#[test]
fn a_request_is_a_line_of_json_first() {
  let (url, _) = server(HashMap::from([("models", MODELS)]));
  let mut w = world(&format!("mount {url} /inference\n"));
  w.client.rdwr("transcribe", b"no request").unwrap_err();
  assert!(w.client.errstr().contains("line of JSON"));
}

#[test]
fn either_mount_conforms() {
  conform::conforms(&mut world("").client);
  let (url, _) = server(HashMap::from([("models", MODELS)]));
  conform::conforms(&mut world(&format!("mount {url} /inference\n")).client);
}
