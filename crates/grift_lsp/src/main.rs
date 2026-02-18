#![forbid(unsafe_code)]

//! grift-lsp: A stdio-based Language Server Protocol server for Grift Lisp.

use std::io::{self, BufReader};

use grift_lsp::{
    CompletionParams, DidChangeParams, DidOpenParams, GriftLanguageServer, HoverParams,
    RpcResponse, read_message, send_notification, send_response,
};

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut writer = stdout.lock();

    let mut server = GriftLanguageServer::new();

    eprintln!("grift-lsp: starting");

    while let Some(msg) = read_message(&mut reader)? {
        let method = match msg.method.as_deref() {
            Some(m) => m,
            None => continue,
        };

        match method {
            "initialize" => {
                let resp = server.initialize(msg.id);
                send_response(&mut writer, &resp)?;
            }
            "initialized" => {
                // Client acknowledgement; nothing to do.
            }
            "textDocument/didOpen" => {
                if let Some(params) = msg.params
                    && let Ok(p) = serde_json::from_value::<DidOpenParams>(params)
                    && let Some(notif) = server.did_open(p)
                {
                    send_notification(&mut writer, &notif)?;
                }
            }
            "textDocument/didChange" => {
                if let Some(params) = msg.params
                    && let Ok(p) = serde_json::from_value::<DidChangeParams>(params)
                    && let Some(notif) = server.did_change(p)
                {
                    send_notification(&mut writer, &notif)?;
                }
            }
            "textDocument/hover" => {
                if let Some(params) = msg.params
                    && let Ok(p) = serde_json::from_value::<HoverParams>(params)
                {
                    let result = server.handle_hover(p);
                    let resp = RpcResponse {
                        jsonrpc: "2.0".into(),
                        id: msg.id,
                        result: Some(
                            result
                                .and_then(|h| serde_json::to_value(h).ok())
                                .unwrap_or(serde_json::Value::Null),
                        ),
                        error: None,
                    };
                    send_response(&mut writer, &resp)?;
                }
            }
            "textDocument/completion" => {
                if let Some(params) = msg.params
                    && let Ok(_p) = serde_json::from_value::<CompletionParams>(params)
                {
                    let items = server.handle_completion();
                    let resp = RpcResponse {
                        jsonrpc: "2.0".into(),
                        id: msg.id,
                        result: serde_json::to_value(items).ok(),
                        error: None,
                    };
                    send_response(&mut writer, &resp)?;
                }
            }
            "shutdown" => {
                let resp = server.shutdown(msg.id);
                send_response(&mut writer, &resp)?;
            }
            "exit" => {
                break;
            }
            _ => {
                // Unknown method — respond with MethodNotFound for requests (those with id)
                if msg.id.is_some() {
                    let resp = RpcResponse {
                        jsonrpc: "2.0".into(),
                        id: msg.id,
                        result: None,
                        error: Some(grift_lsp::RpcError {
                            code: -32601,
                            message: format!("Method not found: {method}"),
                        }),
                    };
                    send_response(&mut writer, &resp)?;
                }
            }
        }
    }

    eprintln!("grift-lsp: shutting down");
    Ok(())
}
