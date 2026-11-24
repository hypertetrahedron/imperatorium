//! Settings > Remote machines: testing a host's connection and adding one
//! (`remote add`, run here without questions), and restarting the
//! dispatcher so it polls the new host.

use super::*;

/// The one add the window may have under way, and what it has said so far.
#[derive(Default)]
pub(super) struct AddJob {
    id: u64,
    host: String,
    log: Arc<Mutex<Vec<String>>>,
    /// None while it runs; then whether the host was written to config.json.
    result: Arc<Mutex<Option<bool>>>,
}

impl Shared {
    /// POST /api/remote/test: probe one host, configured or not.
    pub(super) fn remote_test(&self, p: &Map<String, Value>) -> Value {
        let host = p.get("host").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if let Err(e) = crate::remote_cli::check_host_name(&host) {
            return json!({"ok": false, "error": e});
        }
        // A configured host is tested as configured (its tunnel decides
        // whether hooks are needed); a new one as the form would add it.
        let configured = self.configured_host(&host);
        let tunnel = match &configured {
            Some(h) => h.get("tunnel").is_some_and(|t| truthy(Some(t))),
            None => p.get("tunnel").is_none_or(|t| truthy(Some(t))),
        };
        let mut out = crate::remote_cli::test_host(&host, tunnel);
        out["configured"] = json!(configured.is_some());
        out
    }

    fn configured_host(&self, name: &str) -> Option<Value> {
        load_config(self.home.as_deref())
            .get("remote")
            .and_then(|r| r.get("hosts"))
            .and_then(Value::as_array)?
            .iter()
            .map(|h| match h {
                Value::String(s) => json!({"name": s}),
                o => o.clone(),
            })
            .find(|h| h.get("name").and_then(Value::as_str) == Some(name))
    }

    /// POST /api/remote/key: key login, the part that needs no password.
    /// Makes this machine's key if it has none, and gives the one command
    /// that copies it to the host - run in a terminal, where ssh asks the
    /// host's password. The password never passes through here.
    pub(super) fn remote_key(&self, p: &Map<String, Value>) -> Value {
        let host = p.get("host").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if let Err(e) = crate::remote_cli::check_host_name(&host) {
            return json!({"ok": false, "error": e});
        }
        match crate::remote_cli::ensure_key() {
            Ok((key, made)) => json!({"ok": true, "made": made, "key": key.to_string_lossy(),
                                      "command": crate::remote_cli::copy_key_command(&host, &key)}),
            Err(e) => json!({"ok": false, "error": e}),
        }
    }

    /// POST /api/remote/add: start `remote add HOST` in the background. The
    /// window follows it with GET /api/remote/job.
    pub(super) fn remote_add_start(&self, p: &Map<String, Value>) -> Value {
        let host = p.get("host").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if let Err(e) = crate::remote_cli::check_host_name(&host) {
            return json!({"ok": false, "error": e});
        }
        let label = p.get("label").and_then(Value::as_str).map(str::to_string);
        let tunnel = p.get("tunnel").is_none_or(|t| truthy(Some(t)));
        let install_claude = truthy(p.get("install_claude"));
        let mut job = self.add_job.lk();
        if job.result.lk().is_none() && !job.host.is_empty() {
            return json!({"ok": false, "error": format!("{} is still being added", job.host)});
        }
        let id = job.id + 1;
        *job = AddJob { id, host: host.clone(), ..Default::default() };
        let (log, result) = (Arc::clone(&job.log), Arc::clone(&job.result));
        let home = self.home.clone();
        let me = self.me.get().cloned();
        let spawned = std::thread::Builder::new().name("remote-add".into()).spawn(move || {
            let sink = Arc::clone(&log);
            let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::remote_cli::add_for_window(&host, label.as_deref(), tunnel, install_claude, home.as_deref(), move |s| {
                    // A step heading comes as "\n== title": one line each.
                    sink.lk().extend(s.split('\n').filter(|l| !l.is_empty()).map(str::to_string));
                })
            }))
            .unwrap_or_else(|_| {
                log.lk().push("   the add stopped unexpectedly".into());
                false
            });
            if ok {
                // The fleet is read at start: the new host waits on a restart.
                if let Some(s) = me.and_then(|w| w.upgrade()) {
                    s.restart_needed.lk().insert("dispatcher".into());
                }
            }
            *result.lk() = Some(ok);
        });
        if let Err(e) = spawned {
            *job.result.lk() = Some(false);
            return json!({"ok": false, "error": format!("cannot start: {e}")});
        }
        json!({"ok": true, "job": id, "host": job.host})
    }

    /// GET /api/remote/job: the add under way or last finished.
    pub(super) fn remote_add_status(&self) -> Value {
        let mut out = {
            let job = self.add_job.lk();
            if job.host.is_empty() {
                return json!({"ok": true, "job": null});
            }
            let result = *job.result.lk();
            json!({"ok": true, "job": job.id, "host": job.host, "running": result.is_none(),
                   "added": result == Some(true), "log": *job.log.lk()})
        };
        // Asked of the service manager, so only once there is an answer.
        if out["added"] == json!(true) {
            out["can_restart"] = json!(self.can_restart());
        }
        out
    }

    /// Only the live dispatcher, run by its own unit, restarts itself: a
    /// test's or a foreground copy would restart the machine's real one.
    fn can_restart(&self) -> bool {
        self.serves_live
            && self.store.is_some()
            && crate::service::Service::new("Imperatorium".into(), std::env::current_exe().unwrap_or_default(), paths::config_home(self.home.as_deref()), vec![])
                .is_installed("Dispatcher")
    }

    /// POST /api/remote/restart: `imperatorium restart dispatcher`, in a
    /// process that outlives this one. The window reloads when the new
    /// dispatcher answers.
    pub(super) fn restart_self(&self) -> Value {
        if !self.can_restart() {
            return json!({"ok": false, "error": "this dispatcher is not the one the service runs: restart it yourself (imperatorium restart dispatcher)"});
        }
        let mut c = std::process::Command::new(std::env::current_exe().unwrap_or_default());
        c.args(["restart", "dispatcher"]);
        if let Some(h) = &self.home {
            c.args(["--home", h]);
        }
        c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW,
            // as the update's: it must outlive the dispatcher it restarts.
            c.creation_flags(0x0000_0008 | 0x0000_0200 | crate::proc::CREATE_NO_WINDOW);
        }
        match c.spawn() {
            Ok(_) => json!({"ok": true}),
            Err(e) => json!({"ok": false, "error": format!("cannot restart: {e}")}),
        }
    }
}
