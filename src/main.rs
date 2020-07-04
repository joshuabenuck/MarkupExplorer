use anyhow::{anyhow, Result};
use clap;
use reqwest;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use soup::prelude::*;
use tokio;

struct MarkupExplorer {
    url: Option<String>,
    contents: Option<String>,
    cols: Option<usize>,
}

impl MarkupExplorer {
    fn new() -> MarkupExplorer {
        MarkupExplorer {
            url: None,
            contents: None,
            cols: Some(80),
        }
    }

    fn parse_line(&self, line: String) -> Vec<String> {
        // Note: This performs allocations....
        // It may eventually be rewritten so it doesn't.
        let mut args = Vec::new();
        let mut next = String::new();
        let mut quoted = false;
        let mut escaped = false;
        for char in line.chars() {
            if char == '\\' {
                escaped = true;
            } else if escaped {
                escaped = false;
                next.push(char);
            } else if quoted && char == '"' {
                quoted = false;
                args.push(next);
                next = String::new();
            } else if char == '"' {
                quoted = true;
            } else if !quoted && char == ' ' {
                args.push(next);
                next = String::new();
            } else {
                next.push(char);
            }
        }
        if next.len() > 0 {
            args.push(next);
        }
        args
    }

    async fn url(&mut self, url: &str) -> Result<()> {
        self.url = Some(url.to_string());
        let response: reqwest::Response = reqwest::get(url).await?;
        if response.status().is_server_error() {
            return Err(anyhow!("server error: {}", response.status()));
        }
        self.contents = Some(response.text().await?);
        Ok(())
    }

    async fn process_line(&mut self, line: String) -> Result<()> {
        let mut args = self.parse_line(line);
        if args.len() == 0 {
            return Ok(());
        }
        let command = args.remove(0);
        match command.as_str() {
            "cols" => {
                let count = args.remove(0);
                if count == "max" {
                    self.cols = None;
                } else {
                    self.cols = Some(count.parse()?);
                }
            }
            "find" => {
                let soup = soup::Soup::new(self.contents.as_ref().expect("No contents to parse."));
                let mut iter = args.iter().peekable();
                let mut arg = iter.next();
                let mut node = None;
                while arg.is_some() {
                    let value = arg.unwrap();
                    match value.as_str() {
                        "tag" => {
                            let tag = iter.next().expect("No tag specified!");
                            if tag == "true" {
                                node = soup.tag(true).find();
                            } else {
                                node = soup.tag(tag.as_str()).find();
                            }
                            if node.as_ref().is_none() {
                                return Err(anyhow!("Unable to find tag {}", tag));
                            }
                        }
                        "name" => {
                            println!("{}", node.as_ref().unwrap().name());
                        }
                        "attrs" => {
                            for (name, _value) in node.as_ref().unwrap().attrs() {
                                println!("{}", name);
                            }
                        }
                        "values" => {
                            for (name, value) in node.as_ref().unwrap().attrs() {
                                println!("{} = {}", name, value);
                            }
                        }
                        "tree" => {
                            let children = match &node {
                                Some(n) => n.children(),
                                None => soup.children(),
                            };
                            for child in children {
                                println!("{}", child.name());
                            }
                        }
                        v => {
                            return Err(anyhow!("Unrecognized param: {}", v));
                        }
                    }
                    arg = iter.next();
                }
            }
            "url" => {
                let url = &args[0];
                self.url(url).await?;
            }
            "head" => {
                let max = &args[0];
                let max: u32 = max.parse()?;
                let mut count = 0;
                match &self.contents {
                    None => return Err(anyhow!("No contents available.")),
                    Some(c) => {
                        for line in c.split("\n") {
                            let chars: Vec<char> = line.chars().collect();
                            if self.cols.is_some() && chars.len() > self.cols.unwrap() {
                                let trunc: String =
                                    chars.iter().take(self.cols.unwrap() - 3).collect();
                                println!("{}...", trunc);
                            } else {
                                println!("{}", line);
                            }
                            count += 1;
                            if count >= max {
                                break;
                            }
                        }
                    }
                }
            }
            _ => {}
        };
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let home = dirs::home_dir().expect("Unable to find home dir.");
    let history_dir = home.join(".me");
    if !history_dir.exists() {
        if std::fs::create_dir(&history_dir).is_err() {
            println!("Unable to create {}", &history_dir.display());
            std::process::exit(1);
        }
    }
    let history = history_dir.join("history");
    // `()` can be used when no completer is required
    let mut rl = Editor::<()>::new();
    if rl.load_history(&history).is_err() {
        println!("No previous history.");
    }
    let mut me = MarkupExplorer::new();
    loop {
        let readline = rl.readline(">> ");
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str());
                match me.process_line(line).await {
                    Ok(_) => (),
                    Err(err) => println!("Error: {}", err),
                };
            }
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }
    rl.save_history(&history).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_line() {
        let me = MarkupExplorer::new();
        // Space separated
        assert_eq!(
            me.parse_line("cat ~/file".to_string()),
            vec!("cat", "~/file")
        );
        // Quoted
        assert_eq!(
            me.parse_line("cat \"~/file\"".to_string()),
            vec!("cat", "~/file")
        );
        // Quoted with embedded spaces
        assert_eq!(
            me.parse_line("cat \"quoted arg\"".to_string()),
            vec!("cat", "quoted arg")
        );
        // Escaped quotes
        assert_eq!(
            me.parse_line("cat \"arg with \\\"embedded\\\" quotes".to_string()),
            vec!("cat", "arg with \"embedded\" quotes")
        );
        // Escaped spaces
        assert_eq!(
            me.parse_line("cat arg\\ with\\ escaped\\ spaces".to_string()),
            vec!("cat", "arg with escaped spaces")
        );
    }
}
