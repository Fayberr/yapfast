//! Validation and storage of a batch, separate from platform and network I/O.

use anyhow::{Result, bail};
use futures_util::{StreamExt, stream};
use std::{collections::HashSet, future::Future};
use yapfast_discord::import::{Discovery, Found, ProfileFailure, ReadFailure, Unsupported};

const VALIDATIONS: usize = 4;

pub(super) fn discovery_result(
    result: std::result::Result<Result<Discovery>, tokio::task::JoinError>,
) -> Result<Discovery> {
    match result {
        Ok(Ok(discovery)) => Ok(discovery),
        Ok(Err(error)) => {
            tracing::warn!(
                error = format!("{error:#}"),
                "could not discover Discord app sessions"
            );
            let message = if error.is::<Unsupported>() {
                error.to_string()
            } else {
                ReadFailure::from_error(&error).to_string()
            };
            bail!("{message}")
        }
        Err(error) => {
            tracing::warn!(
                error = format!("{error:#}"),
                "Discord app discovery worker failed"
            );
            bail!(
                "The Discord import worker stopped unexpectedly. Retry the import or use QR login."
            )
        }
    }
}

/// Identity and profile returned by the authenticated /users/@me request.
pub(super) struct Validated {
    pub id: String,
    pub token: String,
    pub username: String,
    pub name: String,
}

/// A cause for each failed candidate, without its credentials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    SignedOut,
    Timeout,
    Service(u16),
    Network,
    Invalid,
    Storage,
}

impl Failure {
    pub fn refused(status: u16) -> Self {
        if status == 401 {
            Self::SignedOut
        } else {
            Self::Service(status)
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignedOut => f.write_str("Discord reports the session is signed out. Sign in to Discord and retry."),
            Self::Timeout => f.write_str("Discord validation timed out. Check your connection and retry."),
            Self::Service(status) => write!(f, "Discord returned HTTP {status}. Retry later."),
            Self::Network => f.write_str("Yapfast could not reach Discord. Check your connection and retry."),
            Self::Invalid => f.write_str("Discord session data could not be validated. Use QR login to link the account."),
            Self::Storage => f.write_str("Yapfast could not save the account in the OS credential store. Check access to the credential store and retry."),
        }
    }
}

impl std::error::Error for Failure {}

#[derive(Debug, Default)]
pub(super) struct Summary {
    pub linked: usize,
    pub added: usize,
    pub failures: Vec<Failure>,
    pub unreadable_profiles: Vec<ProfileFailure>,
}

impl Summary {
    pub fn message(&self) -> String {
        let mut message = if self.linked == 0 {
            "No accounts were imported.".into()
        } else if self.added == 0 {
            let noun = if self.linked == 1 {
                "account"
            } else {
                "accounts"
            };
            format!("{} {noun} already linked.", self.linked)
        } else {
            let noun = if self.added == 1 {
                "account"
            } else {
                "accounts"
            };
            format!(
                "Imported {} {noun}; {} already linked.",
                self.added,
                self.linked - self.added
            )
        };
        let mut reported = Vec::new();
        for cause in &self.failures {
            if reported.contains(cause) {
                continue;
            }
            reported.push(*cause);
            let count = self
                .failures
                .iter()
                .filter(|failure| *failure == cause)
                .count();
            let noun = if count == 1 { "session" } else { "sessions" };
            message.push_str(&format!(" {count} {noun} could not be linked. {cause}"));
        }
        if !self.unreadable_profiles.is_empty() {
            let count = self.unreadable_profiles.len();
            let noun = if count == 1 { "profile" } else { "profiles" };
            message.push_str(&format!(" {count} unreadable {noun}."));
            for failure in &self.unreadable_profiles {
                message.push_str(&format!(" {}", failure.message()));
            }
        }
        if self.linked == 0 && self.failures.is_empty() && self.unreadable_profiles.is_empty() {
            message
                .push_str(" No saved Discord sessions were found. Sign in to Discord and retry.");
        }
        message
    }
}

