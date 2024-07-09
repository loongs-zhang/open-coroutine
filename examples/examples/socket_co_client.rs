use open_coroutine_examples::{start_co_client, start_server};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[open_coroutine::main(event_loop_size = 2, max_size = 1)]
fn main() -> std::io::Result<()> {
    let addr = "127.0.0.1:8899";
    let server_finished_pair = Arc::new((Mutex::new(true), Condvar::new()));
    let server_finished = Arc::clone(&server_finished_pair);
    _ = std::thread::Builder::new()
        .name("crate_server".to_string())
        .spawn(move || start_server(addr, server_finished_pair))
        .expect("failed to spawn thread");
    _ = std::thread::Builder::new()
        .name("crate_co_client".to_string())
        .spawn(move || start_co_client(addr))
        .expect("failed to spawn thread");

    let (lock, cvar) = &*server_finished;
    let result = cvar
        .wait_timeout_while(
            lock.lock().unwrap(),
            Duration::from_secs(30),
            |&mut pending| pending,
        )
        .unwrap();
    if result.1.timed_out() {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "The coroutine client did not completed within the specified time",
        ))
    } else {
        Ok(())
    }
}
