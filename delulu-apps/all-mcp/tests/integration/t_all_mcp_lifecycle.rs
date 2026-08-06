//!  Delulu All-MCP — e2e process lifecycle over stdio and HTTP transports
//!
//!  Copyright (C) 2026  Mamy Ratsimbazafy
//!
//!  This program is free software: you can redistribute it and/or modify
//!  it under the terms of the GNU Affero General Public License as published by
//!  the Free Software Foundation, either version 3 of the License, or
//!  (at your option) any later version.
//!
//!  This program is distributed in the hope that it will be useful,
//!  but WITHOUT ANY WARRANTY; without even the implied warranty of
//!  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//!  GNU Affero General Public License for more details.
//!
//!  You should have received a copy of the GNU Affero General Public License
//!  along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Spawns `delulu-all-mcp stdio`, completes the MCP initialize handshake, and
//! asserts the process lifecycle fixes in `delulu-mcp-server-helper`:
//!
//! - **T1:** closing stdin (EOF) after the handshake makes the server exit
//!   promptly with code 0 — it no longer waits for Ctrl+C forever.
//! - **T2 (unix-only):** SIGTERM (what Docker `docker stop` / Kubernetes
//!   send) triggers the same graceful shutdown.
//! - **T3:** SIGINT (Ctrl+C) while connected still triggers graceful shutdown.
//! - **T4 (unix-only):** SIGTERM on the HTTP transport (the primary container
//!   surface) triggers the same graceful shutdown.
//!
//! Notes:
//! - Closing stdin BEFORE the initialize handshake makes `serve_server` return
//!   `Err(ConnectionClosed)` (non-zero exit); that pre-handshake path is
//!   unchanged and out of scope, so every test completes the handshake first.
//! - `std::process::Child::kill()` sends SIGKILL, not SIGTERM, so signal
//!   delivery uses `libc::kill(2)` with the specific signal.

mod mcp_helpers;

use mcp_helpers::{find_binary, mcp_initialize, spawn_stdio_server, stream_stderr_to_console};
use std::time::Duration;
use tokio::process::{Child, ChildStdin, ChildStdout};

/// How long the child has to exit after the trigger (EOF / signal).
const EXIT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long to wait for the HTTP server to start accepting connections.
const READY_TIMEOUT: Duration = Duration::from_secs(10);
/// Settle time before/after delivering a shutdown signal.
///
/// The signal handlers are registered on the shutdown select's first poll — a
/// few instructions *after* the externally observable readiness event (the
/// MCP handshake response for stdio, the first TCP accept for http). A signal
/// delivered before registration kills the child by the default (terminating)
/// disposition. The registration window is sub-ms, so the 300 ms settle makes
/// the race practically unreachable; `signal_shutdown_test` additionally
/// retries the whole scenario (bounded) when the child was killed by a signal
/// instead of exiting gracefully.
const SIGNAL_SETTLE: Duration = Duration::from_millis(300);

/// Spawn `delulu-all-mcp stdio` and complete the MCP initialize handshake.
///
/// # Pre
/// The release binary exists (see `find_binary`).
///
/// # Post
/// The server is serving; `serve_server` has returned and `run_stdio` is
/// waiting on its shutdown select.
async fn spawn_initialized_stdio() -> (Child, ChildStdin, ChildStdout) {
    let path = find_binary("delulu-all-mcp")
        .unwrap_or_else(|e| panic!("find_binary(delulu-all-mcp): {e}"));
    let (child, mut stdin, mut stdout) = spawn_stdio_server(&path)
        .await
        .unwrap_or_else(|e| panic!("failed to spawn delulu-all-mcp: {e}"));
    mcp_initialize(&mut stdin, &mut stdout)
        .await
        .unwrap_or_else(|e| panic!("MCP initialize failed: {e}"));
    (child, stdin, stdout)
}

/// Wait for the child to exit within `EXIT_TIMEOUT`, asserting a graceful
/// (exit code 0) shutdown.
///
/// Used by the EOF test (T1), which never delivers a signal, so a child
/// terminated by a signal here would indicate an unexpected external kill
/// rather than a shutdown bug.
async fn expect_graceful_exit(child: &mut Child, what: &str) {
    let status = tokio::time::timeout(EXIT_TIMEOUT, child.wait())
        .await
        .unwrap_or_else(|_| {
            let _ = child.start_kill();
            panic!(
                "{what}: server did not exit within {}s",
                EXIT_TIMEOUT.as_secs()
            );
        })
        .unwrap_or_else(|e| panic!("{what}: failed to wait for server exit: {e}"));
    if status.code().is_none() {
        panic!(
            "{what}: server was terminated by a signal ({status:?}) — no shutdown \
             signal was sent by this test, so this is an unexpected external kill"
        );
    }
    assert!(
        status.success(),
        "{what}: server should exit with code 0 (graceful shutdown), got: {status:?}"
    );
}

