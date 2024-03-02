use std::io::{self, Read, Write};
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
}

const READ_AHEAD_MAX: usize = 4096;

fn main() {
    let opts = Options::parse();

    let main_stdin = io::stdin();
    let mut main_stdin_reader = main_stdin.lock();

    let mut buffer = vec![0; READ_AHEAD_MAX];

    // Read ahead some stdin
    let read_ahead = {
        let mut written = 0;
        loop {
            let n = main_stdin_reader
                .read(&mut buffer[written..])
                .expect("Unable to read from stdin");
            if n == 0 {
                break;
            }
            written += n;
        }
        &mut buffer[..written]
    };

    // Ask OpenAI for a command
    let command = ask_openapi_for_command(&read_ahead, &opts);

    eprintln!("Running: {}", command);

    // Spawn the child command suggested by OpenAI
    let mut child = Command::new("bash")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn");

    let mut child_stdin = child.stdin.take().expect("Unable to open child stdin");

    // Flush the read ahead buffer to the child
    let mut cur = read_ahead;
    loop {
        let n = child_stdin
            .write(cur)
            .expect("Unable to write to child stdin");
        if n == 0 {
            break;
        }
        cur = &mut cur[n..];
    }

    // Send the rest of the input to the child
    let cur = buffer.as_mut_slice();
    loop {
        let read = main_stdin_reader.read(cur).expect("Unable to read from stdin");
        if read == 0 {
            break;
        }
        child_stdin
            .write_all(&cur[..read])
            .expect("Unable to write to child stdin");
    }
}

fn ask_openapi_for_command(buffer: &[u8], opts: &Options) -> String {
    let auth = Auth::from_env().unwrap();
    let openai = OpenAI::new(auth, "https://api.openai.com/v1/");
    let example = {
        let newlines = buffer
            .iter()
            .enumerate()
            .filter(|(_, &b)| b == b'\n')
            .map(|(i, _)| i);
        let line_based = newlines.clone().count() > 10;
        let end = if line_based {
            newlines.last().unwrap()
        } else {
            buffer.len()
        };
        std::str::from_utf8(&buffer[..end]).expect("Input from stdin is not utf8")
    };
    let query = format!(
        "
Write a cli command that reads from stdin.
The program goal is: {}.

This is an example of the input ---
{}---
        ",
        opts.message, example
    );

    if opts.debug {
        eprintln!("Sending the following query to OPENAI\n {}", query);
    }

    let body = ChatBody {
        model: "gpt-4".to_string(),
        max_tokens: Some(1000),
        temperature: Some(0.000001),
        top_p: Some(0.000001),
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
    let rs = openai
        .chat_completion_create(&body)
        .expect("chat_completion_create");
    let first = rs.choices.first().unwrap();
    let message = first.message.as_ref().unwrap();

    if opts.debug {
        eprintln!("OpenAI Answer (command): {}", message.content);
    }

    message.content.clone()
}
