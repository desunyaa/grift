use grift::Lisp;
use grift_check::docs::{DocEntry, DocKind, builtin_docs};
use rustyline::DefaultEditor;

const PROMPT: &str = "Λ> ";
const ARENA_SIZE: usize = 100_000;

fn print_banner() {
    println!("  ╔═══════════════════════════════════════════╗");
    println!("  ║  Grift v{:<35}║", env!("CARGO_PKG_VERSION"));
    println!("  ║  A no_std Lisp · vau calculus · arena GC  ║");
    println!("  ╠═══════════════════════════════════════════╣");
    println!("  ║  ,help       — show available commands     ║");
    println!("  ║  ,builtins   — list all built-in forms     ║");
    println!("  ║  ,doc <name> — show documentation          ║");
    println!("  ║  ,check <expr> — static analysis           ║");
    println!("  ║  ,env        — show arena statistics       ║");
    println!("  ║  ,quit       — exit the REPL               ║");
    println!("  ╚═══════════════════════════════════════════╝");
    println!();
}

fn handle_meta_command(
    cmd: &str,
    lisp: &Lisp<ARENA_SIZE>,
    docs: &[(&'static str, DocEntry)],
) -> bool {
    let (command, arg) = match cmd.find(' ') {
        Some(pos) => (&cmd[..pos], cmd[pos + 1..].trim()),
        None => (cmd, ""),
    };

    match command {
        ",help" => print_help(),
        ",builtins" => print_builtins(docs),
        ",doc" => print_doc(arg, docs),
        ",check" => run_check(arg),
        ",env" | ",stats" => print_stats(lisp),
        ",quit" | ",exit" | ",q" => return true,
        _ => eprintln!("Unknown command: {command}. Type ,help for available commands."),
    }
    false
}

fn print_help() {
    println!("REPL Commands:");
    println!("  ,help              Show this help message");
    println!("  ,builtins          List all built-in operatives and applicatives");
    println!("  ,doc <name>        Show documentation for a builtin (e.g. ,doc lambda)");
    println!("  ,check <expr>      Run static analysis on an expression");
    println!("  ,env               Show arena allocation statistics");
    println!("  ,quit              Exit the REPL");
    println!();
    println!("Evaluation:");
    println!("  Enter any Grift expression to evaluate it.");
    println!("  Examples:");
    println!("    (+ 1 2 3)                         ; → 6");
    println!("    (define! double (lambda (x) (* x 2)))");
    println!("    (double 21)                        ; → 42");
    println!();
}

fn print_builtins(docs: &[(&'static str, DocEntry)]) {
    let operatives: Vec<&str> = docs
        .iter()
        .filter(|(_, e)| e.kind == DocKind::Operative)
        .map(|(name, _)| *name)
        .collect();

    let applicatives: Vec<&str> = docs
        .iter()
        .filter(|(_, e)| e.kind == DocKind::Applicative)
        .map(|(name, _)| *name)
        .collect();

    println!("Operatives (receive unevaluated operands):");
    println!("  {}", operatives.join("  "));
    println!();
    println!("Applicatives (evaluate arguments first):");
    for chunk in applicatives.chunks(8) {
        println!("  {}", chunk.join("  "));
    }
    println!();
    println!("Use ,doc <name> for details on any builtin.");
}

fn print_doc(name: &str, docs: &[(&'static str, DocEntry)]) {
    if name.is_empty() {
        println!("Usage: ,doc <name>");
        println!("Example: ,doc lambda");
        return;
    }

    match docs.iter().find(|(n, _)| *n == name) {
        Some((_, entry)) => {
            println!("{}", entry.signature);
            println!();
            println!("  {}", entry.description);
        }
        None => {
            eprintln!("No documentation for '{name}'.");
            eprintln!("Use ,builtins to see all available builtins.");
        }
    }
}

fn run_check(expr: &str) {
    if expr.is_empty() {
        println!("Usage: ,check <expression>");
        println!("Example: ,check (+ 1 2)");
        return;
    }

    let result = grift_check::check(expr);
    if result.diagnostics.is_empty() {
        println!("✓ No issues found.");
    } else {
        for d in &result.diagnostics {
            let severity = match d.severity {
                grift_check::Severity::Error => "error",
                grift_check::Severity::Warning => "warning",
                grift_check::Severity::Hint => "hint",
            };
            println!(
                "[{severity}] {}:{}: {}",
                d.start.line + 1,
                d.start.col + 1,
                d.message
            );
        }
    }
}

fn print_stats(lisp: &Lisp<ARENA_SIZE>) {
    let stats = lisp.stats();
    println!("Arena Statistics:");
    println!("  Capacity:   {}", stats.capacity);
    println!("  Allocated:  {}", stats.allocated);
    println!("  Free:       {}", stats.free);
    println!("  Occupancy:  {:.1}%", stats.usage_percent());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Handle subcommands
    if args.len() > 1 {
        match args[1].as_str() {
            "check" => {
                if args.len() < 3 {
                    eprintln!("Usage: grift check <file-or-expression>");
                    std::process::exit(1);
                }
                let input = if std::path::Path::new(&args[2]).exists() {
                    std::fs::read_to_string(&args[2]).unwrap_or_else(|e| {
                        eprintln!("Error reading file: {e}");
                        std::process::exit(1);
                    })
                } else {
                    args[2..].join(" ")
                };
                let result = grift_check::check(&input);
                if result.diagnostics.is_empty() {
                    println!("✓ No issues found.");
                } else {
                    for d in &result.diagnostics {
                        let severity = match d.severity {
                            grift_check::Severity::Error => "error",
                            grift_check::Severity::Warning => "warning",
                            grift_check::Severity::Hint => "hint",
                        };
                        println!(
                            "[{severity}] {}:{}: {}",
                            d.start.line + 1,
                            d.start.col + 1,
                            d.message
                        );
                    }
                    if !result.is_ok() {
                        std::process::exit(1);
                    }
                }
                return;
            }
            "run" => {
                if args.len() < 3 {
                    eprintln!("Usage: grift run <file>");
                    std::process::exit(1);
                }
                let input = std::fs::read_to_string(&args[2]).unwrap_or_else(|e| {
                    eprintln!("Error reading file: {e}");
                    std::process::exit(1);
                });
                let lisp: Lisp<ARENA_SIZE> = Lisp::new();
                match lisp.eval(&input) {
                    Ok(val) => println!("{val}"),
                    Err(e) => {
                        eprintln!("error: {e:?}");
                        std::process::exit(1);
                    }
                }
                return;
            }
            "lsp" => {
                // Launch the grift-lsp binary (installed alongside grift)
                let exe = std::env::current_exe().ok().and_then(|p| {
                    let dir = p.parent()?;
                    let lsp = dir.join("grift-lsp");
                    lsp.exists().then_some(lsp)
                });

                match exe {
                    Some(lsp_path) => {
                        let status = std::process::Command::new(lsp_path)
                            .stdin(std::process::Stdio::inherit())
                            .stdout(std::process::Stdio::inherit())
                            .stderr(std::process::Stdio::inherit())
                            .status();
                        match status {
                            Ok(s) => std::process::exit(s.code().unwrap_or(0)),
                            Err(e) => {
                                eprintln!("Failed to start grift-lsp: {e}");
                                std::process::exit(1);
                            }
                        }
                    }
                    None => {
                        // Fallback: try PATH
                        let status = std::process::Command::new("grift-lsp")
                            .stdin(std::process::Stdio::inherit())
                            .stdout(std::process::Stdio::inherit())
                            .stderr(std::process::Stdio::inherit())
                            .status();
                        match status {
                            Ok(s) => std::process::exit(s.code().unwrap_or(0)),
                            Err(_) => {
                                eprintln!("grift-lsp not found. Build it with:");
                                eprintln!("  cargo build -p grift_lsp");
                                eprintln!();
                                eprintln!("Or run it directly:");
                                eprintln!("  cargo run -p grift_lsp");
                                std::process::exit(1);
                            }
                        }
                    }
                }
            }
            "help" | "--help" | "-h" => {
                println!("grift — A no_std Lisp interpreter (vau calculus)");
                println!();
                println!("USAGE:");
                println!("  grift              Start the interactive REPL");
                println!("  grift run <file>   Execute a Grift source file");
                println!("  grift check <src>  Run static analysis on a file or expression");
                println!("  grift lsp          Start the LSP server (stdio)");
                println!("  grift help         Show this help message");
                println!();
                println!("REPL COMMANDS:");
                println!("  ,help              Show REPL commands");
                println!("  ,builtins          List all built-in forms");
                println!("  ,doc <name>        Show documentation for a builtin");
                println!("  ,check <expr>      Run static analysis on an expression");
                println!("  ,env               Show arena statistics");
                println!("  ,quit              Exit the REPL");
                return;
            }
            "--version" | "-V" => {
                println!("grift {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            other => {
                eprintln!("Unknown command: {other}");
                eprintln!("Run 'grift help' for usage information.");
                std::process::exit(1);
            }
        }
    }

    // Start the REPL
    let lisp: Lisp<ARENA_SIZE> = Lisp::new();
    let docs = builtin_docs();
    let mut rl = DefaultEditor::new().expect("failed to initialize editor");

    print_banner();

    loop {
        match rl.readline(PROMPT) {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(line);

                // Handle meta-commands
                if line.starts_with(',') {
                    if handle_meta_command(line, &lisp, &docs) {
                        break;
                    }
                    continue;
                }

                // Run static analysis first for better error messages
                let check_result = grift_check::check(line);
                if !check_result.is_ok() {
                    for d in &check_result.diagnostics {
                        let severity = match d.severity {
                            grift_check::Severity::Error => "error",
                            grift_check::Severity::Warning => "warning",
                            grift_check::Severity::Hint => "hint",
                        };
                        eprintln!(
                            "[{severity}] {}:{}: {}",
                            d.start.line + 1,
                            d.start.col + 1,
                            d.message
                        );
                    }
                    continue;
                }

                // Show warnings but still evaluate
                for d in &check_result.diagnostics {
                    if d.severity == grift_check::Severity::Warning {
                        eprintln!(
                            "[warning] {}:{}: {}",
                            d.start.line + 1,
                            d.start.col + 1,
                            d.message
                        );
                    }
                }

                match lisp.eval(line) {
                    Ok(val) => println!("{val}"),
                    Err(e) => eprintln!("error: {e:?}"),
                }
            }
            Err(
                rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof,
            ) => {
                break;
            }
            Err(e) => {
                eprintln!("error: {e}");
                break;
            }
        }
    }
}
