//! An optional one-line summary above the excerpt — never instead of it.
//!
//! # What this is for
//!
//! `docs/ASKING-FROM-A-PHONE.md` asks agents to write questions that survive the trip to a phone.
//! Agents will not always comply, and a harness nobody wrote rules for will not comply at all. A
//! small model reading the tail and saying *what is being asked* is the fix that works regardless.
//!
//! # The constraint that makes it safe
//!
//! **It may only ever ADD.** The summary is one line above the raw excerpt, which is always sent
//! unchanged underneath. If the model paraphrases the question wrongly, the operator can see that —
//! the real text is right there. A gate that *replaced* the excerpt would let a bad paraphrase send
//! them to answer a question nobody asked, and they would never know.
//!
//! Three more rules follow from the same thinking:
//!
//! - **Fail open, always.** Gateway down, slow, or answering strangely → the push goes out with no
//!   summary. A push that never arrives is far worse than an untidy one, and this is a convenience.
//! - **Short timeout.** The push is already a debounce window behind the ask; it must not also wait
//!   on inference. Measured locally: `local-coder` answers in ~0.7s, `glm-5.3-flash` in ~3s.
//! - **Never on the reply path.** D3 is that nothing interprets the operator's words before they
//!   reach a terminal. This runs agent→operator only, and there is no function here that touches
//!   anything travelling the other way.
//!
//! # Why the output is validated rather than trusted
//!
//! A model asked for one line will sometimes return a paragraph, a refusal, a markdown block, or a
//! restatement of the prompt. Any of those is worse than nothing at the top of a notification, so
//! [`plausible`] throws them away and the push goes out bare.
//!
//! # Where it is allowed to ask
//!
//! This is the one place in the bridge that sends the operator's terminal anywhere other than their
//! own chat, so where it sends has to be proved here rather than assumed. It once was assumed: the
//! gate sent a task class, another program on this machine was trusted to route that class to a
//! model on this machine, and when that routing quietly fell through to a hosted provider nothing
//! here could tell. Real excerpts left the machine and the journal still said they had not.
//!
//! Three things now have to hold before any of the operator's text goes out, and each of them is
//! checked in this file with no reference to any other program's configuration:
//!
//! - **The address is on this machine.** Loopback, parsed as a URL rather than matched as a string,
//!   because the interesting mistakes are the ones that look local.
//! - **Somebody local has already answered.** The first request of every run is a throwaway line
//!   out of the shipped prompt, sent to find out who is there. The excerpt only follows once the
//!   reply says who answered and it is a responder this bridge recognises.
//! - **They keep being the one answering.** Every later reply is checked the same way, because a
//!   routing chain can fall over halfway through a run. The first unrecognised answer switches
//!   summaries off for the rest of the run and they do not come back on by themselves.
//!
//! A proved address is still only a string until something writes to a socket, and a HTTP client
//! will happily pick the socket for you: a proxy variable in this service's environment, a redirect
//! the gateway answers with, a name the resolver moves. Each of those once carried the excerpt off
//! this machine with every gate above still passing. So the client is built with all three closed,
//! and then the address it actually reached is read back off the connection and checked — on the
//! probe, before any of the operator's text follows.
//!
//! Two variables widen this, and they are the only two. `HERDR_TG_SUMMARIZER_LOCAL_MODELS` replaces
//! the list of responder names that count as local — for the operator whose own gateway answers to
//! a name this crate has never heard of. `HERDR_TG_SUMMARIZER_ALLOW_REMOTE=1` is the operator
//! saying, in one exact word, that their pane text may leave this machine; it lifts the address
//! check and the responder check together, because both are asking them the same question.
//!
//! Refusing costs a summary. Guessing costs the operator's screen. So every one of these fails
//! closed, and a push always goes out either way.

use std::collections::BTreeSet;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The gateway on this machine, which is where this points unless the operator moves it.
const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8090/v1/chat/completions";

/// Where the operator says, in one exact word, that their terminal may leave this machine.
const ALLOW_REMOTE_ENV: &str = "HERDR_TG_SUMMARIZER_ALLOW_REMOTE";

/// Where the operator names the responders their own gateway answers to.
const LOCAL_MODELS_ENV: &str = "HERDR_TG_SUMMARIZER_LOCAL_MODELS";

/// The responder names this bridge treats as local without being told to.
///
/// These are seeds measured on the machine this was built on, not a claim about the world. The
/// check is the mechanism and this list is only data: when a gateway answers to some other name,
/// the warning in the journal prints that name exactly as it came back, and the operator puts it in
/// `HERDR_TG_SUMMARIZER_LOCAL_MODELS`. `bash scripts/eval-gist.sh` prints the same name as
/// "served by …", which is the way to find it out without waiting for the warning.
///
/// `qwen2.5-coder-1.5b-q4` is measured rather than guessed: the model server behind the gateway on
/// this machine reports exactly that name for itself. Whether the gateway passes that name through
/// or answers with its own alias for it could not be checked from here — it needs the operator's
/// gateway key — so both spellings are seeded and the operator confirms with `eval-gist.sh`.
const DEFAULT_LOCAL_MODELS: &[&str] = &[
    "local-coder",
    "local-qwen3",
    "qwen2.5-coder-1.5b",
    "qwen2.5-coder-1.5b-instruct",
    "qwen2.5-coder-1.5b-q4",
];

/// The line sent to find out who answers, before any of the operator's text goes anywhere.
///
/// It is copied out of `prompts/gist.txt`, so it is already public in this repository and it says
/// nothing about the operator or their machine. Spending one of these is what makes it impossible
/// for the excerpt to be the thing that discovers a wrong destination.
const PROBE: &str = "Overwrite config.yaml? [y/N]";

/// Is this address on this machine?
///
/// Provable from inside this crate, which is the whole point — the old answer was inferred from a
/// different program's configuration file that this crate never reads.
///
/// Parsing is left to a real URL parser because the inputs that matter are the ones that look
/// local. `http://127.0.0.1@evil.example/` is a request to evil.example with a username of
/// 127.0.0.1, and reading it by eye or by prefix gets it wrong.
///
/// `localhost` is accepted even though a name is not an address and resolution can change under a
/// running process. It is accepted because it is not left to resolution: [`loopback_pin`] fixes it
/// to 127.0.0.1 and ::1 in the client, so a `/etc/hosts` line cannot move it, and the address the
/// connection actually reached is checked afterwards regardless. Refusing the name would cost
/// every operator who writes `localhost` their summaries and buy nothing the pin does not give.
fn is_local_endpoint(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    // `localhost.` is the same name written as a fully qualified one.
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(ip) => on_this_machine(ip),
        Err(_) => false,
    }
}

/// Is this address one of this machine's own?
///
/// One definition, used both for the endpoint the operator configured and for the address the
/// connection actually reached, so those two can never drift apart.
fn on_this_machine(ip: IpAddr) -> bool {
    match ip {
        // `::ffff:127.0.0.1` reaches 127.0.0.1, but as an IPv6 address it is not itself loopback.
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.is_loopback(),
            None => v6.is_loopback(),
        },
        IpAddr::V4(v4) => v4.is_loopback(),
    }
}

/// The host name whose resolution must be nailed to loopback, and the port it was configured with.
///
/// `None` when the endpoint is already a literal address — there is nothing for a resolver to
/// decide — or when it does not parse at all, in which case no request will be made anyway.
///
/// The port is carried because the pin replaces resolution, and a name resolved to the wrong port
/// would simply stop the gateway working.
fn loopback_pin(endpoint: &str) -> Option<(String, u16)> {
    let url = reqwest::Url::parse(endpoint).ok()?;
    let host = url.host_str()?;
    if host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .is_ok()
    {
        return None;
    }
    Some((host.to_string(), url.port_or_known_default()?))
}

/// The responder allowlist as a person reads it.
///
/// `{:?}` on a set renders braces and quotes, which is implementation vocabulary in a sentence
/// written for whoever has to fix the configuration.
fn as_prose(names: &BTreeSet<String>) -> String {
    if names.is_empty() {
        return "none at all, because the list is empty".to_string();
    }
    names.iter().cloned().collect::<Vec<_>>().join(", ")
}

