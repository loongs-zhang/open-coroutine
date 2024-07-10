use open_coroutine::task;
use std::io::{ErrorKind, IoSlice, IoSliceMut, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub fn start_server<A: ToSocketAddrs>(addr: A, server_finished: Arc<(Mutex<bool>, Condvar)>) {
    let listener = TcpListener::bind(addr).expect("start server failed");
    for stream in listener.incoming() {
        let mut socket = stream.expect("accept new connection failed");
        let mut buffer1 = [0; 256];
        for _ in 0..3 {
            assert_eq!(12, socket.read(&mut buffer1).expect("recv failed"));
            println!("Server Received: {}", String::from_utf8_lossy(&buffer1));
            assert_eq!(256, socket.write(&buffer1).expect("send failed"));
            println!("Server Send");
        }
        let mut buffer2 = [0; 256];
        for _ in 0..3 {
            let mut buffers = [IoSliceMut::new(&mut buffer1), IoSliceMut::new(&mut buffer2)];
            assert_eq!(
                26,
                socket.read_vectored(&mut buffers).expect("readv failed")
            );
            println!(
                "Server Received Multiple: {}{}",
                String::from_utf8_lossy(&buffer1),
                String::from_utf8_lossy(&buffer2)
            );
            let responses = [IoSlice::new(&buffer1), IoSlice::new(&buffer2)];
            assert_eq!(
                512,
                socket.write_vectored(&responses).expect("writev failed")
            );
            println!("Server Send Multiple");
        }
        println!("Server Shutdown Write");
        if socket.shutdown(Shutdown::Write).is_ok() {
            println!("Server Closed Connection");
            let (lock, cvar) = &*server_finished;
            let mut pending = lock.lock().unwrap();
            *pending = false;
            cvar.notify_one();
            println!("Server Closed");
            return;
        }
    }
}

pub fn start_co_server<A: ToSocketAddrs>(addr: A, server_finished: Arc<(Mutex<bool>, Condvar)>) {
    let listener = TcpListener::bind(addr).expect("start server failed");
    for stream in listener.incoming() {
        _ = task!(
            |_, mut socket| {
                let mut buffer1 = [0; 256];
                for _ in 0..3 {
                    assert_eq!(12, socket.read(&mut buffer1).expect("recv failed"));
                    println!("Server Received: {}", String::from_utf8_lossy(&buffer1));
                    assert_eq!(256, socket.write(&buffer1).expect("send failed"));
                    println!("Server Send");
                }
                let mut buffer2 = [0; 256];
                for _ in 0..3 {
                    let mut buffers =
                        [IoSliceMut::new(&mut buffer1), IoSliceMut::new(&mut buffer2)];
                    assert_eq!(
                        26,
                        socket.read_vectored(&mut buffers).expect("readv failed")
                    );
                    println!(
                        "Server Received Multiple: {}{}",
                        String::from_utf8_lossy(&buffer1),
                        String::from_utf8_lossy(&buffer2)
                    );
                    let responses = [IoSlice::new(&buffer1), IoSlice::new(&buffer2)];
                    assert_eq!(
                        512,
                        socket.write_vectored(&responses).expect("writev failed")
                    );
                    println!("Server Send Multiple");
                }
                println!("Server Shutdown Write");
                if socket.shutdown(Shutdown::Write).is_ok() {
                    println!("Server Closed Connection");
                    let (lock, cvar) = &*server_finished;
                    let mut pending = lock.lock().unwrap();
                    *pending = false;
                    cvar.notify_one();
                    println!("Server Closed");
                }
            },
            stream.expect("accept new connection failed"),
        );
    }
}

pub fn start_client<A: ToSocketAddrs>(addr: A) {
    let mut stream = connect_timeout(addr, Duration::from_secs(3)).expect("connect failed");
    let mut buffer1 = [0; 256];
    for i in 0..3 {
        assert_eq!(
            12,
            stream
                .write(format!("RequestPart{i}").as_ref())
                .expect("send failed")
        );
        println!("Client Send");
        assert_eq!(256, stream.read(&mut buffer1).expect("recv failed"));
        println!("Client Received: {}", String::from_utf8_lossy(&buffer1));
    }
    let mut buffer2 = [0; 256];
    for i in 0..3 {
        let request1 = format!("RequestPart{i}1");
        let request2 = format!("RequestPart{i}2");
        let requests = [
            IoSlice::new(request1.as_ref()),
            IoSlice::new(request2.as_ref()),
        ];
        assert_eq!(26, stream.write_vectored(&requests).expect("writev failed"));
        println!("Client Send Multiple");
        let mut buffers = [IoSliceMut::new(&mut buffer1), IoSliceMut::new(&mut buffer2)];
        assert_eq!(
            512,
            stream.read_vectored(&mut buffers).expect("readv failed")
        );
        println!(
            "Client Received Multiple: {}{}",
            String::from_utf8_lossy(&buffer1),
            String::from_utf8_lossy(&buffer2)
        );
    }
    println!("Client Shutdown Write");
    stream.shutdown(Shutdown::Write).expect("shutdown failed");
    println!("Client Closed");
}

fn connect_timeout<A: ToSocketAddrs>(addr: A, timeout: Duration) -> std::io::Result<TcpStream> {
    let mut last_err = None;
    for addr in addr.to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(l) => return Ok(l),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        std::io::Error::new(
            ErrorKind::InvalidInput,
            "could not resolve to any addresses",
        )
    }))
}