pub(super) async fn run<F, Fut, S, Save>(
    discovery: Discovery,
    validate: F,
    mut save: S,
) -> Result<Summary>
where
    F: FnMut(Found) -> Fut,
    Fut: Future<Output = Result<Validated>>,
    S: FnMut(Validated) -> Save,
    Save: Future<Output = Result<bool>>,
{
    let mut summary = Summary {
        unreadable_profiles: discovery.unreadable_profiles,
        ..Summary::default()
    };
    let mut seen = HashSet::new();
    // Futures belong to this task: cancelling it also drops pending validation.
    let mut pending = stream::iter(discovery.accounts)
        .map(validate)
        .buffer_unordered(VALIDATIONS);
    while let Some(result) = pending.next().await {
        let account = match result {
            Ok(account) if !account.id.is_empty() => account,
            Ok(_) => {
                tracing::warn!("Discord account validation returned no user ID");
                summary.failures.push(Failure::Invalid);
                continue;
            }
            Err(error) => {
                tracing::info!(
                    error = format!("{error:#}"),
                    "could not validate a Discord app session"
                );
                summary.failures.push(
                    error
                        .downcast_ref::<Failure>()
                        .copied()
                        .unwrap_or(Failure::Network),
                );
                continue;
            }
        };
        if seen.contains(&account.id) {
            continue;
        }
        let id = account.id.clone();
        match save(account).await {
            Ok(added) => {
                seen.insert(id);
                summary.linked += 1;
                summary.added += usize::from(added);
            }
            Err(error) => {
                tracing::warn!(
                    error = format!("{error:#}"),
                    "could not save a Discord app session"
                );
                summary.failures.push(Failure::Storage);
            }
        }
    }
    if summary.linked == 0 {
        bail!("{}", summary.message());
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::ready;
    use yapfast_discord::import::{Discovery, Flavour, Found};

    async fn run<F, Fut, S>(discovery: Discovery, validate: F, mut save: S) -> Result<Summary>
    where
        F: FnMut(Found) -> Fut,
        Fut: Future<Output = Result<Validated>>,
        S: FnMut(Validated) -> Result<bool>,
    {
        super::run(discovery, validate, move |account| ready(save(account))).await
    }

    fn discovery(tokens: &[&str]) -> Discovery {
        Discovery {
            accounts: tokens
                .iter()
                .map(|token| Found {
                    token: (*token).into(),
                    flavour: Flavour::Stable,
                })
                .collect(),
            unreadable_profiles: vec![],
        }
    }

    fn validated(account: Found) -> Validated {
        Validated {
            id: "100000000000000001".into(),
            token: account.token,
            username: "fixture".into(),
            name: "Fixture".into(),
        }
    }

    #[test]
    fn only_401_is_a_signed_out_session() {
        assert_eq!(Failure::refused(401), Failure::SignedOut);
        for status in [403, 429, 500, 502, 503] {
            assert_eq!(Failure::refused(status), Failure::Service(status));
            assert!(!Failure::refused(status).to_string().contains("Sign in"));
        }
    }

    #[tokio::test]
    async fn worker_failure_is_not_presented_as_a_changing_discord_profile() {
        let result = tokio::task::spawn_blocking(|| -> Result<Discovery> {
            panic!("fixture worker failure")
        })
        .await;
        let message = discovery_result(result).unwrap_err().to_string();
        assert!(message.contains("worker stopped unexpectedly"));
        assert!(!message.contains("Close Discord"));
        for cause in [
            ReadFailure::Location,
            ReadFailure::Limit,
            ReadFailure::MissingState,
            ReadFailure::Key,
        ] {
            let message =
                discovery_result(Ok(Err(anyhow::anyhow!("fixture cause").context(cause))))
                    .unwrap_err()
                    .to_string();
            assert_eq!(message, cause.to_string());
            assert!(!message.contains("Close Discord"));
        }
    }

    #[tokio::test]
    async fn validation_failures_retain_their_cause_in_both_outcomes() {
        for cause in [
            Failure::SignedOut,
            Failure::Timeout,
            Failure::Service(503),
            Failure::Network,
            Failure::Invalid,
        ] {
            let failure = run(
                discovery(&["bad"]),
                |_| ready(Err(anyhow::anyhow!("fixture cause").context(cause))),
                |_| Ok(true),
            )
            .await
            .unwrap_err()
            .to_string();
            assert!(failure.contains(&cause.to_string()), "{failure}");
            assert_eq!(
                failure.contains("Sign in to Discord"),
                cause == Failure::SignedOut
            );
            let partial = run(
                discovery(&["bad", "works"]),
                |account| {
                    ready(if account.token == "bad" {
                        Err(anyhow::anyhow!("fixture cause").context(cause))
                    } else {
                        Ok(validated(account))
                    })
                },
                |_| Ok(true),
            )
            .await
            .unwrap();
            assert_eq!(partial.failures, [cause]);
            assert!(partial.message().contains(&cause.to_string()));
        }
    }

    #[tokio::test]
    async fn validation_runs_concurrently_with_a_limit_of_four() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let summary = super::run(
            discovery(&["1", "2", "3", "4", "5", "6", "7", "8", "9"]),
            |account| {
                let active = active.clone();
                let peak = peak.clone();
                async move {
                    let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(count, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    let mut checked = validated(account);
                    checked.id = checked.token.clone();
                    Ok(checked)
                }
            },
            |_| ready(Ok(true)),
        )
        .await
        .unwrap();
        assert_eq!(peak.load(Ordering::SeqCst), VALIDATIONS);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(summary.added, 9);
    }

    #[tokio::test]
    async fn cancelling_drops_pending_validations_and_stops_further_saves() {
        use std::cell::Cell;
        let started = Cell::new(0);
        let saved = Cell::new(0);
        let future = super::run(
            discovery(&["1", "2", "3", "4", "5"]),
            |_| {
                started.set(started.get() + 1);
                std::future::pending::<Result<Validated>>()
            },
            |_| {
                saved.set(saved.get() + 1);
                ready(Ok(true))
            },
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), future)
                .await
                .is_err()
        );
        assert_eq!(started.get(), VALIDATIONS);
        assert_eq!(saved.get(), 0);
    }

    #[tokio::test]
    async fn network_and_storage_failures_do_not_claim_the_user_is_signed_out() {
        let network = run(
            discovery(&["offline"]),
            |_| ready(Err(anyhow::anyhow!("network unavailable"))),
            |_| Ok(true),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(!network.contains("Sign in to Discord"), "{network}");
        let storage = run(
            discovery(&["valid"]),
            |account| ready(Ok(validated(account))),
            |_| Err(anyhow::anyhow!("keyring unavailable")),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(storage.contains("credential store"), "{storage}");
        assert!(!storage.contains("Sign in to Discord"), "{storage}");
    }

    #[tokio::test]
    async fn rejected_candidates_never_become_success_or_reach_storage() {
        let mut saved = vec!["unrelated existing account".to_owned()];
        let result = run(
            discovery(&["revoked"]),
            |_| ready(Err(anyhow::anyhow!("refused"))),
            |account| {
                saved.push(account.token);
                Ok(true)
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(saved, ["unrelated existing account"]);
    }

    #[tokio::test]
    async fn a_validated_account_already_linked_is_a_success() {
        let result = run(
            discovery(&["known"]),
            |account| ready(Ok(validated(account))),
            |_| Ok(false),
        )
        .await
        .unwrap();
        assert_eq!(result.linked, 1);
        assert_eq!(result.added, 0);
        assert_eq!(result.message(), "1 account already linked.");
    }

    #[tokio::test]
    async fn two_sessions_for_the_same_authenticated_identity_are_saved_once() {
        let mut saved = 0;
        let result = run(
            discovery(&["one", "two"]),
            |account| ready(Ok(validated(account))),
            |_| {
                saved += 1;
                Ok(true)
            },
        )
        .await
        .unwrap();
        assert_eq!(saved, 1);
        assert_eq!(result.added, 1);
        assert_eq!(result.failures.len(), 0);
    }

    #[tokio::test]
    async fn partial_validation_and_profile_failures_keep_the_working_account() {
        let mut input = discovery(&["revoked", "works"]);
        input.unreadable_profiles = vec![yapfast_discord::import::ProfileFailure {
            flavour: Flavour::Stable,
            cause: yapfast_discord::import::ReadFailure::Changed,
        }];
        let result = run(
            input,
            |account| {
                ready(if account.token == "revoked" {
                    Err(anyhow::anyhow!("refused"))
                } else {
                    Ok(validated(account))
                })
            },
            |_| Ok(true),
        )
        .await
        .unwrap();
        assert_eq!(
            (
                result.added,
                result.failures.len(),
                result.unreadable_profiles.len()
            ),
            (1, 1, 1)
        );
        assert!(result.message().contains("1 unreadable profile"));
    }

    #[tokio::test]
    async fn storage_failure_can_retry_a_second_session_for_the_same_identity() {
        let mut attempts = 0;
        let result = run(
            discovery(&["one", "two"]),
            |account| ready(Ok(validated(account))),
            |_| {
                attempts += 1;
                if attempts == 1 {
                    Err(anyhow::anyhow!("vault unavailable"))
                } else {
                    Ok(true)
                }
            },
        )
        .await
        .unwrap();
        assert_eq!((result.linked, result.failures.len(), attempts), (1, 1, 2));
    }

    #[tokio::test]
    async fn unreadable_profiles_offer_a_storage_retry_instead_of_claiming_signed_out() {
        let mut input = discovery(&[]);
        input.unreadable_profiles = vec![yapfast_discord::import::ProfileFailure {
            flavour: Flavour::Stable,
            cause: yapfast_discord::import::ReadFailure::Changed,
        }];
        let error = run(input, |account| ready(Ok(validated(account))), |_| Ok(true))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Close Discord"));
    }

    #[tokio::test]
    async fn candidate_failures_log_error_chains_without_logging_tokens() {
        use std::sync::{Arc, Mutex};
        let logs = Arc::new(Mutex::new(Vec::<u8>::new()));
        struct LogWriter(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for LogWriter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let sink = logs.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || LogWriter(sink.clone()))
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let _ = run(
            discovery(&["private-validation-token"]),
            |_| {
                ready(Err(
                    anyhow::anyhow!("validation failure").context("validation context")
                ))
            },
            |_| Ok(true),
        )
        .await;
        let _ = run(
            discovery(&["private-save-token"]),
            |account| ready(Ok(validated(account))),
            |_| Err(anyhow::anyhow!("vault failure").context("vault context")),
        )
        .await;
        let text = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
        for expected in [
            "validation context",
            "validation failure",
            "vault context",
            "vault failure",
        ] {
            assert!(text.contains(expected), "missing error context: {expected}");
        }
        assert!(!text.contains("private-validation-token") && !text.contains("private-save-token"));
    }
}
