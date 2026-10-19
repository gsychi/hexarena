//! Bots that run in another process, so a CPU compiled from a different checkout can
//! take a seat. The host writes the seat's [`Style`] as one JSON line, then one [`View`]
//! line per tick; the program answers each view with one [`PlayerCommand`] line.
//! `hexarena bot` is such a program for every brain compiled into it.

use crate::{brains, Bot, Style};
use hexarena_protocol::{PlayerCommand, View};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// A seat that has not answered within this long plays no-ops for the rest of the game.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Program {
    pub exe: String,
    #[serde(default)]
    pub args: Vec<String>,
}

pub struct ProcessBot {
    name: String,
    child: Child,
    stdin: Option<BufWriter<ChildStdin>>,
    replies: Receiver<String>,
    failed: bool,
}

impl ProcessBot {
    pub fn spawn(program: &Program, style: &Style) -> Result<ProcessBot, String> {
        let mut child = Command::new(&program.exe)
            .args(&program.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("{}: {e}", program.exe))?;
        let stdout = child.stdout.take().unwrap();
        let (tx, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut stdin = BufWriter::new(child.stdin.take().unwrap());
        let hello = Style { program: None, ..style.clone() };
        writeln!(stdin, "{}", serde_json::to_string(&hello).unwrap())
            .map_err(|e| format!("{}: {e}", program.exe))?;
        Ok(ProcessBot { name: style.name.clone(), child, stdin: Some(stdin), replies, failed: false })
    }

    fn ask(&mut self, view: &View) -> Option<PlayerCommand> {
        let stdin = self.stdin.as_mut()?;
        writeln!(stdin, "{}", serde_json::to_string(view).ok()?).ok()?;
        stdin.flush().ok()?;
        let line = self.replies.recv_timeout(REPLY_TIMEOUT).ok()?;
        serde_json::from_str(&line).ok()
    }
}

/// Starts `program` for `style` and asks it for one move, failing unless it answers.
pub fn probe(program: &Program, style: &Style, view: &View) -> Result<PlayerCommand, String> {
    let mut bot = ProcessBot::spawn(program, style)?;
    bot.ask(view).ok_or_else(|| format!("{} {} did not answer a view", program.exe, program.args.join(" ")))
}

impl Bot for ProcessBot {
    fn name(&self) -> &str {
        &self.name
    }

    fn act(&mut self, view: &View) -> PlayerCommand {
        if self.failed {
            return PlayerCommand::default();
        }
        match self.ask(view) {
            Some(cmd) => cmd,
            None => {
                eprintln!("bot '{}' stopped answering; it plays no-ops from tick {}", self.name, view.tick);
                self.failed = true;
                let _ = self.child.kill();
                PlayerCommand::default()
            }
        }
    }
}

impl Drop for ProcessBot {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The program side: reads a [`Style`] line, then answers every [`View`] line.
pub fn serve_stdio() -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let hello = lines.next().ok_or("expected a style line")?.map_err(|e| e.to_string())?;
    let style: Style = serde_json::from_str(&hello).map_err(|e| format!("style: {e}"))?;
    let mut bot = brains::build(Style { program: None, ..style })?;
    let mut out = BufWriter::new(std::io::stdout().lock());
    for line in lines {
        let line = line.map_err(|e| e.to_string())?;
        let view: View = serde_json::from_str(&line).map_err(|e| format!("view: {e}"))?;
        writeln!(out, "{}", serde_json::to_string(&bot.act(&view)).unwrap()).map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}