/// Where to ask, what to ask, and what has been proved about the answer so far.
#[derive(Clone)]
pub struct Summarizer {
    pub endpoint: String,
    /// What kind of work this is, sent as `X-Task-Class`.
    ///
    /// `autocomplete` by default, which the gateway routes to the small local model. That became
    /// the right answer only once the prompt was distilled for it: on the same eight blocked panes
    /// the local model scores 8/8 at 390ms against the hosted model's 8/8 at 2082ms — same
    /// accuracy, five times faster, and no API tokens at all.
    ///
    /// Before distillation the same model scored 1/8 with a hand-written prompt, which is why this
    /// defaulted to the hosted chain until now.
    pub task_class: Option<String>,
    pub model: Option<String>,
    pub key: String,
    pub timeout: Duration,
    /// The responder names whose answers may be shown: whatever the operator listed, or the seeds.
    pub local_models: BTreeSet<String>,
    /// The operator has said, in one exact word, that their pane text may leave this machine.
    pub allow_remote: bool,
    /// Set once somebody this bridge recognises has answered in this run. Until then the only thing
    /// sent to this address is the probe.
    armed: Arc<AtomicBool>,
    /// Set the first time something unrecognised answers, and never cleared. A routing chain that
    /// fell over once will fall over again, and the operator has not been asked about it yet.
    off: Arc<AtomicBool>,
}

impl fmt::Debug for Summarizer {
    /// Hand-written so the gateway credential cannot reach a log through `{:?}`. The bot's whole
    /// context holds this struct, so one debug line added later would otherwise print a live key.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Summarizer")
            .field("endpoint", &self.endpoint)
            .field("task_class", &self.task_class)
            .field("model", &self.model)
            .field("key", &"<redacted>")
            .field("timeout", &self.timeout)
            .field("local_models", &self.local_models)
            .field("allow_remote", &self.allow_remote)
            .field("off", &self.off.load(Ordering::Relaxed))
            .finish()
    }
}

/// What the environment asked for.
///
/// Three outcomes rather than two, because "nobody switched this on" and "somebody switched it on
/// and it was refused" must not look the same. The first is the default and is fine; the second is
/// a mistake that silently costs the operator the thing they think they configured, so it is worth
/// a sentence in the journal that names the variable at fault.
enum Setup {
    /// No key, so the gate is off. The default, and not a problem.
    Off,
    On(Summarizer),
    /// Switched on, and refused. The sentence names the variable and how to satisfy it.
    Refused(String),
}

/// What came back from one request, and — the part the operator's privacy rests on — where from.
struct Answer {
    /// The reply's top-level `model`: who says they answered.
    responder: String,
    content: String,
    /// The address this crate's connection actually reached, read back off the connection rather
    /// than taken from the configuration.
    ///
    /// `None` when it could not be read, which counts the same as an address off this machine: a
    /// destination that cannot be shown to be here is not one the operator's screen may go to.
    peer: Option<SocketAddr>,
}

/// Did the bytes go to a socket on this machine?
///
/// The startup gate proves something about a string. This is the only thing that can say what
/// happened on the wire, and it is deliberately separate from who answered: an address off this
/// machine is a leak whatever name the reply puts on itself.
fn answer_came_from_this_machine(peer: Option<SocketAddr>) -> bool {
    matches!(peer, Some(addr) if on_this_machine(addr.ip()))
}

/// The instruction, written BY a strong model FOR a small one, and measured at every step.
///
/// # Why it is a file and not a string literal
///
/// It was produced by distillation rather than by hand: `glm-5.3` was given the task, the small
/// model's exact failures, and a note that a 1.5B model completes patterns rather than reasons.
/// Keeping it as data makes the next round a diff of a prompt rather than a diff of Rust.
///
/// # What the rounds cost and bought
///
/// Scored against the distribution that actually occurs — panes herdr has already flagged as
/// blocked, since the gist never runs on any other kind:
///
/// | prompt | model | score | median |
/// |---|---|---|---|
/// | hand-written | glm-5.3-flash (hosted) | 8/8 | 2082ms |
/// | distilled, round 1 | qwen2.5-coder-1.5b (local) | 6/8 | 381ms |
/// | distilled, round 2 | qwen2.5-coder-1.5b (local) | **8/8** | **390ms** |
///
/// Two things worth keeping from getting there. Round 2 scored WORSE (3/11 vs 6/11) on a mixed set
/// that included panes which were not asking anything — it had been tuned to find questions, so it
/// found them everywhere. That set was the wrong test: the notifier only calls this on a blocked
/// pane. Measuring the wrong distribution nearly threw away the better prompt.
///
/// And distillation is not monotonic. Round 2 fixed the permission dialog and broke the negatives;
/// only re-scoring on the real distribution showed it was the right trade. Each round needs its own
/// measurement, not an assumption that more iterations are better.
const SYSTEM: &str = include_str!("../prompts/gist.txt");

/// The longest summary worth showing. Beyond this it stops being a glance and becomes a second
/// thing to read.
const MAX_CHARS: usize = 110;

impl Summarizer {
    /// Read the configuration from the environment, or `None` if there will be no summaries.
    ///
    /// Off unless deliberately switched on. The key is its own variable rather than being read out
    /// of some other tool's config file: a bridge that goes looking through the operator's other
    /// credentials to find one it can use is not behaving well, however convenient.
    ///
    /// `None` also covers the case where it WAS switched on and was refused — a destination that is
    /// not on this machine, or a pinned model this bridge cannot show is local. That case says so
    /// loudly in the journal, naming the variable at fault, because the operator will otherwise be
    /// left believing a feature is running that is not.
    pub fn from_env() -> Option<Self> {
        match Self::configure() {
            Setup::On(summarizer) => Some(summarizer),
            Setup::Off => None,
            Setup::Refused(why) => {
                tracing::warn!(
                    %why,
                    "summaries are switched on but were refused; asks go out with no summary"
                );
                None
            }
        }
    }

    /// The three startup gates, kept apart from [`Summarizer::from_env`] so the refusal sentence is
    /// a value that can be read back and tested rather than only a line in a log.
    fn configure() -> Setup {
        let Ok(key) = std::env::var("HERDR_TG_SUMMARIZER_KEY") else {
            return Setup::Off;
        };
        if key.trim().is_empty() {
            return Setup::Off;
        }

        // One exact spelling. `true`, `yes` and `on` are deliberately NOT this: a near miss has to
        // fail closed, and one literal is greppable across a whole machine's environment.
        let allow_remote = std::env::var(ALLOW_REMOTE_ENV)
            .map(|v| v.trim() == "1")
            .unwrap_or(false);

        let endpoint =
            std::env::var("HERDR_TG_SUMMARIZER_URL").unwrap_or_else(|_| DEFAULT_ENDPOINT.into());
        if !allow_remote && !is_local_endpoint(&endpoint) {
            return Setup::Refused(format!(
                "HERDR_TG_SUMMARIZER_URL={endpoint} is not on this machine, and what gets sent \
                 there is a piece of the operator's screen. No summaries will be added. Point it \
                 at 127.0.0.1 or localhost, or set {ALLOW_REMOTE_ENV}=1 if sending screen text off \
                 this machine is what you meant."
            ));
        }

        let local_models: BTreeSet<String> = match std::env::var(LOCAL_MODELS_ENV) {
            Ok(raw) if !raw.trim().is_empty() => raw
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect(),
            // Replaces the seeds rather than adding to them, so there is exactly one place to read
            // the list that is actually in force — and so the operator can narrow it as well.
            _ => DEFAULT_LOCAL_MODELS.iter().map(|s| s.to_string()).collect(),
        };
        if local_models.iter().any(|name| name.contains('*')) {
            return Setup::Refused(format!(
                "{LOCAL_MODELS_ENV} is a list of exact names, not a pattern. A `*` matches nothing \
                 here and would read as permission it does not grant. Set {ALLOW_REMOTE_ENV}=1 if \
                 you mean to accept whoever answers."
            ));
        }

        let model = std::env::var("HERDR_TG_SUMMARIZER_MODEL")
            .ok()
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty());
        if let Some(pinned) = &model {
            let listed = local_models.iter().any(|l| l.eq_ignore_ascii_case(pinned));
            if !allow_remote && !listed {
                return Setup::Refused(format!(
                    "HERDR_TG_SUMMARIZER_MODEL={pinned} names something that is not in \
                     {LOCAL_MODELS_ENV}, which lists {}. Naming one skips the gateway's own \
                     routing entirely, which is exactly how pieces of the operator's screen \
                     reached a hosted provider before. Unset it, add {pinned} to \
                     {LOCAL_MODELS_ENV} if it runs on this machine, or set {ALLOW_REMOTE_ENV}=1.",
                    as_prose(&local_models)
                ));
            }
        }

