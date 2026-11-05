mod api;
mod app;
mod art;
mod config;
mod log;
mod player;
mod search;
mod settings;
mod store;
mod theme;
mod ui;

use ratatui::crossterm::event::{self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event};
use ratatui::crossterm::execute;
use ratatui_image::picker::Picker;
use std::time::{Duration, Instant};

const USAGE: &str = "splynt: a terminal music client for Navidrome and Subsonic servers

usage: splynt [options]

  --images auto|halfblocks|off   how cover art is drawn (default: auto)
  --sign-out                     forget the saved server and token
  --config                       print the config file path
  --log                          print the log file path
  -h, --help                     show this help

Press ? inside the app for every key.";

fn main() {
    let mut cfg = config::Config::load();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            "--config" => {
                println!("{}", config::Config::path().display());
                return;
            }
            "--log" => {
                println!("{}", log::path().display());
                return;
            }
            "--sign-out" | "--logout" => {
                cfg.token.clear();
                cfg.salt.clear();
                cfg.save();
                println!("Signed out of {}.", cfg.server);
                return;
            }
            "--images" => {
                cfg.images = args.next().unwrap_or_else(|| "auto".into());
            }
            other => {
                eprintln!("unknown option {other}\n\n{USAGE}");
                std::process::exit(2);
            }
        }
    }

    log::init();
    log::info(format!(
        "terminal: TERM={} TERM_PROGRAM={} COLORTERM={}",
        std::env::var("TERM").unwrap_or_default(),
        std::env::var("TERM_PROGRAM").unwrap_or_default(),
        std::env::var("COLORTERM").unwrap_or_default()
    ));

    // Ask the terminal which graphics protocol it speaks before taking over the screen.
    let picker = if cfg.images == "off" {
        Picker::halfblocks()
    } else {
        Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks())
    };

    log::info(format!("images: {:?}, truecolor: {}", picker.protocol_type(), theme::truecolor()));
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture, EnableBracketedPaste);
    let mut app = app::App::new(cfg, picker);

    let truecolor = theme::truecolor();
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    while !app.quit {
        app.pump();
        let tick = app.frame_interval();
        if app.dirty || tick.is_some_and(|t| last_draw.elapsed() >= t) {
            app.dirty = false;
            let _ = terminal.draw(|f| {
                ui::draw(f, &mut app);
                if !truecolor {
                    theme::downgrade(f.buffer_mut());
                }
            });
            last_draw = Instant::now();
        }
        if event::poll(Duration::from_millis(25)).unwrap_or(false) {
            match event::read() {
                Ok(Event::Key(k)) => app.on_key(k),
                Ok(Event::Mouse(m)) => app.on_mouse(m),
                Ok(Event::Paste(s)) => app.paste(s),
                Ok(Event::Resize(..)) => app.dirty = true,
                _ => {}
            }
        }
    }

    let _ = execute!(std::io::stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
    app.shutdown();
}
