use grift::Lisp;
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

fn handle_meta_command(cmd: &str, lisp: &Lisp<ARENA_SIZE>) -> bool {
    let (command, arg) = match cmd.find(' ') {
        Some(pos) => (&cmd[..pos], cmd[pos + 1..].trim()),
        None => (cmd, ""),
    };

    match command {
        ",help" => print_help(),
        ",builtins" => print_builtins(),
        ",doc" => print_doc(arg),
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

fn print_builtins() {
    println!("Operatives (receive unevaluated operands):");
    println!("  quote  if  define!  set!  lambda  vau  begin  cond  and  or  let");
    println!();
    println!("Applicatives (evaluate arguments first):");
    println!("  Arithmetic:   +  -  *  /");
    println!("  Comparison:   =  <  >  <=  >=");
    println!("  Pairs/Lists:  cons  car  cdr  list");
    println!("  Predicates:   null?  pair?  number?  symbol?  boolean?");
    println!("                inert?  ignore?  operative?  applicative?  environment?");
    println!("  Equality:     eq?  equal?  not");
    println!("  Combiners:    eval  wrap  unwrap");
    println!("  Environments: make-environment  make-empty-environment");
    println!();
    println!("Use ,doc <name> for details on any builtin.");
}

fn print_doc(name: &str) {
    if name.is_empty() {
        println!("Usage: ,doc <name>");
        println!("Example: ,doc lambda");
        return;
    }

    match name {
        "quote" => {
            println!("(quote expr) → expr");
            println!();
            println!("  Return expr without evaluating it.");
            println!("  Shorthand: 'expr");
            println!();
            println!("  (quote (+ 1 2))   ; → (+ 1 2)");
        }
        "if" => {
            println!("(if test consequent [alternative])");
            println!();
            println!("  Evaluate test. If #t, evaluate consequent (tail position).");
            println!("  If #f, evaluate alternative (tail position), or () if omitted.");
            println!("  Test must evaluate to a boolean.");
            println!();
            println!("  (if #t 1 2)        ; → 1");
            println!("  (if (< 3 5) 10 20) ; → 10");
        }
        "define!" => {
            println!("(define! definiend expression)");
            println!();
            println!("  Evaluate expression, then match definiend (parameter tree)");
            println!("  against the result, binding symbols in the current environment.");
            println!("  Returns #inert.");
            println!();
            println!("  (define! x 42)");
            println!("  (define! (a b) (list 1 2))");
        }
        "set!" => {
            println!("(set! env-expr definiend expression)");
            println!();
            println!("  Evaluate env-expr to get a target environment and expression");
            println!("  to get a value, then bind definiend in the target environment.");
            println!("  Returns #inert.");
        }
        "lambda" => {
            println!("(lambda params body ...)");
            println!();
            println!("  Create an applicative (arguments are evaluated before binding).");
            println!("  Equivalent to (wrap (vau params #ignore (begin body ...))).");
            println!("  Multiple body expressions are wrapped in begin.");
            println!();
            println!("  (define! add1 (lambda (x) (+ x 1)))");
            println!("  (add1 5)  ; → 6");
        }
        "vau" => {
            println!("(vau params env-param body ...)");
            println!();
            println!("  Create an operative (fexpr). Operands are NOT evaluated.");
            println!("  params: formal parameter tree matched against unevaluated operands.");
            println!("  env-param: symbol bound to caller's environment, or #ignore.");
            println!();
            println!("  (define! my-quote (vau (x) #ignore x))");
            println!("  (my-quote (+ 1 2))  ; → (+ 1 2)");
        }
        "begin" => {
            println!("(begin expr1 expr2 ... exprN)");
            println!();
            println!("  Evaluate each expression in order. Last is in tail position.");
            println!("  Returns the value of the last expression, or () if empty.");
        }
        "cond" => {
            println!("(cond (test1 body1 ...) (test2 body2 ...) ... (else bodyN ...))");
            println!();
            println!("  Evaluate tests in order until one returns #t or else is reached.");
            println!("  Evaluate the corresponding body (last in tail position).");
        }
        "and" => {
            println!("(and expr1 expr2 ... exprN)");
            println!();
            println!("  Short-circuit: returns #f immediately if any expr is #f.");
            println!("  Otherwise returns result of last expr. With no args, returns #t.");
        }
        "or" => {
            println!("(or expr1 expr2 ... exprN)");
            println!();
            println!("  Short-circuit: returns #t immediately if any expr is #t.");
            println!("  Otherwise returns result of last expr. With no args, returns #f.");
        }
        "let" => {
            println!("(let ((name1 val1) (name2 val2) ...) body ...)");
            println!();
            println!("  Create a child environment, evaluate vals in the outer environment,");
            println!("  bind names in the child, evaluate body in the child.");
            println!();
            println!("  (let ((x 10) (y 20)) (+ x y))  ; → 30");
        }
        "+" => {
            println!("(+ . numbers) → number");
            println!();
            println!("  Variadic addition. Zero arguments returns 0.");
            println!("  Uses checked arithmetic (overflow signals ArithmeticOverflow).");
            println!();
            println!("  (+ 1 2 3)  ; → 6");
            println!("  (+)        ; → 0");
        }
        "-" => {
            println!("(- n . rest) → number");
            println!();
            println!("  With one arg: negate. With two+: left fold subtraction.");
            println!();
            println!("  (- 10 3)  ; → 7");
            println!("  (- 5)    ; → -5");
        }
        "*" => {
            println!("(* . numbers) → number");
            println!();
            println!("  Variadic multiplication. Zero arguments returns 1.");
            println!();
            println!("  (* 2 3 4)  ; → 24");
            println!("  (*)        ; → 1");
        }
        "/" => {
            println!("(/ a b) → number");
            println!();
            println!("  Integer (truncating) division. DivisionByZero if b is 0.");
            println!();
            println!("  (/ 10 3)  ; → 3");
        }
        "=" | "<" | ">" | "<=" | ">=" => {
            println!("({name} a b) → boolean");
            println!();
            println!("  Numeric comparison. Both arguments must be numbers.");
        }
        "cons" => {
            println!("(cons a b) → pair");
            println!();
            println!("  Construct a pair (cons cell).");
            println!();
            println!("  (cons 1 2)          ; → (1 . 2)");
            println!("  (cons 1 (list 2 3)) ; → (1 2 3)");
        }
        "car" => {
            println!("(car pair) → value");
            println!();
            println!("  Return the first element of a pair.");
        }
        "cdr" => {
            println!("(cdr pair) → value");
            println!();
            println!("  Return the second element of a pair.");
        }
        "list" => {
            println!("(list . items) → list");
            println!();
            println!("  Return the argument list as-is (already a proper list).");
            println!();
            println!("  (list 1 2 3)  ; → (1 2 3)");
        }
        "null?" | "pair?" | "number?" | "symbol?" | "boolean?" | "inert?" | "ignore?"
        | "operative?" | "applicative?" | "environment?" => {
            println!("({name} . objects) → boolean");
            println!();
            println!("  Variadic type predicate. Returns #t iff every argument");
            println!("  matches the type.");
        }
        "not" => {
            println!("(not boolean) → boolean");
            println!();
            println!("  Boolean negation. Argument must be a boolean.");
        }
        "eq?" => {
            println!("(eq? a b) → boolean");
            println!();
            println!("  Identity equality. For immutable scalar types, compares by value.");
            println!("  For mutable/constructed types, compares by arena identity.");
        }
        "equal?" => {
            println!("(equal? a b) → boolean");
            println!();
            println!("  Structural equality. Returns #t whenever eq? would.");
            println!("  Additionally compares pairs recursively and strings char-by-char.");
        }
        "eval" => {
            println!("(eval expr [env]) → value");
            println!();
            println!("  Evaluate expr in the given environment (defaults to standard env).");
            println!();
            println!("  (eval (list '+ 1 2))  ; → 3");
        }
        "wrap" => {
            println!("(wrap combiner) → applicative");
            println!();
            println!("  Wrap a combiner in an applicative.");
            println!("  Arguments will be evaluated before reaching the combiner.");
        }
        "unwrap" => {
            println!("(unwrap applicative) → combiner");
            println!();
            println!("  Extract the underlying combiner from an applicative.");
        }
        "make-environment" => {
            println!("(make-environment . envs) → environment");
            println!();
            println!("  Create a new environment with the given parent environments.");
        }
        "make-empty-environment" => {
            println!("(make-empty-environment) → environment");
            println!();
            println!("  Create a new environment with no parents.");
        }
        _ => {
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
            "help" | "--help" | "-h" => {
                println!("grift — A no_std Lisp interpreter (vau calculus)");
                println!();
                println!("USAGE:");
                println!("  grift              Start the interactive REPL");
                println!("  grift run <file>   Execute a Grift source file");
                println!("  grift check <src>  Run static analysis on a file or expression");
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
                    if handle_meta_command(line, &lisp) {
                        break;
                    }
                    continue;
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