        let timeout = std::env::var("HERDR_TG_SUMMARIZER_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_millis(4000));

        Setup::On(Self {
            endpoint,
            model,
            key: key.trim().to_string(),
            timeout,
            task_class: std::env::var("HERDR_TG_SUMMARIZER_CLASS")
                .ok()
                .filter(|c| !c.trim().is_empty())
                .or_else(|| Some("autocomplete".into())),
            local_models,
            allow_remote,
            // Under an explicit grant there is nothing left to prove, so no probe is spent.
            armed: Arc::new(AtomicBool::new(allow_remote)),
            off: Arc::new(AtomicBool::new(false)),
        })
    }

    /// The HTTP client, built so that the address this crate proved is the address the bytes go to.
    ///
    /// A client library decides the destination for you in three separate ways, and each of them
    /// has carried a real excerpt past a passing address gate:
    ///
    /// - **A proxy.** reqwest reads `HTTP_PROXY` / `http_proxy` / `ALL_PROXY` out of the process
    ///   environment by default, so one line in a systemd unit sends the excerpt AND the gateway
    ///   key to whatever host it names, while the configured endpoint stays a loopback address that
    ///   passes every check. [`no_proxy`](reqwest::ClientBuilder::no_proxy) is what refuses that.
    /// - **A redirect.** The default policy follows up to ten hops and re-sends the POST body on
    ///   307 and 308, which is exactly the shape of a routing chain falling through to a hosted
    ///   provider. A gist gateway has no reason to redirect, so this follows none.
    /// - **The resolver.** `localhost` is a name, and a `/etc/hosts` line can move it. Pinning it
    ///   here means the socket matches the address that was proved.
    ///
    /// Only the pin is lifted by `HERDR_TG_SUMMARIZER_ALLOW_REMOTE=1`, and only because an
    /// operator who has said their pane text may leave this machine has to be able to name a host
    /// that is not on it. The other two hold either way: an environment variable and a redirect are
    /// nobody's idea of consent, and an operator who means their text to go somewhere can say where
    /// in the URL. The cost is real and worth naming — a machine that can only reach the outside
    /// through a proxy cannot use a remote gateway at all — and it is the right way round, because
    /// the failure it prevents is silent and this one is not.
    fn client(&self) -> Option<reqwest::Client> {
        let mut builder = reqwest::Client::builder()
            .timeout(self.timeout)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none());
        if !self.allow_remote {
            if let Some((host, port)) = loopback_pin(&self.endpoint) {
                builder = builder.resolve_to_addrs(
                    &host,
                    &[
                        SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
                        SocketAddr::from((Ipv6Addr::LOCALHOST, port)),
                    ],
                );
            }
        }
        builder.build().ok()
    }

    /// One request. Gives back who answered, what they said, and where the connection went — or
    /// nothing at all.
    ///
    /// The responder is the reply's top-level `model`, the one field in an OpenAI-shaped reply that
    /// says who actually answered as opposed to who was asked for. It travels with the content, and
    /// with the address that was reached, rather than being fished out at the call site: a caller
    /// that can reach the content without those two is a caller that will forget to check them.
    async fn ask(&self, excerpt: &str, max_tokens: u32) -> Option<Answer> {
        let mut body = serde_json::json!({
            "max_tokens": max_tokens,
            "temperature": 0,
            "messages": [
                {"role": "system", "content": SYSTEM},
                {"role": "user", "content": excerpt},
            ],
        });

        if let Some(m) = &self.model {
            body["model"] = serde_json::Value::String(m.clone());
        }

        let client = self.client()?;
        let mut req = client.post(&self.endpoint).bearer_auth(&self.key);
        if let Some(c) = &self.task_class {
            req = req.header("X-Task-Class", c);
        }
        let res = req.json(&body).send().await;

        // Read off the connection before the body is consumed. This is the crate's own account of
        // where it wrote to, and the only claim here that no configuration file can contradict.
        let peer = res.as_ref().ok().and_then(|r| r.remote_addr());

        let text = match res {
            Ok(r) if r.status().is_success() => r.text().await.ok()?,
            Ok(r) => {
                // A redirect arrives here rather than being followed, and is declined like any
                // other status this bridge did not ask for.
                tracing::debug!(status = %r.status(), "the summarizer declined; pushing without it");
                return None;
            }
            Err(e) => {
                tracing::debug!(error = %e, "the summarizer is unreachable; pushing without it");
                return None;
            }
        };

        let parsed: serde_json::Value = serde_json::from_str(&text).ok()?;
        let responder = parsed
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        // Some models put their thinking in `reasoning_content` and leave `content` empty when the
        // token budget runs out. That is not an error, just nothing to show.
        let content = parsed
            .pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        Some(Answer {
            responder,
            content,
            peer,
        })
    }

    /// May this answer be shown — and, before that, was the operator's screen shown to somebody
    /// this bridge can prove is on this machine?
    ///
    /// Both halves, because either alone has been defeated: a recognised name is worthless if the
    /// bytes went to a proxy, and a loopback socket is worthless if what is behind it is a tunnel
    /// answering for a hosted provider.
    fn answer_is_local(&self, answer: &Answer) -> bool {
        if self.allow_remote {
            return true;
        }
        answer_came_from_this_machine(answer.peer) && self.responder_ok(&answer.responder)
    }

    /// Is this somebody whose answer may be shown — and, before that, somebody the operator's
    /// screen may be shown to?
    fn responder_ok(&self, responder: &str) -> bool {
        if self.allow_remote {
            return true;
        }
        let responder = responder.trim();
        // A reply that does not say who answered is a reply from somebody unknown. An OpenAI-shaped
        // reply always carries that field; one that does not cannot be shown to be local, and
        // cannot-be-shown means no.
        !responder.is_empty()
            && self
                .local_models
                .iter()
                .any(|known| known.eq_ignore_ascii_case(responder))
    }

    /// One line describing what the agent needs, or `None`.
    ///
    /// # The rule the excerpt depends on
    ///
    /// A piece of the operator's screen only ever goes to an address that has already answered, in
    /// this run, as somebody this bridge recognises. The first call spends one throwaway request on
    /// [`PROBE`] — a line out of the prompt shipped in this repository — to find out who is there.
    /// So a gateway whose routing falls through to a hosted provider is discovered with a public
    /// string instead of with the operator's terminal.
    ///
    /// The probe proves two things, not one: who is answering, and — from the connection itself —
    /// that the connection went to this machine.
    ///
    /// Every later reply is checked the same way, because routing can fall over halfway through a
    /// run. The first answer that fails either check switches summaries off for the rest of the
    /// run, so the most that can ever go astray is one probe — and an excerpt only if the routing
    /// changes after the probe has already been answered, which the journal then says plainly.
    ///
    /// Every failure path still returns `None` and the caller sends the excerpt alone. Nothing here
    /// can prevent a push.
    pub async fn one_line(&self, excerpt: &str) -> Option<String> {
        if self.off.load(Ordering::SeqCst) {
            return None;
        }
        if excerpt.trim().is_empty() {
            return None;
        }

        if !self.armed.load(Ordering::SeqCst) {
            // Gateway down or answering nonsense: no summary this time, and still nothing proved,
            // so the next push probes again. Going without a summary costs the operator nothing.
            let answer = self.ask(PROBE, 16).await?;
            if !self.answer_is_local(&answer) {
                self.trip(&answer, false);
                return None;
            }
            self.armed.store(true, Ordering::SeqCst);
            tracing::info!(
                responder = %answer.responder,
                "the summarizer is answered from this machine; summaries are on"
            );
        }

        let answer = self.ask(excerpt, 500).await?;
        if !self.answer_is_local(&answer) {
            // The excerpt is already on the wire by the time this reply can be inspected, and the
            // journal has to say so rather than repeat the reassuring sentence from the probe.
            self.trip(&answer, true);
            return None;
        }

        plausible(&answer.content, excerpt)
    }

    /// Why summaries stopped, as the sentence the journal gets.
    ///
    /// A value rather than only a log line, for the same reason [`Setup::Refused`] is one: this
    /// sentence has to be readable back and testable. It is the one place the bridge tells the
    /// operator what left the machine, and it is reached from two call sites — after the probe,
    /// where nothing of theirs has gone, and after the excerpt, where it has. Saying the
    /// comforting version in both is a false statement about a leak, so the caller passes in which
    /// it is and this never guesses.
    fn why_summaries_stopped(&self, answer: &Answer, screen_text_was_sent: bool) -> String {
        let exposure = if screen_text_was_sent {
            "a piece of the operator's screen had already been sent when this came back"
        } else {
            "nothing from the screen was sent"
        };

        if !answer_came_from_this_machine(answer.peer) {
            let reached = match answer.peer {
                Some(addr) => format!("at {addr}, which is not on this machine"),
                None => "at an address this bridge could not read back".to_string(),
            };
            return format!(
                "the summarizer answered {reached}, so summaries are OFF for the rest of this run \
                 and {exposure}. The address it was told to use was {}. Look for a proxy setting \
                 in this service's environment, or a gateway that answers with a redirect.",
                self.endpoint
            );
        }

        let who = if answer.responder.is_empty() {
            "the reply did not say who answered".to_string()
        } else {
            format!("{} answered", answer.responder)
        };
        format!(
            "{who}, which is not a name this bridge recognises, so summaries are OFF for the rest \
             of this run and {exposure}. The names it recognises are {}. If that one does run on \
             this machine, add it to {LOCAL_MODELS_ENV} and restart.",
            as_prose(&self.local_models)
        )
    }

    /// Switch summaries off for the rest of the run, and say so once in the journal — where naming
    /// who answered and what address they answered at is diagnosis, not disclosure.
    ///
    /// `screen_text_was_sent` is the caller's answer to the only question the operator will have.
    ///
    /// The journal is currently the only place this is said. Telling the operator in chat as well,
    /// so they do not sit wondering where their summaries went, is a deliberate follow-up: it needs
    /// a sentence in the module that owns the bridge's words, and that module is being changed
    /// elsewhere right now. Nothing about the safety of this waits on it.
    fn trip(&self, answer: &Answer, screen_text_was_sent: bool) {
        if !self.off.swap(true, Ordering::SeqCst) {
            tracing::warn!(
                "{}",
                self.why_summaries_stopped(answer, screen_text_was_sent)
            );
        }
    }
}

