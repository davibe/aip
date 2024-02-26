use std::io::{self, BufRead, Write};
use std::process::{Command, Stdio};

use openai_api_rust::chat::*;
use openai_api_rust::*;

use clap::Parser;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Options {
    /// The message to send to the AI
    message: String,

    /// Activate debug messages
    #[clap(short, long, value_name = "debug")]
    debug: bool,

    /// How many output lines will be sent to OpenAI as an example of your input
    #[clap(short, long, value_name = "sendlines", default_value = "10")]
    send_lines: usize,
}

fn main() {
    let opts = Options::parse();

    let main_stdin = io::stdin();
    let mut main_stdin_reader = main_stdin.lock();

    let main_stdout = io::stdout();
    let mut main_stdout_writer = main_stdout.lock();

    let mut read_ahead_buffer = String::new();
    let mut read_ahead_count = 0;

    while let Ok(bytes_read) = main_stdin_reader.read_line(&mut read_ahead_buffer) {
        if bytes_read == 0 {
            break;
        }
        if read_ahead_count >= opts.send_lines {
            break;
        }
        read_ahead_count += 1;
    }

    let auth = Auth::from_env().unwrap();
    let openai = OpenAI::new(auth, "https://api.openai.com/v1/");
    let query = format!(
        "
Write a cli command that reads lines from stdin.
The program goal is: {}.

This is an example of the input ---
{}---
",
        opts.message, read_ahead_buffer
    );

    if opts.debug {
        eprintln!("Sending the following query to OPENAI\n {}", query);
    }

    let body = ChatBody {
        model: "gpt-4".to_string(),
        max_tokens: Some(1000),
        temperature: Some(0_f32),
        top_p: Some(0_f32),
        n: Some(1),
        stream: Some(false),
        stop: None,
        presence_penalty: None,
        frequency_penalty: None,
        logit_bias: None,
        user: None,
        messages: vec![
            Message {
                role: Role::System,
                content: "You only output raw unix commands, no markdown. You prefer oneliners if possible.".to_string(),
            },
            Message {
                role: Role::User,
                content: query,
            },
        ],
    };
    let rs = openai.chat_completion_create(&body);
    let choice = rs.unwrap().choices;
    let message = &choice[0].message.as_ref().unwrap();

    if opts.debug {
        eprintln!("OpenAI Answer (command): {}", message.content);
    }

    eprintln!("Running: {}", message.content);

    let command = message
        .content
        .clone()
        .replace(r".*```bash", "")
        .replace(r"```.*", "")
        .trim()
        .to_string();

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");

    let mut child_stdin = child.stdin.take().expect("stdin");
    let child_stdout = child.stdout.take().expect("stdout");

    // TODO: does this need to be a thread ?
    child_stdin
        .write_all(read_ahead_buffer.as_bytes())
        .expect("write");
    // NOTE: we're reusing the read_ahead_buffer for subsequent lines after readahead
    read_ahead_buffer.clear();
    while let Ok(bytes_read) = main_stdin_reader.read_line(&mut read_ahead_buffer) {
        if bytes_read == 0 {
            break;
        }
        child_stdin
            .write_all(read_ahead_buffer.as_bytes())
            .expect("write");
        read_ahead_buffer.clear();
    }
    drop(child_stdin);

    // Pull child output
    let mut line = String::new();
    let mut child_stdout_reader = io::BufReader::new(child_stdout);
    while let Ok(bytes_read) = child_stdout_reader.read_line(&mut line) {
        if bytes_read == 0 {
            break;
        }
        main_stdout_writer
            .write_all(line.as_bytes())
            .expect("Error writing to stdout");
        line.clear();
    }

    // Wait for the child to finish
    let output = child.wait_with_output().expect("wait");
    if output.status.success() {
        if opts.debug {
            eprintln!("Process done.");
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("Process error: {}", stderr);
    }
}
