//! Linking a Discord account by QR code, or by a token the person pastes,
//! beside the chat backend, so a build without Matrix can link before
//! anything else runs. The login goes into the OS credential store beside
//! the accounts already linked, and becomes the active one; see
//! `yapfast_discord::remote_auth` and `yapfast_discord::token`.

use tokio::sync::mpsc::{self, UnboundedReceiver};
use yapfast_discord::{
    api::{Api, Refused},
    import::{self, Unsupported},
    remote_auth::{self, Step, Stopped},
    token,
};

use crate::runtime::{BackgroundRuntime, BackgroundTask};

/// How linking goes, for [`crate::application::Application::linking`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    /// The code to show, as the address it carries.
    Code(String),
    /// Someone scanned it, by their username, and confirms on their phone.
    Scanned(String),
    /// Discord wants a captcha solved, on the page at this address.
    Captcha(String),
    /// The account is linked and its login saved.
    Linked,
    /// The code expired before anyone confirmed it.
    Expired,
    Failed(String),
}

/// Starts linking an account on `runtime`: by QR code, or with `token`
/// when the person pasted one. Its progress arrives on the receiver until it
/// ends, and dropping the task stops it.
pub fn start(
    runtime: &BackgroundRuntime,
    token: Option<String>,
) -> (BackgroundTask<()>, UnboundedReceiver<Progress>) {
    let (sender, progress) = mpsc::unbounded_channel();
    let report = move |step| {
        let _ = sender.send(step);
    };
    let task = match token {
        Some(token) => runtime.spawn(with_token(token, report)),
        None => runtime.spawn(run(report)),
    };
    (task, progress)
}

/// Starts importing the accounts the vanilla Discord client keeps on this
/// device, on `runtime`. Each login found is checked against Discord and
/// linked beside any account already there; the result arrives on the
/// receiver as one [`Progress`], as linking does, so the sign-in screen
/// follows it the same way. Dropping the task stops it.
pub fn import(runtime: &BackgroundRuntime) -> (BackgroundTask<()>, UnboundedReceiver<Progress>) {
    let (sender, progress) = mpsc::unbounded_channel();
    let report = move |step| {
        let _ = sender.send(step);
    };
    (runtime.spawn(run_import(report)), progress)
}

/// Finds the vanilla client's logins, links the ones Discord still accepts,
/// and tells `progress` how it went. A login the client holds but Discord has
/// logged out is skipped, not an error.
async fn run_import(progress: impl Fn(Progress)) {
    let found = match import::discover() {
        Ok(found) => found,
        Err(error) if error.is::<Unsupported>() => {
            return progress(Progress::Failed(error.to_string()));
        }
        Err(error) => {
            tracing::warn!(
                error = format!("{error:#}"),
                "could not read the Discord app's logins"
            );
            return progress(Progress::Failed(format!(
                "Yapfast couldn't read the Discord app's accounts: {error}"
            )));
        }
    };
    if found.is_empty() {
        return progress(Progress::Failed(
            "No Discord accounts were found in the Discord app on this device.".into(),
        ));
    }

    let mut added = 0usize;
    for account in found {
        let token = match token::clean(&account.token) {
            Ok(token) => token,
            Err(_) => continue,
        };
        // A login the client kept but Discord logged out, or one Discord no
        // longer accepts, is left out rather than linked dead.
        let described = async {
            let api = Api::new(token.clone())?;
            api.learn_build_number().await;
            let me = api.me().await?;
            let fresh = token::adopt(&token)?;
            let name = me.global_name.unwrap_or_default();
            // The avatar fills in when the account first connects; the name
            // shows in the switcher until then.
            token::describe(&token, &me.username, &name, "")?;
            anyhow::Ok(fresh)
        };
        match described.await {
            Ok(true) => added += 1,
            Ok(false) => {}
            Err(error) => tracing::info!(
                error = format!("{error:#}"),
                flavour = account.flavour.label(),
                "skipped a Discord app login that wouldn't link"
            ),
        }
    }

    // Any linked account, new or already there, means there's one to sign in
    // with; none usable is the one failure worth showing.
    progress(if added > 0 || token::listing().is_ok_and(|list| !list.accounts.is_empty()) {
        Progress::Linked
    } else {
        Progress::Failed(
            "The Discord app's accounts couldn't be linked. They may be signed out.".into(),
        )
    });
}

/// Links the account a pasted token belongs to, once Discord accepts it.
async fn with_token(token: String, progress: impl Fn(Progress)) {
    let token = match token::clean(&token) {
        Ok(token) => token,
        Err(error) => return progress(Progress::Failed(error.to_string())),
    };
    let checked = async {
        let api = Api::new(token.clone())?;
        api.learn_build_number().await;
        api.me().await?;
        token::store(&token)
    };
    progress(match checked.await {
        Ok(()) => Progress::Linked,
        Err(error) => match error.downcast_ref::<Refused>() {
            Some(Refused(status)) if status.as_u16() == 401 => {
                Progress::Failed("Discord didn't accept that token.".into())
            }
            _ => {
                tracing::warn!(
                    error = format!("{error:#}"),
                    "could not link Discord by token"
                );
                Progress::Failed(format!("Yapfast couldn't link Discord: {error}"))
            }
        },
    });
}

/// Links an account, telling `progress` how it goes until it ends.
async fn run(progress: impl Fn(Progress)) {
    let linked = remote_auth::link(|step| {
        progress(match step {
            Step::Code(address) => Progress::Code(address),
            Step::Scanned { name, .. } => Progress::Scanned(name),
            Step::Captcha(page) => Progress::Captcha(page),
        })
    })
    .await;
    progress(match linked.and_then(|login| token::store(&login)) {
        Ok(()) => Progress::Linked,
        Err(error) => match error.downcast_ref::<Stopped>() {
            Some(Stopped::Expired) => Progress::Expired,
            Some(stopped) => Progress::Failed(stopped.to_string()),
            None => {
                tracing::warn!(error = format!("{error:#}"), "could not link Discord");
                Progress::Failed(format!("Yapfast couldn't link Discord: {error}"))
            }
        },
    });
}