/// Is this actually a one-line summary, or is it something else the model decided to say?
///
/// Rejects rather than repairs. A summary that needs repairing is one the operator would have to
/// second-guess, and the excerpt below it is already the truth.
pub fn plausible(raw: &str, excerpt: &str) -> Option<String> {
    let line = raw.trim().trim_matches('"').trim();
    if line.is_empty() {
        return None;
    }
    // More than one line means it ignored the instruction, and the rest may be commentary.
    if line.lines().count() > 1 {
        return None;
    }
    if line.chars().count() > MAX_CHARS {
        return None;
    }
    // Markdown, code fences and HTML would either render wrongly or fight the message's own markup.
    if line.contains("```") || line.contains('<') || line.contains('>') {
        return None;
    }
    let low = line.to_lowercase();
    // The agreed signal for "this pane is not asking anything". Not an error — most panes are not.
    if low == "none" || low == "none." {
        return None;
    }
    // A model echoing the instruction instead of following it, which `local-coder` did verbatim.
    if low.starts_with("is the agent") || low.contains("waiting for the human to answer") {
        return None;
    }
    // Filler a model reaches for when it has nothing: it looks like a summary and says nothing.
    for filler in [
        "please provide",
        "the necessary information",
        "no question",
        "not asking",
    ] {
        if low.contains(filler) {
            return None;
        }
    }
    // A model talking about itself rather than the excerpt.
    for tell in [
        "as an ai",
        "i cannot",
        "i can't",
        "i'm sorry",
        "sorry,",
        "here is",
        "here's a",
        "summary:",
        "the agent is asking",
        "this excerpt",
    ] {
        if low.starts_with(tell) || low.contains("as an ai") {
            return None;
        }
    }
    // THE rule that matters most, and the reason it is here rather than only in the prompt: a
    // gist that ANSWERS instead of restating is worse than no gist at all. It reads as a
    // recommendation, and the operator may act on it without noticing the real options below.
    //
    // Measured: given a genuine three-way question, one model replied "Work E4 clean-install." —
    // grammatical, confident, and one of the three choices presented as if it were the summary.
    //
    // The prompt forbids this, but a prompt is the part most likely to drift or be swapped for a
    // different model. So it is also checked structurally: if the source asks, the gist must ask.
    if asks_something(excerpt) && !line.contains('?') {
        tracing::debug!(gist = %line, "the summary answered instead of restating; dropped");
        return None;
    }

    Some(line.to_string())
}