/// Run one signal-shutdown scenario with up to 3 retries.
///
/// Spawns via `spawn`, settles so the child's signal handlers are registered,
/// sends `sig` (re-sent once if the child is still running after the settle),
/// and asserts a graceful exit (code 0). If the child was terminated by a
/// signal instead — the shutdown signal raced handler registration and the
/// default disposition killed it — the whole scenario is retried (bounded;
/// the registration window is sub-ms, so 3 attempts make the flake
/// probability negligible).
///
/// `spawn` returns `(Child, G)` where `G` is an opaque guard that keeps any
/// handles alive for the duration of the attempt: for stdio tests this is the
/// `(ChildStdin, ChildStdout)` pair (dropping stdin would end the serve loop
/// via EOF and make the test vacuous); for the HTTP test it is `()`.
#[cfg(unix)]
async fn signal_shutdown_test<F, Fut, G>(sig: libc::c_int, what: &str, spawn: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = (Child, G)>,
    G: Send,
{
    for attempt in 1..=3u32 {
        let (mut child, _guard) = spawn().await;
        tokio::time::sleep(SIGNAL_SETTLE).await;

        let pid = child.id().expect("child pid") as libc::pid_t;
        // Safety: `pid` is the live child process we spawned.
        let rc = unsafe { libc::kill(pid, sig) };
        assert_eq!(rc, 0, "kill({what}) on pid {pid} failed");

        // If still running after the settle, the child may have been
        // descheduled: re-send once, then await the final exit (tokio caches
        // the exit status, so the wait below is idempotent).
        if tokio::time::timeout(SIGNAL_SETTLE, child.wait())
            .await
            .is_err()
        {
            let rc = unsafe { libc::kill(pid, sig) };
            assert_eq!(rc, 0, "re-kill({what}) on pid {pid} failed");
        }

        let status = tokio::time::timeout(EXIT_TIMEOUT, child.wait())
            .await
            .unwrap_or_else(|_| {
                let _ = child.start_kill();
                panic!(
                    "{what}: server did not exit within {}s",
                    EXIT_TIMEOUT.as_secs()
                );
            })
            .expect("failed to wait for child");
        match status.code() {
            Some(0) => return, // graceful shutdown confirmed
            None => {
                // Terminated by a signal: the shutdown signal raced handler
                // registration and the default disposition killed the child.
                tracing::warn!(
                    attempt,
                    "{what}: signal raced handler registration; retrying"
                );
            }
            Some(code) => panic!("{what}: unexpected exit code {code}"),
        }
    }
    panic!("{what}: signal raced handler registration on 3 consecutive attempts");
}

/// Wait until the HTTP server accepts TCP connections on `addr`.
///
/// The listener bind happens before the graceful-shutdown future (and thus
/// the signal-handler registration) is polled, so connect-success is the
/// strongest externally observable readiness signal for the HTTP path.
async fn wait_for_http_ready(addr: std::net::SocketAddr) {
    let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(_) => return,
            Err(_e) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => panic!("HTTP server never accepted connections on {addr}: {e}"),
        }
    }
}

/// T1: closing stdin (EOF) after the handshake must make the server exit
/// promptly with code 0 — it must not wait for Ctrl+C.
#[tokio::test(flavor = "multi_thread")]
async fn stdio_exits_when_stdin_closed_after_handshake() {
    let (mut child, stdin, _stdout) = spawn_initialized_stdio().await;

    // Dropping the piped stdin closes the write end; the server's stdio
    // transport sees EOF and the serve loop terminates.
    drop(stdin);

    expect_graceful_exit(&mut child, "stdin EOF").await;
}

/// T3: SIGINT (Ctrl+C) while a client is connected must still trigger a
/// graceful shutdown.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn stdio_exits_gracefully_on_sigint() {
    signal_shutdown_test(libc::SIGINT, "SIGINT", || async {
        let (child, stdin, stdout) = spawn_initialized_stdio().await;
        (child, (stdin, stdout))
    })
    .await;
}

/// T2: SIGTERM (what Docker `docker stop` / Kubernetes send on container
/// stop) must trigger the same graceful shutdown as SIGINT.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn stdio_exits_gracefully_on_sigterm() {
    signal_shutdown_test(libc::SIGTERM, "SIGTERM", || async {
        let (child, stdin, stdout) = spawn_initialized_stdio().await;
        (child, (stdin, stdout))
    })
    .await;
}

/// T4: SIGTERM on the HTTP transport — the primary container surface (Docker
/// `docker stop` / Kubernetes send SIGTERM on container stop) — must trigger
/// the same graceful shutdown as stdio: the process exits with code 0.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn http_exits_gracefully_on_sigterm() {
    signal_shutdown_test(libc::SIGTERM, "HTTP SIGTERM", || async {
        // Bind an ephemeral port to learn a free one, then free it for the child.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");
        drop(listener);

        let path = find_binary("delulu-all-mcp")
            .unwrap_or_else(|e| panic!("find_binary(delulu-all-mcp): {e}"));
        let port = addr.port().to_string();
        let mut child = tokio::process::Command::new(&path)
            .args(["http", "--host", "127.0.0.1", "--port", port.as_str()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn http server");
        // Drain stderr so the child never blocks on a full pipe buffer.
        let stderr = child.stderr.take().unwrap();
        std::mem::drop(stream_stderr_to_console(stderr));

        // Behavioral readiness: the listener accepts connections once bound.
        wait_for_http_ready(addr).await;
        (child, ())
    })
    .await;
}
