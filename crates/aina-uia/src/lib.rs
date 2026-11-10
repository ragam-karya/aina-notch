// UI Automation for the Brain and for what the user marks. v1 ran a C# helper process and talked to it
// one JSON line per request; v2 runs the same commands in-process on a dedicated COM thread (UIA calls
// must stay on one apartment). The thread starts on first use and ends after a while idle, so it costs
// nothing while aina rests. A request that hangs (a frozen app can block UIA calls) fails after a
// timeout, and the next request gets a fresh thread, as v1 restarted its helper.
//
// Commands and their JSON are v1's (see driver.rs): list_windows, get_window_state, click, set_value,
// focus_window, element_at.

#[cfg(windows)]
pub mod driver;

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

pub const TIMEOUT: Duration = Duration::from_secs(8);
pub const IDLE: Duration = Duration::from_secs(60);

/// Runs one command; created on (and kept to) the worker thread.
pub type Handler = Box<dyn FnMut(&str, &Value) -> Result<Value, String>>;
pub type MakeHandler = Arc<dyn Fn() -> Result<Handler, String> + Send + Sync>;

struct Job {
    cmd: String,
    args: Value,
    reply: Sender<Result<Value, String>>,
}

pub struct Uia {
    make: MakeHandler,
    worker: Mutex<Option<Sender<Job>>>,
    timeout: Duration,
    idle: Duration,
}

impl Default for Uia {
    fn default() -> Self {
        Uia::new()
    }
}

impl Uia {
    /// The real UI Automation (Windows).
    #[cfg(windows)]
    pub fn new() -> Uia {
        Uia::with_handler(Arc::new(|| driver::Driver::new().map(|mut d| Box::new(move |cmd: &str, args: &Value| d.run(cmd, args)) as Handler)), TIMEOUT, IDLE)
    }

    #[cfg(not(windows))]
    pub fn new() -> Uia {
        Uia::with_handler(Arc::new(|| Err("UI Automation is not available here; use the computer tools.".into())), TIMEOUT, IDLE)
    }

    pub fn with_handler(make: MakeHandler, timeout: Duration, idle: Duration) -> Uia {
        Uia { make, worker: Mutex::new(None), timeout, idle }
    }

    fn spawn(&self) -> Sender<Job> {
        let (tx, rx): (Sender<Job>, Receiver<Job>) = channel();
        let make = self.make.clone();
        let idle = self.idle;
        std::thread::spawn(move || {
            let mut handler = match make() {
                Ok(h) => h,
                Err(e) => {
                    // Answer whatever is queued with the reason, then end.
                    while let Ok(job) = rx.recv_timeout(Duration::from_millis(50)) {
                        let _ = job.reply.send(Err(e.clone()));
                    }
                    return;
                }
            };
            loop {
                match rx.recv_timeout(idle) {
                    Ok(job) => {
                        let _ = job.reply.send(handler(&job.cmd, &job.args));
                    }
                    Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        tx
    }

    /// Runs a command, waiting at most `timeout` (default 8 s; a short one suits element_at).
    pub fn request(&self, cmd: &str, args: Value, timeout: Option<Duration>) -> Result<Value, String> {
        let (reply, answer) = channel();
        let mut job = Job { cmd: cmd.into(), args, reply };
        // The worker may have ended (idle, or after a hang); one retry with a fresh one.
        for _ in 0..2 {
            let tx = {
                let mut w = self.worker.lock().unwrap();
                w.get_or_insert_with(|| self.spawn()).clone()
            };
            match tx.send(job) {
                Ok(()) => break,
                Err(back) => {
                    job = back.0;
                    *self.worker.lock().unwrap() = None;
                }
            }
        }
        match answer.recv_timeout(timeout.unwrap_or(self.timeout)) {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) => {
                // A hung UIA call blocks the thread; the next request starts a fresh one.
                *self.worker.lock().unwrap() = None;
                Err("The app did not answer UI Automation in time (it may be busy or frozen). Use the computer tools for it.".into())
            }
            Err(RecvTimeoutError::Disconnected) => {
                *self.worker.lock().unwrap() = None;
                Err("The UI Automation helper stopped.".into())
            }
        }
    }

    /// Ends the worker (its refs go with it).
    pub fn stop(&self) {
        *self.worker.lock().unwrap() = None;
    }
}

// Same cases as test/uia.test.js, with a fake handler in place of the helper process.
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn requests_get_their_replies_and_errors_become_errs() {
        let make: MakeHandler = Arc::new(|| {
            Ok(Box::new(|cmd: &str, _: &Value| if cmd == "list_windows" { Ok(json!([{ "title": "Calculator" }])) } else { Err("Unknown ref \"e9\".".into()) }) as Handler)
        });
        let u = Uia::with_handler(make, Duration::from_secs(2), IDLE);
        assert_eq!(u.request("list_windows", json!({}), None), Ok(json!([{ "title": "Calculator" }])));
        assert!(u.request("click", json!({ "ref": "e9" }), None).unwrap_err().contains("Unknown ref"));
    }

    #[test]
    fn a_hung_app_times_out_and_the_next_request_gets_a_fresh_worker() {
        let started = Arc::new(AtomicUsize::new(0));
        let hang = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (s, h) = (started.clone(), hang.clone());
        let make: MakeHandler = Arc::new(move || {
            s.fetch_add(1, Ordering::SeqCst);
            let h = h.clone();
            Ok(Box::new(move |_: &str, _: &Value| {
                if h.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(400));
                }
                Ok(json!("ok"))
            }) as Handler)
        });
        let u = Uia::with_handler(make, Duration::from_millis(50), IDLE);
        assert!(u.request("get_window_state", json!({}), None).unwrap_err().contains("did not answer"));
        hang.store(false, Ordering::SeqCst);
        assert_eq!(u.request("list_windows", json!({}), Some(Duration::from_secs(2))), Ok(json!("ok")));
        assert_eq!(started.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn an_idle_worker_ends_and_comes_back() {
        let started = Arc::new(AtomicUsize::new(0));
        let s = started.clone();
        let make: MakeHandler = Arc::new(move || {
            s.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(|_: &str, _: &Value| Ok(json!(1))) as Handler)
        });
        let u = Uia::with_handler(make, Duration::from_secs(2), Duration::from_millis(30));
        assert_eq!(u.request("x", json!({}), None), Ok(json!(1)));
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(u.request("x", json!({}), None), Ok(json!(1)));
        assert_eq!(started.load(Ordering::SeqCst), 2);
    }
}