pub fn start_co_client<A: ToSocketAddrs>(addr: A) {
    _ = task!(
        |_, mut stream| {
            let mut buffer1 = [0; 256];
            for i in 0..3 {
                assert_eq!(
                    12,
                    stream
                        .write(format!("RequestPart{i}").as_ref())
                        .expect("send failed")
                );
                println!("Client Send");
                assert_eq!(256, stream.read(&mut buffer1).expect("recv failed"));
                println!("Client Received: {}", String::from_utf8_lossy(&buffer1));
            }
            let mut buffer2 = [0; 256];
            for i in 0..3 {
                let request1 = format!("RequestPart{i}1");
                let request2 = format!("RequestPart{i}2");
                let requests = [
                    IoSlice::new(request1.as_ref()),
                    IoSlice::new(request2.as_ref()),
                ];
                assert_eq!(26, stream.write_vectored(&requests).expect("writev failed"));
                println!("Client Send Multiple");
                let mut buffers = [IoSliceMut::new(&mut buffer1), IoSliceMut::new(&mut buffer2)];
                assert_eq!(
                    512,
                    stream.read_vectored(&mut buffers).expect("readv failed")
                );
                println!(
                    "Client Received Multiple: {}{}",
                    String::from_utf8_lossy(&buffer1),
                    String::from_utf8_lossy(&buffer2)
                );
            }
            println!("Client Shutdown Write");
            stream.shutdown(Shutdown::Write).expect("shutdown failed");
            println!("Client Closed");
        },
        connect_timeout(addr, Duration::from_secs(3)).expect("connect failed"),
    );
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("1970-01-01 00:00:00 UTC was {} seconds ago!")
        .as_nanos() as u64
}

pub fn sleep_test(millis: u64) {
    _ = task!(
        move |_, _| {
            println!("[coroutine1] {millis} launched");
        },
        (),
    );
    _ = task!(
        move |_, _| {
            println!("[coroutine2] {millis} launched");
        },
        (),
    );
    let start = now();
    std::thread::sleep(Duration::from_millis(millis));
    let end = now();
    assert!(end - start >= millis, "Time consumption less than expected");
}

pub fn sleep_test_co(millis: u64) {
    _ = task!(
        move |_, _| {
            let start = now();
            std::thread::sleep(Duration::from_millis(millis));
            let end = now();
            assert!(end - start >= millis, "Time consumption less than expected");
            println!("[coroutine1] {millis} launched");
        },
        (),
    );
    _ = task!(
        move |_, _| {
            std::thread::sleep(Duration::from_millis(500));
            println!("[coroutine2] {millis} launched");
        },
        (),
    );
    std::thread::sleep(Duration::from_millis(millis + 500));
}
