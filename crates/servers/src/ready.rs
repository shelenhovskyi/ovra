//! When a server counts as ready: its checks, as a small state machine
//! fed by what the pod reports.

use ovra_core::protocol::servers::ServerReady;

/// What to do after a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Next {
    Wait,
    /// All checks passed, for the first time.
    Ready,
    /// Ask this path over HTTP through the port's socket, until it answers.
    AskHttp(String),
}

/// Without `ready` the port accepting a connection is enough; otherwise
/// each check that is set must pass.
#[derive(Debug, Clone)]
pub(crate) struct Readiness {
    log: Option<String>,
    http: Option<String>,
    log_seen: bool,
    listening: bool,
    answered: bool,
    reported: bool,
}

impl Readiness {
    pub(crate) fn new(ready: Option<&ServerReady>) -> Self {
        let ready = ready.cloned().unwrap_or_default();
        Self {
            log: ready.log,
            http: ready.http,
            log_seen: false,
            listening: false,
            answered: false,
            reported: false,
        }
    }

    pub(crate) fn line(&mut self, line: &str) -> Next {
        if self.log.as_deref().is_some_and(|log| line.contains(log)) {
            self.log_seen = true;
        }
        self.next()
    }

    pub(crate) fn listening(&mut self) -> Next {
        if self.listening {
            return Next::Wait;
        }
        self.listening = true;
        match &self.http {
            Some(path) => Next::AskHttp(path.clone()),
            None => self.next(),
        }
    }

    pub(crate) fn answered(&mut self) -> Next {
        self.answered = true;
        self.next()
    }

    fn next(&mut self) -> Next {
        let by_port = self.log.is_none() && self.http.is_none();
        let holds = (!by_port || self.listening)
            && (self.log.is_none() || self.log_seen)
            && (self.http.is_none() || self.answered);
        if holds && !self.reported {
            self.reported = true;
            Next::Ready
        } else {
            Next::Wait
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(log: Option<&str>, http: Option<&str>) -> Readiness {
        Readiness::new(Some(&ServerReady {
            log: log.map(Into::into),
            http: http.map(Into::into),
        }))
    }

    #[test]
    fn without_checks_the_port_is_enough_once() {
        let mut r = Readiness::new(None);
        assert_eq!(r.line("anything"), Next::Wait);
        assert_eq!(r.listening(), Next::Ready);
        assert_eq!(r.listening(), Next::Wait);
        assert_eq!(r.line("more"), Next::Wait);
    }

    #[test]
    fn a_log_check_needs_its_substring_not_the_port() {
        let mut r = ready(Some("ready in"), None);
        assert_eq!(r.listening(), Next::Wait);
        assert_eq!(r.line("starting"), Next::Wait);
        assert_eq!(r.line("  VITE ready in 300 ms"), Next::Ready);
        assert_eq!(r.line("ready in 1 ms"), Next::Wait);
    }

    #[test]
    fn an_http_check_is_asked_once_the_port_listens() {
        let mut r = ready(None, Some("/health"));
        assert_eq!(r.listening(), Next::AskHttp("/health".into()));
        assert_eq!(r.answered(), Next::Ready);
    }

    #[test]
    fn with_both_checks_both_must_hold() {
        let mut r = ready(Some("up"), Some("/"));
        assert_eq!(r.listening(), Next::AskHttp("/".into()));
        assert_eq!(r.answered(), Next::Wait);
        assert_eq!(r.line("up"), Next::Ready);
    }
}
