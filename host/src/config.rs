//! The seat's config, as skills/hunch/SEAT.md specifies it: what the
//! human lets leave the seat. Read each time, since the tray writes it
//! while the service runs.

use serde_json::Value;
use std::path::PathBuf;

fn path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
        .join("hunch/seat.json")
}

fn read() -> Value {
    let path = path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Value::Null;
    };
    serde_json::from_str(&text).unwrap_or_else(|e| {
        eprintln!(
            "hunch-host: {} isn't JSON, so nothing may leave: {e}",
            path.display()
        );
        Value::Null
    })
}

fn hosts(allow: &Value) -> impl Iterator<Item = String> + '_ {
    allow
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        // ssh would take it for an option
        .filter(|host| {
            let option = host.starts_with('-');
            if option {
                eprintln!("hunch-host: the seat's config allows {host:?}, which isn't a host");
            }
            !option
        })
        .map(String::from)
}

/// The hosts the browser's pages may go to
pub fn browser_allow() -> Vec<String> {
    hosts(&read()["browser"]["allow"]).collect()
}

/// Every host some data of the seat may go to
pub fn allowed() -> Vec<String> {
    let config = read();
    let devices = config["devices"].as_object().into_iter().flatten();
    let mut all: Vec<String> = hosts(&config["browser"]["allow"])
        .chain(devices.flat_map(|(_, device)| hosts(&device["allow"])))
        .collect();
    all.sort();
    all.dedup();
    all
}
