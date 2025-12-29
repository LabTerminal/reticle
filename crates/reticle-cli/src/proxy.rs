//! stdio proxy implementation for CLI
//!
//! This module implements a bidirectional stdio proxy that wraps an MCP server
//! process and forwards all traffic while emitting events.
//!
//! Supports:
//! - Forwarding stdin/stdout/stderr between parent and child
//! - Emitting telemetry events to the Reticle Hub
//! - Receiving inject commands from the Hub to send messages to the MCP server
//! - Proper signal handling for clean shutdown

use reticle_core::events::{EventSink, InjectReceiver};
use reticle_core::protocol::{Direction, LogEntry, MessageType};
use reticle_core::session_names::create_session_id;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// Run a stdio proxy for an MCP server
///
/// If `inject_rx` is provided, the proxy will listen for inject commands
/// from the Reticle Hub and forward them to the MCP server's stdin.
pub async fn run_stdio_proxy<E: EventSink>(
    command: &str,
    args: &[&str],
    server_name: &str,
    event_sink: E,
    inject_rx: Option<InjectReceiver>,
) -> Result<i32, String> {
    // Generate session ID with beautiful name
    let session = create_session_id(Some(server_name));
    let session_id = session.id.clone();

    // Emit session started (use display name for UI)
    event_sink
        .emit_session_started(&session_id, &session.name)
        .await
        .map_err(|e| format!("Failed to emit session started: {e}"))?;

    // Start the child process
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start process: {e}"))?;

    let child_stdin = child.stdin.take().ok_or("Failed to get child stdin")?;
    let child_stdout = child.stdout.take().ok_or("Failed to get child stdout")?;
    let child_stderr = child.stderr.take().ok_or("Failed to get child stderr")?;

    // Set up readers
    let mut stdout_reader = BufReader::new(child_stdout).lines();
    let mut stderr_reader = BufReader::new(child_stderr).lines();
    let mut stdin_reader = BufReader::new(tokio::io::stdin()).lines();
    let mut child_stdin = child_stdin;
    let mut inject_rx = inject_rx;

    let mut log_counter = 0u64;

    // Set up unified signal handler for clean shutdown
    // On Unix, we handle SIGTERM, SIGHUP, and SIGINT
    // On Windows, we only handle Ctrl+C
    #[cfg(unix)]
    let shutdown_signal = async {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
        let mut sighup = signal(SignalKind::hangup()).expect("SIGHUP handler");
        let mut sigint = signal(SignalKind::interrupt()).expect("SIGINT handler");
        tokio::select! {
            _ = sigterm.recv() => "SIGTERM",
            _ = sighup.recv() => "SIGHUP",
            _ = sigint.recv() => "SIGINT",
        }
    };
    #[cfg(not(unix))]
    let shutdown_signal = async {
        tokio::signal::ctrl_c().await.ok();
        "SIGINT"
    };
    tokio::pin!(shutdown_signal);

    // Main proxy loop
    loop {
        tokio::select! {
            // Handle shutdown signals
            signal_name = &mut shutdown_signal => {
                tracing::info!("Received {}, shutting down...", signal_name);
                let _ = child.kill().await;
                let _ = event_sink.emit_session_ended(&session_id).await;
                let _ = child.wait().await;
                return Ok(match signal_name {
                    "SIGTERM" => 143,
                    "SIGHUP" => 129,
                    _ => 130, // SIGINT
                });
            }

            // Read from parent's stdin, write to child's stdin
            line = stdin_reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        log_counter += 1;
                        let log_id = format!("log-{log_counter}");

                        // Parse as JSON if possible and emit log event
                        tracing::trace!("stdin: {} bytes, log_id={}", line.len(), log_id);
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                            let entry = LogEntry::with_server(
                                log_id.clone(),
                                session_id.clone(),
                                Direction::In,
                                json,
                                server_name.to_string(),
                            );
                            if let Err(e) = event_sink.emit_log(&entry).await {
                                tracing::warn!("emit_log error: {}", e);
                            }
                        } else {
                            let entry = LogEntry::new_raw_with_server(
                                log_id.clone(),
                                session_id.clone(),
                                Direction::In,
                                line.clone(),
                                MessageType::Raw,
                                server_name.to_string(),
                            );
                            if let Err(e) = event_sink.emit_log(&entry).await {
                                tracing::warn!("emit_log error: {}", e);
                            }
                        }

                        // Forward to child
                        if let Err(e) = child_stdin.write_all(line.as_bytes()).await {
                            tracing::error!("Failed to write to child stdin: {}", e);
                            break;
                        }
                        if let Err(e) = child_stdin.write_all(b"\n").await {
                            tracing::error!("Failed to write newline to child stdin: {}", e);
                            break;
                        }
                        let _ = child_stdin.flush().await;
                    }
                    Ok(None) => {
                        // Parent stdin closed - this means the parent process exited
                        tracing::info!("Parent stdin closed, terminating child process...");
                        let _ = child.kill().await;
                        let _ = event_sink.emit_session_ended(&session_id).await;
                        // Give child a moment to clean up
                        let status = child.wait().await;
                        tracing::info!("Child process terminated: {:?}", status);
                        return Ok(0);
                    }
                    Err(e) => {
                        tracing::error!("Error reading stdin: {}", e);
                        break;
                    }
                }
            }

            // Read from child's stdout, write to parent's stdout
            line = stdout_reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        log_counter += 1;
                        let log_id = format!("log-{log_counter}");

                        // Parse as JSON if possible and emit log event
                        tracing::trace!("stdout: {} bytes, log_id={}", line.len(), log_id);
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                            let entry = LogEntry::with_server(
                                log_id.clone(),
                                session_id.clone(),
                                Direction::Out,
                                json,
                                server_name.to_string(),
                            );
                            if let Err(e) = event_sink.emit_log(&entry).await {
                                tracing::warn!("emit_log error: {}", e);
                            }
                        } else {
                            let entry = LogEntry::new_raw_with_server(
                                log_id.clone(),
                                session_id.clone(),
                                Direction::Out,
                                line.clone(),
                                MessageType::Raw,
                                server_name.to_string(),
                            );
                            if let Err(e) = event_sink.emit_log(&entry).await {
                                tracing::warn!("emit_log error: {}", e);
                            }
                        }

                        // Forward to parent stdout
                        println!("{line}");
                    }
                    Ok(None) => {
                        // Child stdout closed
                        tracing::info!("Child stdout closed");
                        break;
                    }
                    Err(e) => {
                        tracing::error!("Error reading child stdout: {}", e);
                        break;
                    }
                }
            }

            // Read from child's stderr, log it
            line = stderr_reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        log_counter += 1;
                        let log_id = format!("log-{log_counter}");

                        let entry = LogEntry::new_raw_with_server(
                            log_id,
                            session_id.clone(),
                            Direction::Out,
                            line.clone(),
                            MessageType::Stderr,
                            server_name.to_string(),
                        );
                        let _ = event_sink.emit_log(&entry).await;

                        // Forward to parent stderr
                        eprintln!("{line}");
                    }
                    Ok(None) => {
                        // Child stderr closed - this is expected
                    }
                    Err(e) => {
                        tracing::error!("Error reading child stderr: {}", e);
                    }
                }
            }

            // Handle inject commands from the Hub (if enabled)
            message = async {
                if let Some(ref mut rx) = inject_rx {
                    rx.recv().await
                } else {
                    // If no inject receiver, never complete this branch
                    std::future::pending::<Option<String>>().await
                }
            } => {
                if let Some(message) = message {
                    log_counter += 1;
                    let log_id = format!("inject-{log_counter}");

                    tracing::info!("Injecting message from Hub: {} bytes", message.len());

                    // Log the injected message
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&message) {
                        let entry = LogEntry::with_server(
                            log_id.clone(),
                            session_id.clone(),
                            Direction::In,
                            json,
                            server_name.to_string(),
                        );
                        let _ = event_sink.emit_log(&entry).await;
                    }

                    // Write to child's stdin
                    if let Err(e) = child_stdin.write_all(message.as_bytes()).await {
                        tracing::error!("Failed to inject message: {}", e);
                    } else {
                        if let Err(e) = child_stdin.write_all(b"\n").await {
                            tracing::error!("Failed to write newline after inject: {}", e);
                        }
                        let _ = child_stdin.flush().await;
                        tracing::debug!("Injected message successfully");
                    }
                }
            }

            // Check if child has exited
            status = child.wait() => {
                match status {
                    Ok(status) => {
                        tracing::info!("Child process exited with: {}", status);
                        let _ = event_sink.emit_session_ended(&session_id).await;
                        return Ok(status.code().unwrap_or(0));
                    }
                    Err(e) => {
                        tracing::error!("Error waiting for child: {}", e);
                        let _ = event_sink.emit_session_ended(&session_id).await;
                        return Err(format!("Error waiting for child: {e}"));
                    }
                }
            }
        }
    }

    // If we broke out of the loop (stdin/stdout error), kill the child and wait
    tracing::info!("Proxy loop ended, terminating child process...");
    let _ = child.kill().await;
    let status = child
        .wait()
        .await
        .map_err(|e| format!("Error waiting for child: {e}"))?;
    let _ = event_sink.emit_session_ended(&session_id).await;

    Ok(status.code().unwrap_or(0))
}
