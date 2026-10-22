mod app;
mod cli;
mod errors;
mod exporters;
mod input;
mod models;
mod pipeline;

use anyhow::Result;
use chrono::Local;
use clap::Parser;
use cli::Args;
use std::io::{self, Write};

fn main() -> Result<()> {
    let args = Args::parse();
    init_tracing(&args);
    let auto_mode = args.input.is_none() && args.dir.is_none();
    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();

    let result = app::run(args, &timestamp);

    if let Err(ref e) = result {
        eprintln!("Error: {:?}", e);
    }

    if auto_mode {
        pause_before_exit();
    }

    if result.is_err() {
        std::process::exit(1);
    }

    Ok(())
}

/// Nivel de diagnóstico por defecto `info`; `--verbose` lo sube a `debug` (revela
/// las métricas de consolidación) y `--quiet` lo baja a `error` (nada salvo fallos).
/// `RUST_LOG`, si está definida, tiene prioridad sobre ambos flags.
fn init_tracing(args: &Args) {
    let default_level = if args.verbose {
        "debug"
    } else if args.quiet {
        "error"
    } else {
        "info"
    };

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level));

    tracing_subscriber::fmt().with_env_filter(env_filter).init();
}

/// En Windows, si el ejecutable se abrió con doble click no hay terminal que
/// mantenga la ventana abierta al terminar: se espera una tecla para poder
/// leer el resultado antes de que se cierre.
fn pause_before_exit() {
    if cfg!(windows) {
        println!("\nPresione Enter para salir...");
        let _ = io::stdout().flush();
        let mut buf = String::new();
        let _ = io::stdin().read_line(&mut buf);
    }
}
