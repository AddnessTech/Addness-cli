use std::{
    io::{Read, Write},
    sync::mpsc,
    time::{Duration, Instant},
};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use ratatui::crossterm::{
    event::{self, Event, KeyModifiers, MouseEventKind},
    terminal::{disable_raw_mode, enable_raw_mode},
};

const CHILD_ENV: &str = "ADDNESS_CROSSTERM_MOUSE_PTY_CHILD";
const READY: &str = "READY:crossterm-mouse-pty";
const EVENT_PREFIX: &str = "EVENT:crossterm-mouse-pty";

#[test]
fn crossterm_mouse_pty_child() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }

    enable_raw_mode().expect("child should enable raw mode on its PTY");
    println!("{READY}");
    std::io::stdout().flush().unwrap();

    let event = event::read().expect("child should read one terminal event");
    disable_raw_mode().ok();

    let Event::Mouse(mouse) = event else {
        panic!("expected mouse event, got {event:?}");
    };
    let kind = match mouse.kind {
        MouseEventKind::ScrollUp => "ScrollUp",
        MouseEventKind::ScrollDown => "ScrollDown",
        MouseEventKind::ScrollLeft => "ScrollLeft",
        MouseEventKind::ScrollRight => "ScrollRight",
        other => panic!("expected wheel event, got {other:?}"),
    };
    println!(
        "{EVENT_PREFIX}:{kind}:col={}:row={}:shift={}:alt={}:ctrl={}",
        mouse.column,
        mouse.row,
        mouse.modifiers.contains(KeyModifiers::SHIFT),
        mouse.modifiers.contains(KeyModifiers::ALT),
        mouse.modifiers.contains(KeyModifiers::CONTROL),
    );
}

#[test]
fn crossterm_reads_sgr_mouse_wheel_from_pty() {
    if std::env::var_os(CHILD_ENV).is_some() {
        return;
    }

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();

    let exe = std::env::current_exe().unwrap();
    let mut cmd = CommandBuilder::new(exe);
    cmd.arg("--exact");
    cmd.arg("crossterm_mouse_pty_child");
    cmd.arg("--nocapture");
    cmd.env(CHILD_ENV, "1");

    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();

    let (tx, rx) = mpsc::channel::<String>();
    let reader_thread = std::thread::spawn(move || {
        let mut buf = [0_u8; 512];
        let mut output = String::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    output.push_str(&String::from_utf8_lossy(&buf[..n]));
                    let _ = tx.send(output.clone());
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        output
    });

    let mut output = String::new();
    let ready_deadline = Instant::now() + Duration::from_secs(3);
    while !output.contains(READY) {
        let remaining = ready_deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "child did not become ready:\n{output}"
        );
        output = rx
            .recv_timeout(remaining)
            .unwrap_or_else(|_| output.clone());
    }

    // SGR mouse wheel-up at terminal coordinate 21,9. Crossterm exposes zero-based 20,8.
    writer.write_all(b"\x1b[<64;21;9M").unwrap();
    writer.flush().unwrap();

    let exit_deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= exit_deadline {
            let _ = child.kill();
            panic!("child did not exit after SGR mouse input:\n{output}");
        }
        if let Ok(next) = rx.recv_timeout(Duration::from_millis(10)) {
            output = next;
        }
    }

    drop(writer);
    let output = reader_thread.join().unwrap();

    assert!(
        output.contains("EVENT:crossterm-mouse-pty:ScrollUp:col=20:row=8"),
        "SGR wheel sequence should become a crossterm ScrollUp mouse event:\n{output}"
    );
}
