//! `hexarena agent <status|note|say|inbox>`: an agent's one-line way to tell the arena
//! what it is doing, write in the lab notebook, and answer the people messaging it,
//! without hand-writing JSON for curl.

use crate::arena::http;

const USAGE: &str = "usage: hexarena agent <status|note|say|inbox> [--author agent:NAME] [TEXT... | -]
  status TEXT        what you are working on now (the home page shows it on your card)
  note TEXT          a lab notebook entry: [--kind observation|lesson|idea] (observation)
                     [--bots a,b] [--matches m01,m02] [--tags x,y]
  say TEXT           reply in your chat thread
  inbox              messages waiting for your reply, oldest first
TEXT is every word that isn't a flag; `-` reads it from stdin. The author is --author or
$HEXARENA_AUTHOR. The server is $HEXARENA_SERVER_ADDR (default 127.0.0.1:47240).";

const VALUE_FLAGS: [&str; 5] = ["author", "kind", "bots", "matches", "tags"];

struct Args {
    author: String,
    text: String,
    flag: std::collections::HashMap<String, String>,
}

fn parse(rest: &[String]) -> Result<Args, String> {
    let mut flag = std::collections::HashMap::new();
    let mut words = vec![];
    let mut i = 0;
    while i < rest.len() {
        match rest[i].strip_prefix("--") {
            Some(k) if VALUE_FLAGS.contains(&k) => {
                let v = rest.get(i + 1).ok_or_else(|| format!("--{k} needs a value\n{USAGE}"))?;
                flag.insert(k.to_string(), v.clone());
                i += 2;
            }
            Some(k) => return Err(format!("unknown flag --{k}\n{USAGE}")),
            None => {
                words.push(rest[i].clone());
                i += 1;
            }
        }
    }
    let mut text = words.join(" ");
    if text == "-" {
        text.clear();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text).map_err(|e| e.to_string())?;
    }
    let author = flag
        .get("author")
        .cloned()
        .or_else(|| std::env::var("HEXARENA_AUTHOR").ok())
        .ok_or_else(|| format!("say who you are with --author agent:NAME or HEXARENA_AUTHOR\n{USAGE}"))?;
    Ok(Args { author, text: text.trim().to_string(), flag })
}

fn list(flag: &std::collections::HashMap<String, String>, k: &str) -> Vec<String> {
    flag.get(k).map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
}

pub fn main(rest: &[String]) -> Result<(), String> {
    let cmd = rest.first().map(String::as_str).ok_or(USAGE)?;
    let a = parse(&rest[1..])?;
    let needs_text = || if a.text.is_empty() { Err(format!("{cmd} needs some text\n{USAGE}")) } else { Ok(()) };
    let author = &a.author;
    match cmd {
        "status" => {
            needs_text()?;
            http("POST", &format!("/api/agents/{author}/status"), Some(&serde_json::json!({ "text": a.text })))?;
            println!("status set for {author}");
        }
        "note" => {
            needs_text()?;
            let body = serde_json::json!({
                "author": author,
                "kind": a.flag.get("kind").map_or("observation", String::as_str),
                "text": a.text,
                "bots": list(&a.flag, "bots"),
                "matches": list(&a.flag, "matches"),
                "tags": list(&a.flag, "tags"),
            });
            let out = http("POST", "/api/observations", Some(&body))?;
            let n = serde_json::from_str::<serde_json::Value>(&out).ok().and_then(|v| v["n"].as_u64());
            println!("note {} written to the lab notebook", n.map_or("?".into(), |n| format!("n{n}")));
        }
        "say" => {
            needs_text()?;
            http("POST", &format!("/api/agents/{author}/messages"), Some(&serde_json::json!({ "from": author, "text": a.text })))?;
            println!("replied in {author}'s thread");
        }
        "inbox" => {
            let out = http("GET", &format!("/api/agents/{author}/messages"), None)?;
            let thread: Vec<serde_json::Value> = serde_json::from_str(&out).map_err(|e| e.to_string())?;
            let waiting: Vec<&serde_json::Value> = thread.iter().rev().take_while(|m| m["from"] != author.as_str()).collect();
            if waiting.is_empty() {
                println!("no messages waiting");
            }
            for m in waiting.iter().rev() {
                println!("[{}] {}", m["from"].as_str().unwrap_or("?"), m["text"].as_str().unwrap_or_default());
            }
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}
