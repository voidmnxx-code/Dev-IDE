//! A real pseudo-terminal session running a shell, rendered through [`Screen`].

use crate::screen::Screen;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub struct Session {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    screen: Arc<Mutex<Screen>>,
    version: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
}

impl Session {
    pub fn spawn(
        program: &str,
        args: &[String],
        cwd: &str,
        env: &[(String, String)],
        cols: u16,
        rows: u16,
    ) -> Result<Session, String> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd.cwd(cwd);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let screen = Arc::new(Mutex::new(Screen::new()));
        let version = Arc::new(AtomicU64::new(0));
        let alive = Arc::new(AtomicBool::new(true));
        {
            let (screen, version, alive) = (screen.clone(), version.clone(), alive.clone());
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            screen.lock().unwrap().feed(&buf[..n]);
                            version.fetch_add(1, Ordering::Release);
                        }
                    }
                }
                alive.store(false, Ordering::Release);
                version.fetch_add(1, Ordering::Release);
            });
        }
        Ok(Session {
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            screen,
            version,
            alive,
        })
    }

    pub fn write(&self, data: &[u8]) -> Result<(), String> {
        let mut w = self.writer.lock().unwrap();
        w.write_all(data).map_err(|e| e.to_string())?;
        w.flush().map_err(|e| e.to_string())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.master
            .lock()
            .unwrap()
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())
    }

    pub fn lines(&self) -> Vec<String> {
        self.screen.lock().unwrap().lines()
    }

    pub fn cursor(&self) -> (usize, usize) {
        self.screen.lock().unwrap().cursor()
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    pub fn kill(&self) {
        let _ = self.child.lock().unwrap().kill();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_for(s: &Session, needle: &str) -> bool {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            if s.lines().iter().any(|l| l.contains(needle)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn runs_commands_in_a_real_pty() {
        let dir = std::env::temp_dir();
        let s = Session::spawn("/bin/sh", &[], dir.to_str().unwrap(), &[], 80, 24).unwrap();
        s.write(b"echo $((6*7))marker\n").unwrap();
        assert!(wait_for(&s, "42marker"), "lines: {:?}", s.lines());
        assert!(s.is_alive());
        s.write(b"tty >/dev/null && echo istty\n").unwrap();
        assert!(wait_for(&s, "istty"));
    }

    #[test]
    fn exit_marks_session_dead_and_env_is_applied() {
        let dir = std::env::temp_dir();
        let env = vec![("IDE_TEST_VAR".to_string(), "hello_env".to_string())];
        let s = Session::spawn("/bin/sh", &[], dir.to_str().unwrap(), &env, 80, 24).unwrap();
        s.write(b"echo $IDE_TEST_VAR; exit\n").unwrap();
        assert!(wait_for(&s, "hello_env"));
        let end = Instant::now() + Duration::from_secs(5);
        while s.is_alive() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!s.is_alive());
    }

    #[test]
    fn ctrl_c_interrupts_a_running_command() {
        let dir = std::env::temp_dir();
        let s = Session::spawn("/bin/sh", &[], dir.to_str().unwrap(), &[], 80, 24).unwrap();
        s.write(b"sleep 30\n").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        s.write(&[0x03]).unwrap();
        s.write(b"echo after_ctrl_c\n").unwrap();
        assert!(wait_for(&s, "after_ctrl_c"));
    }
}