/// Does this excerpt put a question to the human?
///
/// Deliberately broad. A false positive costs a dropped gist; a false negative lets an
/// answer-shaped gist through, which is the failure this exists to prevent.
fn asks_something(excerpt: &str) -> bool {
    if excerpt.contains('?') {
        return true;
    }
    let low = excerpt.to_lowercase();
    [
        "[y/n]", "(y/n)", "y/n", "yes/no", "select", "confirm", "choose",
    ]
    .iter()
    .any(|t| low.contains(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises the tests that mutate the process environment.
    ///
    /// `set_var`/`remove_var` are process-wide, and the test harness runs tests on many threads.
    /// One test sets `HERDR_TG_SUMMARIZER_KEY` while another removes it, so whichever lost the race
    /// read the other's state — measured as 2 failures in 6 clean workspace runs, which is a coin
    /// flip, not a rare flake. Every test that touches the environment takes this guard first.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        // A panic in one env test must not poison the rest into failing for the wrong reason.
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Start from nothing set. A test that only sets what it cares about would otherwise
        // inherit whatever the previous one left behind, and read as passing for the wrong reason.
        for var in [
            "HERDR_TG_SUMMARIZER_KEY",
            "HERDR_TG_SUMMARIZER_URL",
            "HERDR_TG_SUMMARIZER_MODEL",
            "HERDR_TG_SUMMARIZER_CLASS",
            "HERDR_TG_SUMMARIZER_TIMEOUT_MS",
            ALLOW_REMOTE_ENV,
            LOCAL_MODELS_ENV,
        ] {
            // SAFETY: every reader of these variables is `from_env`, and ENV_LOCK serialises the
            // tests that touch them.
            unsafe { std::env::remove_var(var) };
        }
        guard
    }

    /// The shape the local model actually returned when this was measured.
    #[test]
    fn a_real_summary_survives() {
        assert_eq!(
            plausible(
                "Force-push and drop the 2 commits? [y/N]",
                "Force-push? [y/N]"
            )
            .as_deref(),
            Some("Force-push and drop the 2 commits? [y/N]")
        );
        assert_eq!(
            plausible(
                "  \"Allow access to ~/.local/share?\"  ",
                "Allow access? [y/N]"
            )
            .as_deref(),
            Some("Allow access to ~/.local/share?")
        );
    }

    /// Everything below is a real way a small model answers badly. Each must produce a bare push
    /// rather than a confusing first line.
    #[test]
    fn a_model_that_ignored_the_instruction_is_thrown_away() {
        for bad in [
            "",
            "   ",
            "Here is a summary of what the agent needs:",
            "Summary: the agent wants to force-push",
            "I'm sorry, I cannot summarise that.",
            "As an AI language model, I would say the agent is stuck.",
            "The agent is asking whether to force-push",
            "line one\nline two",
            // Every one of these came out of a real model against a real pane.
            "NONE",
            "none.",
            "Is the agent waiting for the human to answer something?",
            "Please provide the necessary information for the agent to complete the task.",
            "```\nforce-push?\n```",
            "<b>Force-push?</b>",
        ] {
            assert!(
                plausible(bad, "Force-push? [y/N]").is_none(),
                "this should have been rejected: {bad:?}"
            );
        }
    }

    /// The failure the operator singled out: a gist that answers rather than restates. It reads as
    /// a recommendation, and the real options are below where they may not be read.
    #[test]
    fn a_gist_that_answers_instead_of_restating_is_dropped() {
        let question = "What do you want to do — tidy the tracker, work E4 clean-install, \
                        or something else?";
        // The real bad answer, verbatim from a real model against a real pane.
        assert!(plausible("Work E4 clean-install.", question).is_none());
        assert!(plausible("Force-push.", "Force-push and drop 2 commits? [y/N]").is_none());
        assert!(plausible("Yes", "Allow access? [y/N]").is_none());

        // The good answer to the same excerpt survives, because it is still a question.
        assert_eq!(
            plausible("Tidy the tracker, work E4, or something else?", question).as_deref(),
            Some("Tidy the tracker, work E4, or something else?")
        );
    }

    /// A pane that is not asking has no question to preserve, so a declarative line is fine there.
    #[test]
    fn a_declarative_gist_is_fine_when_nothing_was_asked() {
        assert_eq!(
            plausible("Tests finished, 42 passed.", "Running tests... 42 passed").as_deref(),
            Some("Tests finished, 42 passed.")
        );
    }

    #[test]
    fn the_question_detector_is_broad_on_purpose() {
        for asking in [
            "Force-push? [y/N]",
            "Allow once / Allow always / Reject   select  confirm",
            "Choose one:",
            "yes/no",
        ] {
            assert!(asks_something(asking), "missed a question: {asking:?}");
        }
        assert!(!asks_something("Running tests... 42 passed"));
    }

    #[test]
    fn an_over_long_answer_is_thrown_away() {
        let long = "x".repeat(MAX_CHARS + 1);
        assert!(plausible(&long, "x").is_none());
        let ok = "x".repeat(MAX_CHARS);
        assert!(plausible(&ok, "x").is_some());
    }

    /// The default routes to the small local model, because the distilled prompt made it match the
    /// hosted model's accuracy at a fifth of the latency.
    #[test]
    fn no_task_class_is_sent_unless_the_operator_asks_for_one() {
        let _env = env_guard();
        // SAFETY: `_env` holds ENV_LOCK, so no other test reads or writes these vars concurrently,
        // and outside the tests they are read only by from_env.
        unsafe {
            std::env::set_var("HERDR_TG_SUMMARIZER_KEY", "k");
            std::env::remove_var("HERDR_TG_SUMMARIZER_CLASS");
        }
        assert_eq!(
            Summarizer::from_env()
                .expect("configured")
                .task_class
                .as_deref(),
            Some("autocomplete"),
            "the distilled prompt makes the local model the right default"
        );

        unsafe { std::env::set_var("HERDR_TG_SUMMARIZER_CLASS", "bulk") };
        assert_eq!(
            Summarizer::from_env()
                .expect("configured")
                .task_class
                .as_deref(),
            Some("bulk")
        );
        unsafe {
            std::env::remove_var("HERDR_TG_SUMMARIZER_CLASS");
            std::env::remove_var("HERDR_TG_SUMMARIZER_KEY");
        }
    }

    /// The gate is off unless deliberately switched on, and it never goes looking through other
    /// tools' credentials for a key it could use.
    #[test]
    fn the_gate_is_off_without_its_own_key() {
        let _env = env_guard();
        // SAFETY: `_env` holds ENV_LOCK, so no other test reads or writes this var concurrently,
        // and outside the tests it is read only by from_env.
        unsafe { std::env::remove_var("HERDR_TG_SUMMARIZER_KEY") };
        assert!(Summarizer::from_env().is_none());
    }

    /// An empty excerpt has nothing to summarise, and must not cost a round trip.
    #[tokio::test]
    async fn an_empty_excerpt_never_calls_out() {
        // Deliberately unroutable: if this were called the test would hang rather than pass.
        let s = at("http://127.0.0.1:1/nope", &["local-coder"]);
        assert!(s.one_line("   \n  ").await.is_none());
    }

    /// Fail open: an unreachable gateway yields no summary and no error, so the push still goes.
    #[tokio::test]
    async fn an_unreachable_gateway_yields_no_summary_rather_than_failing() {
        let s = at("http://127.0.0.1:1/nope", &["local-coder"]);
        assert!(s.one_line("Force-push? [y/N]").await.is_none());
    }

    /// Everything a recorder was sent, exactly as it arrived: request line, headers and body.
    type Recorded = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

    /// A server on loopback that answers with exactly the raw responses given, in order, and keeps
    /// every request it was sent.
    ///
    /// Raw rather than JSON-wrapped because two of the things worth proving are not replies at all:
    /// a redirect that tries to move the excerpt elsewhere, and a proxy standing in the middle.
    ///
    /// A hand-rolled stand-in for reqwest would prove nothing here: the question is what this crate
    /// does with a real reply, and whether the operator's text was already on the wire by then.
    async fn fake_server(responses: Vec<String>) -> (std::net::SocketAddr, Recorded) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink = std::sync::Arc::clone(&seen);
        tokio::spawn(async move {
            for response in responses {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                // reqwest will not read the response until it has finished writing the request, so
                // the whole body has to be drained before anything is sent back.
                let mut got = Vec::new();
                let mut buf = vec![0u8; 8192];
                loop {
                    let n = match tokio::time::timeout(Duration::from_secs(2), sock.read(&mut buf))
                        .await
                    {
                        Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
                        Ok(Ok(n)) => n,
                    };
                    got.extend_from_slice(&buf[..n]);
                    if let Some(end) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&got[..end]).to_lowercase();
                        let want = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if got.len() >= end + 4 + want {
                            // The whole request, head included: a proxy rewrites the request line
                            // and carries the gateway key in a header, and both are worth reading.
                            sink.lock()
                                .expect("the recorder is only ever locked here")
                                .push(String::from_utf8_lossy(&got).to_string());
                            break;
                        }
                    }
                }
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        (addr, seen)
    }

    /// A gateway on loopback answering with exactly these JSON bodies, in order.
    async fn fake_gateway_at(replies: Vec<&str>) -> (std::net::SocketAddr, Recorded) {
        let responses = replies
            .iter()
            .map(|reply| {
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                    reply.len()
                )
            })
            .collect();
        fake_server(responses).await
    }

    /// The same gateway, given as the URL a summarizer would be pointed at.
    async fn fake_gateway(replies: Vec<&str>) -> (String, Recorded) {
        let (addr, seen) = fake_gateway_at(replies).await;
        (format!("http://{addr}/v1/chat/completions"), seen)
    }

    /// A summarizer aimed at a test gateway, with the shipped defaults everywhere else.
    fn at(endpoint: &str, local_models: &[&str]) -> Summarizer {
        Summarizer {
            endpoint: endpoint.into(),
            task_class: None,
            model: None,
            key: "k".into(),
            timeout: Duration::from_secs(2),
            local_models: local_models.iter().map(|s| s.to_string()).collect(),
            allow_remote: false,
            armed: Arc::new(AtomicBool::new(false)),
            off: Arc::new(AtomicBool::new(false)),
        }
    }

    /// THE property. An excerpt out of the operator's terminal must never be the thing that finds
    /// out where this endpoint actually sends: 46 real excerpts reached a hosted provider because
    /// the first request out was the excerpt itself.
    #[tokio::test]
    async fn no_pane_text_reaches_a_gateway_that_has_not_proved_who_answers() {
        let (url, seen) = fake_gateway(vec![
            r#"{"model":"glm-5.3-flash","choices":[{"message":{"content":"Force-push?"}}]}"#,
        ])
        .await;

        let got = at(&url, &["local-coder"])
            .one_line("SECRET-PANE-TEXT force-push? [y/N]")
            .await;

        assert!(
            got.is_none(),
            "an answer from somewhere this bridge does not know must not be shown: {got:?}"
        );
        let sent = seen
            .lock()
            .expect("the recorder is free once the call returns")
            .clone();
        assert_eq!(
            sent.len(),
            1,
            "one request only, and it is the throwaway probe"
        );
        assert!(
            sent[0].contains("Overwrite config.yaml"),
            "the first thing sent must be the public probe line, not the operator's screen"
        );
        assert!(
            !sent[0].contains("SECRET-PANE-TEXT"),
            "pane text went to a gateway that had not yet said who answers"
        );
    }

    /// Once something unrecognised answers, the bridge stops calling out for the rest of the run.
    /// That is what bounds the exposure to a single public probe instead of 46 excerpts.
    #[tokio::test]
    async fn once_something_foreign_answers_the_summarizer_stops_calling_out() {
        let foreign =
            r#"{"model":"glm-5.3-flash","choices":[{"message":{"content":"Force-push?"}}]}"#;
        let (url, seen) = fake_gateway(vec![foreign, foreign]).await;
        let s = at(&url, &["local-coder"]);

        assert!(s.one_line("Force-push? [y/N]").await.is_none());
        assert!(s.one_line("Another ask? [y/N]").await.is_none());

        assert_eq!(
            seen.lock()
                .expect("the recorder is free between calls")
                .len(),
            1,
            "the second ask must not have gone out at all"
        );
    }

    /// The probe is a precondition, not a replacement: once a responder this bridge calls local has
    /// answered, the excerpt follows and the operator still gets their summary.
    #[tokio::test]
    async fn a_local_responder_arms_the_gist_and_the_excerpt_follows() {
        let (url, seen) = fake_gateway(vec![
            r#"{"model":"local-coder","choices":[{"message":{"content":"Overwrite config.yaml?"}}]}"#,
            r#"{"model":"local-coder","choices":[{"message":{"content":"Force-push and drop the 2 commits?"}}]}"#,
        ])
        .await;

        let got = at(&url, &["local-coder"])
            .one_line("Force-push? [y/N]")
            .await;

        assert_eq!(got.as_deref(), Some("Force-push and drop the 2 commits?"));
        let sent = seen
            .lock()
            .expect("the recorder is free once the call returns")
            .clone();
        assert_eq!(sent.len(), 2, "one probe, then the excerpt");
        assert!(sent[0].contains("Overwrite config.yaml"));
        assert!(!sent[0].contains("Force-push? [y/N]"));
        assert!(sent[1].contains("Force-push? [y/N]"));
    }

    /// A reply that does not say who answered cannot be proved local, and unprovable means no.
    #[tokio::test]
    async fn a_reply_that_does_not_say_who_answered_is_treated_as_unknown() {
        let (url, seen) = fake_gateway(vec![
            r#"{"choices":[{"message":{"content":"Force-push?"}}]}"#,
        ])
        .await;

        let got = at(&url, &["local-coder"])
            .one_line("Force-push? [y/N]")
            .await;

        assert!(got.is_none());
        let sent = seen
            .lock()
            .expect("the recorder is free once the call returns")
            .clone();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].contains("Overwrite config.yaml"));
    }

    /// The one destination that is not on this machine, in the exact shape the journal caught.
    #[test]
    fn an_endpoint_off_this_machine_is_refused_and_the_message_says_how_to_mean_it() {
        let _env = env_guard();
        // SAFETY: `_env` holds ENV_LOCK, so no other test reads or writes these vars concurrently,
        // and outside the tests they are read only by from_env.
        unsafe {
            std::env::set_var("HERDR_TG_SUMMARIZER_KEY", "k");
            std::env::set_var(
                "HERDR_TG_SUMMARIZER_URL",
                "https://api.z.ai/api/coding/paas/v4/chat/completions",
            );
        }
        assert!(
            Summarizer::from_env().is_none(),
            "a pane excerpt is what gets sent there, so an endpoint off this machine is refused"
        );

        // Refusing silently would leave the operator believing summaries are running, so the
        // refusal has to name the variable at fault and the one word that would allow it.
        let Setup::Refused(why) = Summarizer::configure() else {
            panic!("this has to read as refused, not as nobody having switched it on");
        };
        assert!(
            why.contains("HERDR_TG_SUMMARIZER_URL"),
            "which variable? {why}"
        );
        assert!(why.contains(ALLOW_REMOTE_ENV), "and how to mean it? {why}");
    }

    /// Sending pane text off this machine is possible, but it takes one exact word. Anything that
    /// merely looks like agreement is a typo, and a typo must cost the feature, not the privacy.
    #[test]
    fn remote_is_possible_but_only_when_the_operator_types_the_exact_word() {
        let _env = env_guard();
        // SAFETY: `_env` holds ENV_LOCK, so no other test reads or writes these vars concurrently,
        // and outside the tests they are read only by from_env.
        unsafe {
            std::env::set_var("HERDR_TG_SUMMARIZER_KEY", "k");
            std::env::set_var(
                "HERDR_TG_SUMMARIZER_URL",
                "https://api.z.ai/v1/chat/completions",
            );
            std::env::set_var("HERDR_TG_SUMMARIZER_ALLOW_REMOTE", "1");
        }
        assert!(
            Summarizer::from_env().is_some(),
            "an operator who means it can say so"
        );

        for near_miss in ["true", "yes", "on", "0", " 1 x", ""] {
            unsafe { std::env::set_var("HERDR_TG_SUMMARIZER_ALLOW_REMOTE", near_miss) };
            assert!(
                Summarizer::from_env().is_none(),
                "this is not the word, so it must fail closed: {near_miss:?}"
            );
        }
    }

    /// Consent replaces proof: with the grant in place no probe is spent and whoever answers is
    /// accepted, because the operator has already said this may leave the machine.
    #[tokio::test]
    async fn an_explicit_remote_grant_spends_no_probe() {
        let (url, seen) = fake_gateway(vec![
            r#"{"model":"glm-5.3-flash","choices":[{"message":{"content":"Force-push?"}}]}"#,
        ])
        .await;
        let s = {
            let _env = env_guard();
            // SAFETY: `_env` holds ENV_LOCK for the whole of this block, so no other test reads or
            // writes these vars concurrently, and outside the tests they are read only by from_env.
            unsafe {
                std::env::set_var("HERDR_TG_SUMMARIZER_KEY", "k");
                std::env::set_var("HERDR_TG_SUMMARIZER_URL", &url);
                std::env::set_var("HERDR_TG_SUMMARIZER_ALLOW_REMOTE", "1");
            }
            Summarizer::from_env().expect("an explicit grant configures the gate")
        };

        assert_eq!(
            s.one_line("Force-push? [y/N]").await.as_deref(),
            Some("Force-push?")
        );
        assert_eq!(
            seen.lock()
                .expect("the recorder is free once the call returns")
                .len(),
            1,
            "there is nothing left to prove, so nothing is spent proving it"
        );
    }

    /// Naming a model bypasses the gateway's routing chain entirely, which is exactly how excerpts
    /// reached a hosted provider. So a pin has to be one this bridge already calls local.
    #[test]
    fn a_pinned_model_must_be_one_this_bridge_calls_local() {
        let _env = env_guard();
        // SAFETY: `_env` holds ENV_LOCK, so no other test reads or writes these vars concurrently,
        // and outside the tests they are read only by from_env.
        unsafe {
            std::env::set_var("HERDR_TG_SUMMARIZER_KEY", "k");
            std::env::set_var("HERDR_TG_SUMMARIZER_MODEL", "glm-5.3-flash");
        }
        assert!(Summarizer::from_env().is_none(), "a hosted pin is refused");
        let Setup::Refused(why) = Summarizer::configure() else {
            panic!("this has to read as refused, not as nobody having switched it on");
        };
        assert!(
            why.contains("HERDR_TG_SUMMARIZER_MODEL"),
            "which variable? {why}"
        );
        assert!(why.contains(LOCAL_MODELS_ENV), "and where to add it? {why}");

        unsafe { std::env::set_var("HERDR_TG_SUMMARIZER_MODEL", "local-coder") };
        assert_eq!(
            Summarizer::from_env()
                .expect("a model this bridge calls local is fine")
                .model
                .as_deref(),
            Some("local-coder")
        );

        unsafe { std::env::set_var("HERDR_TG_SUMMARIZER_MODEL", "   ") };
        assert_eq!(
            Summarizer::from_env().expect("blank is not a pin").model,
            None,
            "an empty setting is the operator not pinning anything"
        );

        unsafe {
            std::env::set_var("HERDR_TG_SUMMARIZER_LOCAL_MODELS", "my-local-thing");
            std::env::set_var("HERDR_TG_SUMMARIZER_MODEL", "my-local-thing");
        }
        assert!(
            Summarizer::from_env().is_some(),
            "the operator can name what actually runs on their machine"
        );

        unsafe { std::env::set_var("HERDR_TG_SUMMARIZER_LOCAL_MODELS", "*") };
        assert!(
            Summarizer::from_env().is_none(),
            "there is no wildcard, because it would read as permission it does not grant"
        );
    }

    /// The gateway credential must not be able to reach a log through `{:?}`. `Ctx` holds this
    /// struct, so one debug line added later would otherwise print a live key.
    #[test]
    fn the_gateway_key_cannot_reach_a_log_through_debug() {
        let mut s = at(
            "http://127.0.0.1:8090/v1/chat/completions",
            &["local-coder"],
        );
        s.key = "lg_live_DISTINCTIVE".into();
        let shown = format!("{s:?}");
        assert!(
            !shown.contains("lg_live_DISTINCTIVE"),
            "the key was printed: {shown}"
        );
        assert!(shown.contains("<redacted>"));
        assert!(
            shown.contains("127.0.0.1:8090"),
            "the rest of the struct stays debuggable"
        );
    }

    /// Every shape of "not this machine" the operator could plausibly end up with, including the
    /// ones that LOOK local. `http://127.0.0.1@evil.example/` has host `evil.example`.
    #[test]
    fn every_shape_of_not_this_machine_is_refused() {
        for here in [
            "http://127.0.0.1:8090/v1/chat/completions",
            "http://localhost:8090/v1",
            "http://LOCALHOST:8090/v1",
            "http://localhost./v1",
            "http://[::1]:8090/v1",
            "http://[::ffff:127.0.0.1]/v1",
            "http://127.9.9.9:1/v1",
            "https://127.0.0.1/v1",
            "http://user:pass@127.0.0.1:8090/v1",
        ] {
            assert!(is_local_endpoint(here), "this is on this machine: {here:?}");
        }
        for elsewhere in [
            "https://api.z.ai/api/coding/paas/v4/chat/completions",
            "http://127.0.0.1@evil.example/v1",
            "http://localhost.evil.example/v1",
            "http://evil.example#127.0.0.1",
            "http://[::2]/v1",
            "http://10.0.0.5:8090/v1",
            "file:///tmp/x",
            "not a url",
            "127.0.0.1:8090/v1",
            "",
        ] {
            assert!(
                !is_local_endpoint(elsewhere),
                "this is not on this machine: {elsewhere:?}"
            );
        }
    }

    /// Exact ids, not prefixes: `local-coder-2` is a different model from `local-coder`, and a
    /// prefix match would hand a stranger the operator's screen.
    #[test]
    fn the_responder_allowlist_is_exact_and_case_insensitive() {
        let mut s = at(
            "http://127.0.0.1:1/x",
            &["local-coder", "qwen2.5-coder-1.5b"],
        );
        for ours in [
            "local-coder",
            "LOCAL-CODER",
            "  local-coder  ",
            "qwen2.5-coder-1.5b",
        ] {
            assert!(s.responder_ok(ours), "this one is ours: {ours:?}");
        }
        for stranger in ["", "   ", "glm-5.3-flash", "local-coder-2", "local"] {
            assert!(!s.responder_ok(stranger), "this one is not: {stranger:?}");
        }

        // Consent replaces proof: once the operator has said pane text may leave, there is nothing
        // left for the allowlist to decide.
        s.allow_remote = true;
        for anything in ["", "glm-5.3-flash", "local-coder-2"] {
            assert!(
                s.responder_ok(anything),
                "an explicit grant covers this: {anything:?}"
            );
        }
    }

    /// An ambient proxy in this service's environment must not be able to decide where the
    /// operator's screen goes. One line in a systemd unit or a shell profile is all it takes, and
    /// every startup gate still passes: the address was proved, and then a library sent the bytes
    /// somewhere else entirely — with the gateway key attached.
    ///
    /// This has to run in a child process. reqwest reads the proxy variables once and caches the
    /// answer for the life of the process, so a test that sets them after some other test has
    /// already built a client would pass while proving nothing. The recorders stay here, in the
    /// parent, because their ports have to be known before the child starts. Waiting for the child
    /// blocks this thread, so the runtime needs another one to keep answering on those ports.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_ambient_proxy_must_not_be_able_to_reroute_the_gist() {
        let (gateway, gateway_seen) = fake_gateway_at(vec![
            r#"{"model":"local-coder","choices":[{"message":{"content":"Overwrite config.yaml?"}}]}"#,
            r#"{"model":"local-coder","choices":[{"message":{"content":"Force-push and drop the 2 commits?"}}]}"#,
        ])
        .await;
        let (proxy, proxy_seen) = fake_gateway_at(vec![
            r#"{"model":"glm-5.3-flash","choices":[{"message":{"content":"Force-push?"}}]}"#,
            r#"{"model":"glm-5.3-flash","choices":[{"message":{"content":"Force-push?"}}]}"#,
        ])
        .await;

        let child = std::process::Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "summarize::tests::the_child_the_proxy_test_drives",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .env(
                "HERDR_TG_TEST_GIST_ENDPOINT",
                format!("http://{gateway}/v1/chat/completions"),
            )
            .env("http_proxy", format!("http://{proxy}"))
            .env("HTTP_PROXY", format!("http://{proxy}"))
            .env("all_proxy", format!("http://{proxy}"))
            .env("ALL_PROXY", format!("http://{proxy}"))
            // A machine that already excludes loopback from its proxy would hide the whole defect.
            .env_remove("no_proxy")
            .env_remove("NO_PROXY")
            .output()
            .expect("run the child");

        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        assert!(
            child.status.success(),
            "the summary did not come from the gateway on this machine:\n{said}"
        );
        assert!(
            proxy_seen
                .lock()
                .expect("the recorder is free once the child has exited")
                .is_empty(),
            "an environment variable rerouted the excerpt and the gateway key to a host of its own \
             choosing"
        );
        let sent = gateway_seen
            .lock()
            .expect("the recorder is free once the child has exited")
            .clone();
        assert_eq!(
            sent.len(),
            2,
            "the probe and the excerpt both go straight to the gateway"
        );
        assert!(sent[1].contains("SECRET-PANE-TEXT"));
    }

    /// Driven by [`an_ambient_proxy_must_not_be_able_to_reroute_the_gist`], which starts it with
    /// the proxy variables already in the environment. Ignored so nothing else runs it.
    #[tokio::test]
    #[ignore = "started by the proxy test, which sets the environment it needs"]
    async fn the_child_the_proxy_test_drives() {
        let Ok(endpoint) = std::env::var("HERDR_TG_TEST_GIST_ENDPOINT") else {
            eprintln!(
                "this test is started by an_ambient_proxy_must_not_be_able_to_reroute_the_gist"
            );
            return;
        };
        let got = at(&endpoint, &["local-coder"])
            .one_line("SECRET-PANE-TEXT force-push? [y/N]")
            .await;
        assert_eq!(
            got.as_deref(),
            Some("Force-push and drop the 2 commits?"),
            "the summary has to have come from the gateway on this machine, not from the proxy"
        );
    }

    /// A gist gateway has no business redirecting, and following one would hand the excerpt to
    /// whatever host the redirect names — reqwest re-sends a POST body on 307 and 308.
    #[tokio::test]
    async fn a_gateway_that_redirects_is_not_followed_anywhere() {
        let (elsewhere, elsewhere_seen) = fake_gateway_at(vec![
            r#"{"model":"local-coder","choices":[{"message":{"content":"Overwrite config.yaml?"}}]}"#,
            r#"{"model":"local-coder","choices":[{"message":{"content":"Force-push and drop the 2 commits?"}}]}"#,
        ])
        .await;
        let (redirector, seen) = fake_server(vec![format!(
            "HTTP/1.1 307 Temporary Redirect\r\nlocation: http://{elsewhere}/v1/chat/completions\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        )])
        .await;

        let got = at(
            &format!("http://{redirector}/v1/chat/completions"),
            &["local-coder"],
        )
        .one_line("SECRET-PANE-TEXT force-push? [y/N]")
        .await;

        assert!(
            got.is_none(),
            "an answer reached through a redirect must not be shown: {got:?}"
        );
        assert!(
            elsewhere_seen
                .lock()
                .expect("the recorder is free once the call returns")
                .is_empty(),
            "the gateway redirected and this bridge carried the request to the host it named"
        );
        let sent = seen
            .lock()
            .expect("the recorder is free once the call returns")
            .clone();
        assert_eq!(sent.len(), 1, "one probe, and nothing after the redirect");
        assert!(!sent[0].contains("SECRET-PANE-TEXT"));
    }

    /// An answer built by hand, so the address the connection reached can be varied without needing
    /// a machine that has one.
    fn answered_by(responder: &str, peer: Option<&str>) -> Answer {
        Answer {
            responder: responder.into(),
            content: "Force-push and drop the 2 commits?".into(),
            peer: peer.map(|p| p.parse().expect("a test address")),
        }
    }

    /// The gate the two blockers walked through: the endpoint was proved at startup and then a
    /// library chose the socket. Whatever the reply calls itself, an address that is not on this
    /// machine is a leak, and one this bridge could not read back is one it cannot vouch for.
    #[tokio::test]
    async fn an_answer_from_a_socket_that_is_not_on_this_machine_is_refused() {
        let s = at(
            "http://127.0.0.1:8090/v1/chat/completions",
            &["local-coder"],
        );

        for here in [
            "127.0.0.1:8090",
            "[::1]:8090",
            "[::ffff:127.0.0.1]:8090",
            "127.9.9.9:1",
        ] {
            assert!(
                s.answer_is_local(&answered_by("local-coder", Some(here))),
                "this answer came from this machine: {here}"
            );
        }
        for elsewhere in [
            "192.168.178.155:18099",
            "10.0.0.5:8090",
            "100.64.30.5:443",
            "[::ffff:192.168.178.155]:8090",
            "[2001:db8::1]:8090",
        ] {
            assert!(
                !s.answer_is_local(&answered_by("local-coder", Some(elsewhere))),
                "the bytes went off this machine, and a familiar name does not undo that: \
                 {elsewhere}"
            );
        }
        assert!(
            !s.answer_is_local(&answered_by("local-coder", None)),
            "an address this bridge cannot read back is one it cannot vouch for"
        );

        // Consent replaces proof, here as everywhere else in this file.
        let mut allowed = s.clone();
        allowed.allow_remote = true;
        assert!(
            allowed.answer_is_local(&answered_by("glm-5.3-flash", Some("192.168.178.155:443")))
        );
    }

    /// The bridge must never tell the operator a comforting thing that is not true. `trip` is
    /// reached from two places and only one of them can say nothing of theirs was sent.
    #[test]
    fn the_journal_never_says_nothing_was_sent_once_the_excerpt_has_gone() {
        let s = at(
            "http://127.0.0.1:8090/v1/chat/completions",
            &["local-coder", "local-qwen3"],
        );
        let stranger = answered_by("glm-5.3-flash", Some("127.0.0.1:8090"));

        let after_the_probe = s.why_summaries_stopped(&stranger, false);
        assert!(
            after_the_probe.contains("nothing from the screen was sent"),
            "after the probe nothing of theirs has gone, and the operator should be told so: \
             {after_the_probe}"
        );

        let after_the_excerpt = s.why_summaries_stopped(&stranger, true);
        assert!(
            after_the_excerpt.contains("had already been sent"),
            "the excerpt was on the wire before this reply could be read: {after_the_excerpt}"
        );
        assert!(
            !after_the_excerpt.contains("nothing from the screen was sent"),
            "the journal told the operator their screen was safe when it was already gone: \
             {after_the_excerpt}"
        );
    }

    /// A reply from off this machine is a different failure from a reply with an unfamiliar name,
    /// and the sentence has to send the operator to the two settings that actually cause it.
    #[test]
    fn a_reply_from_off_this_machine_names_the_address_and_the_two_ways_it_happens() {
        let s = at(
            "http://127.0.0.1:8090/v1/chat/completions",
            &["local-coder"],
        );

        let why = s.why_summaries_stopped(
            &answered_by("local-coder", Some("192.168.178.155:18099")),
            true,
        );
        assert!(
            why.contains("192.168.178.155:18099"),
            "which address? {why}"
        );
        assert!(
            why.contains("127.0.0.1:8090"),
            "and which one was configured? {why}"
        );
        assert!(
            why.contains("proxy") && why.contains("redirect"),
            "and where to look? {why}"
        );
        assert!(why.contains("had already been sent"), "{why}");

        let unreadable = s.why_summaries_stopped(&answered_by("local-coder", None), false);
        assert!(unreadable.contains("could not read back"), "{unreadable}");
        assert!(
            unreadable.contains("nothing from the screen was sent"),
            "{unreadable}"
        );
    }

    /// Every sentence the operator reads is prose. A set printed with `{:?}` renders braces and
    /// quotes, which is this crate's vocabulary rather than theirs.
    #[test]
    fn the_operator_reads_the_allowlist_as_words_not_as_a_set_literal() {
        let _env = env_guard();
        let s = at(
            "http://127.0.0.1:8090/v1/chat/completions",
            &["local-coder", "local-qwen3"],
        );
        let why =
            s.why_summaries_stopped(&answered_by("glm-5.3-flash", Some("127.0.0.1:8090")), false);
        assert!(why.contains("local-coder, local-qwen3"), "{why}");

        // SAFETY: `_env` holds ENV_LOCK, so no other test reads or writes these vars concurrently,
        // and outside the tests they are read only by from_env.
        unsafe {
            std::env::set_var("HERDR_TG_SUMMARIZER_KEY", "k");
            std::env::set_var("HERDR_TG_SUMMARIZER_MODEL", "glm-5.3-flash");
        }
        let Setup::Refused(refusal) = Summarizer::configure() else {
            panic!("a hosted pin has to read as refused");
        };
        assert!(
            refusal.contains("local-coder, local-qwen3, qwen2.5-coder-1.5b"),
            "{refusal}"
        );

        for sentence in [&why, &refusal] {
            for jargon in ['{', '}', '"', '[', ']'] {
                assert!(
                    !sentence.contains(jargon),
                    "a person has to read this, and {jargon:?} is this crate's punctuation, \
                     not theirs: {sentence}"
                );
            }
        }
    }

    /// `localhost` is a name, and a name is resolved at connect time by something this crate does
    /// not control. It is accepted anyway because the resolution is pinned rather than trusted —
    /// so the pin has to cover every spelling the URL parser will hand over, and only those.
    #[test]
    fn the_name_localhost_is_pinned_to_loopback_rather_than_trusted_at_connect_time() {
        assert_eq!(
            loopback_pin("http://localhost:8090/v1/chat/completions"),
            Some(("localhost".to_string(), 8090))
        );
        // The URL parser lowercases the host of an http URL, so the pin only ever sees one casing.
        assert_eq!(
            loopback_pin("http://LoCaLhOsT/v1"),
            Some(("localhost".to_string(), 80))
        );
        assert_eq!(
            loopback_pin("https://localhost/v1"),
            Some(("localhost".to_string(), 443))
        );
        // The same name written as a fully qualified one is a different string to the resolver.
        assert_eq!(
            loopback_pin("http://localhost./v1"),
            Some(("localhost.".to_string(), 80))
        );
        // A literal address is already an address; there is nothing for a resolver to decide.
        for literal in [
            "http://127.0.0.1:8090/v1",
            "http://[::1]:8090/v1",
            "not a url",
        ] {
            assert_eq!(loopback_pin(literal), None, "nothing to pin in {literal:?}");
        }
    }

    /// Pinning replaces resolution, so it has to keep the port the operator configured — otherwise
    /// the gate would be safe and the feature would simply never work.
    #[tokio::test]
    async fn a_gateway_reached_by_the_name_localhost_still_answers_on_its_own_port() {
        let (addr, seen) = fake_gateway_at(vec![
            r#"{"model":"local-coder","choices":[{"message":{"content":"Overwrite config.yaml?"}}]}"#,
            r#"{"model":"local-coder","choices":[{"message":{"content":"Force-push and drop the 2 commits?"}}]}"#,
        ])
        .await;

        let got = at(
            &format!("http://localhost:{}/v1/chat/completions", addr.port()),
            &["local-coder"],
        )
        .one_line("Force-push? [y/N]")
        .await;

        assert_eq!(got.as_deref(), Some("Force-push and drop the 2 commits?"));
        assert_eq!(
            seen.lock()
                .expect("the recorder is free once the call returns")
                .len(),
            2,
            "one probe, then the excerpt"
        );
    }
}
