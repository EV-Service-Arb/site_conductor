//! Composition root: settings, error monitoring and logs, then `serve`, `migrate` and
//! `gen-data-key`; every other operation is `/api/v1`'s.

// `ev_lib::settings!` expands once per field and recurses; the settings outgrew the default 128.
#![recursion_limit = "256"]

mod config;
mod settings;

use std::{net::SocketAddr, sync::Arc};

use clap::{Parser, Subcommand};
use ev_lib::error_monitoring;
use eyre::WrapErr;
use sa_auth::Catalog;
use sa_core::notify::Locale;
use sa_panel::{Panel, alerts::Hub, seal::DataKey, store::Store};
use sa_server::{
	alerts::{self, Alerting},
	concierge::{Concierge, DevIdentity},
	forward::{Forward, Services},
	grafana::Grafana,
	http, live, mail,
	outlets::{Discord, Networks},
	signin::{SignIn, SignInConfig},
	telegram::BotName,
	tokens, trash,
	web::{self, Files},
};
use social_networks_adapters::notify::{
	email::{Mailer, Smtp},
	telegram::{BASE, Bot},
};

use crate::settings::Settings;

#[derive(Parser)]
#[command(name = "panel", version, about = "The Service-Arb conductor: the sign-in, the forward to the services behind it, and its hubs")]
struct Cli {
	#[command(subcommand)]
	cmd: Cmd,
	#[command(flatten)]
	config: config::SettingsFlags,
	/// Buy the Google answers the queries cache lacks, on real quota; without it only cached ones are served.
	#[arg(long, global = true)]
	spend_real_quota_i_am_certain: bool,
}

#[derive(Subcommand)]
enum Cmd {
	/// Create the databases at PANEL_DB_PATH and PANEL_HISTORY_DB_PATH if need be and apply the
	/// migrations each lacks, then exit. Every command does as much on open, `serve` included; this
	/// one does nothing else.
	Migrate,
	/// The queries cache between two conductors' databases (`scripts/queries-cache-sync.sh`).
	QueriesCache {
		#[command(subcommand)]
		cmd: QueriesCacheCmd,
	},
	/// The HTTP API.
	Serve {
		/// The dev shell sets PANEL_BIND to this machine's port.
		#[arg(long, env = "PANEL_BIND")]
		bind: SocketAddr,
	},
	/// Print a fresh PANEL_DATA_KEY.
	GenDataKey,
}

#[derive(Subcommand)]
enum QueriesCacheCmd {
	/// Every version held, the latest and the history's: a JSON line `[scheme, key, fetched_at]` each.
	Index,
	/// Every version held that `--except` (an `index`'s output) lacks, a JSON line each.
	Export {
		#[arg(long)]
		except: std::path::PathBuf,
	},
	/// `export`'s lines from stdin, each kept as of its own `fetched_at`.
	Import,
}

// Sentry must be initialised before the async runtime starts, hence no #[tokio::main].
fn main() -> eyre::Result<()> {
	// a sandbox worker, before anything of the conductor's is set up (docs/ARCHITECTURE.md, Code mode)
	if std::env::args().nth(1).as_deref() == Some(sa_sandbox::WORKER) {
		sa_sandbox::worker();
	}
	color_eyre::install()?;

	// What nix/contract.nix's `provides` must equal (tests/it/contract.rs).
	if std::env::args().nth(1).as_deref() == Some("--print-provided-paths") {
		for path in sa_server::PROVIDED_PATHS {
			println!("{path}");
		}
		return Ok(());
	}
	// What nix/contract.nix's ingress and `consumes` must equal (tests/it/contract.rs).
	if std::env::args().nth(1).as_deref() == Some("--print-paths") {
		let paths = sa_server::forward::paths();
		let listed: Vec<&str> = match std::env::args().nth(2).as_deref() {
			Some("rate-limited") => paths.filter(|p| p.rate_limited).map(|p| p.prefix).collect(),
			Some("unpublished") => paths.filter(|p| !p.published).map(|p| p.prefix).collect(),
			Some("consumed") => settings::CONSUMED.to_vec(),
			other => eyre::bail!("--print-paths rate-limited|unpublished|consumed, not {other:?}"),
		};
		for path in listed {
			println!("{path}");
		}
		return Ok(());
	}
	// The deploy contract, straight out of the image: the gitops preflight diffs it with the
	// cluster Secret's keys, so a missing variable is caught before the rollout.
	if let Some(profile) = settings::print_required_vars_for() {
		for var in Settings::required_var_names(&profile) {
			println!("{var}");
		}
		return Ok(());
	}
	// Exits 78 (EX_CONFIG) on a bad environment, before anything else is built.
	let settings = ev_lib::settings::or_exit(Settings::from_env());
	// Refused like a bad environment, whatever the command: in production, or off loopback,
	// it would let anyone in.
	let dev_sign_in = match settings.dev_sign_in() {
		Ok(d) => d,
		Err(e) => {
			eprintln!("{e:#}");
			std::process::exit(ev_lib::settings::EX_CONFIG);
		}
	};

	// Held for the life of main: dropping it flushes. A no-op without SENTRY_DSN.
	let _sentry = error_monitoring::init(&error_monitoring::Config {
		dsn: settings.sentry_dsn.clone(),
		environment: settings.app_env.clone(),
		release: error_monitoring::release_name!().map(|r| r.into_owned()),
		// the same name OTEL uses, so an issue and its trace agree on the service
		service: std::env::var("OTEL_SERVICE_NAME").ok().filter(|s| !s.trim().is_empty()),
		traces_sample_rate: error_monitoring::Config::traces_sample_rate_for(&settings.app_env),
	});
	let cli = Cli::parse();
	init_tracing()?;

	tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
		.wrap_err("building the tokio runtime")?
		.block_on(run(cli, settings, dev_sign_in))
}

/// Logs go to stderr, so `gen-data-key`'s stdout stays its output.
fn init_tracing() -> eyre::Result<()> {
	use tracing_subscriber::{EnvFilter, fmt, prelude::*};

	let filter = EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new(option_env!("LOG_DIRECTIVES").unwrap_or("info")))?;
	tracing_subscriber::registry()
		.with(filter)
		.with(fmt::layer().with_writer(std::io::stderr))
		.with(error_monitoring::tracing_layer())
		.with(sa_trace::layer())
		.init();
	Ok(())
}

async fn run(cli: Cli, settings: Settings, dev_sign_in: Option<settings::DevSignIn>) -> eyre::Result<()> {
	let flags = cli.config;
	let spend = cli.spend_real_quota_i_am_certain;
	let accounts = || -> eyre::Result<Arc<sa_places::Accounts>> {
		let config = config::Config::try_build(flags.clone()).map_err(|e| eyre::eyre!("the config file: {e}"))?;
		eyre::ensure!(
			settings.app_env != "production" || !config.google.is_empty(),
			"production needs a Google account in the config file's `google`"
		);
		Ok(Arc::new(sa_places::Accounts::new(config.google, config.slices)))
	};
	let open = || async { Store::open(settings.db_path()?, settings.history_db_path()?, sa_panel::live::Bus::default()).await };
	let connect = || async {
		let store = open().await?;
		let accounts = accounts()?;
		let keys = sa_places::Keys::new(
			accounts
				.iter()
				.filter(|a| a.export.is_some()) // a key bills only on an account whose export we read
				.filter_map(|a| Some((a.name.clone(), a.provides.places.as_deref()?.to_owned())))
				.collect(),
		);
		let rank = sa_panel::meters::ByStanding {
			store: store.clone(),
			accounts: accounts.clone(),
		};
		let google = sa_places::Google::try_new(keys, Arc::new(rank), Arc::new(store.clone()))?.spend(spend);
		let smtp = Arc::new(sa_panel::mail::Gmail);
		eyre::Ok(Panel::new(store, settings.data_key()?, settings.renewal()?, google, accounts, smtp))
	};
	match cli.cmd {
		Cmd::Migrate => {
			open().await?.close().await;
			Ok(())
		}
		Cmd::QueriesCache { cmd } => {
			let store = open().await?;
			queries_cache(&store, cmd).await?;
			store.close().await;
			Ok(())
		}
		Cmd::Serve { bind } => {
			let identity = match dev_sign_in {
				Some(dev) => Identity::Dev(dev),
				None =>
					Identity::Concierge(settings.sign_in()?.ok_or_else(|| {
						eyre::eyre!("serve needs the sign-in: PANEL_PUBLIC_ORIGIN, CONCIERGE_PUBLIC_ORIGIN, CONCIERGE_GRPC_ADDR, RP_CLIENT_SECRET_SA (or PANEL_DEV_SIGN_IN)")
					})?),
			};
			let forward = settings.forward()?.map(Services::new);
			let networks = AlertSettings {
				telegram: settings.telegram()?,
				smtp: settings.smtp()?,
				discord: settings.panel_public_origin.as_deref().map(|o| settings.discord(url::Url::parse(o)?.join(alerts::DISCORD_CALLBACK)?)).transpose()?.flatten(),
			};
			eyre::ensure!(
				networks.telegram.is_none() || forward.is_some(),
				"TELEGRAM_BOT_TOKEN needs the forward (PANEL_ASSERTION_KEY and the services' URLs): the hub reads the services' feeds"
			);
			let grafana = settings.grafana()?;
			eyre::ensure!(
				grafana.is_none() || forward.is_some(),
				"PANEL_GRAFANA_SERVER needs the forward (PANEL_ASSERTION_KEY and the services' URLs): Grafana reads the services through it"
			);
			let panel = connect().await?;
			let readers = panel
				.accounts()
				.iter()
				.filter(|a| a.export.is_some())
				.map(sa_panel::meters::google::Google::try_new)
				.collect::<eyre::Result<Vec<_>>>()?;
			for a in panel.accounts().iter() {
				tracing::info!(
					account = a.name,
					export = a.export.as_ref().map(|e| e.table.as_str()),
					places = a.provides.places.is_some(),
					browser = a.provides.browser.is_some(),
					"Google account"
				);
			}
			serve(
				panel,
				identity,
				forward,
				networks,
				grafana,
				settings.web()?,
				settings.browsers()?,
				settings.model(),
				settings.placement(),
				readers,
				settings.app_env == "production",
				bind,
			)
			.await
		}
		Cmd::GenDataKey => {
			println!("{}", DataKey::generate_hex()?);
			Ok(())
		}
	}
}

async fn queries_cache(store: &Store, cmd: QueriesCacheCmd) -> eyre::Result<()> {
	use std::io::{BufRead, Write};

	use sa_core::queries_cache::{Key, Scheme};

	let mut out = std::io::BufWriter::new(std::io::stdout().lock());
	match cmd {
		QueriesCacheCmd::Index =>
			for held in store.queries_index().await? {
				serde_json::to_writer(&mut out, &held)?;
				writeln!(out)?;
			},
		QueriesCacheCmd::Export { except } => {
			let mut lacking = store.queries_index().await?;
			let text = std::fs::read_to_string(&except).wrap_err_with(|| format!("reading the index {}", except.display()))?;
			for (n, line) in text.lines().enumerate() {
				let held: (Scheme, Key, jiff::Timestamp) = serde_json::from_str(line).wrap_err_with(|| format!("{} line {}", except.display(), n + 1))?;
				lacking.remove(&held);
			}
			for (scheme, key, at) in &lacking {
				let version = store
					.query_version(*scheme, *key, *at)
					.await?
					.ok_or_else(|| eyre::eyre!("{scheme} {key} as of {at}: indexed, then gone"))?;
				serde_json::to_writer(&mut out, &version)?;
				writeln!(out)?;
			}
			tracing::info!(versions = lacking.len(), "exported");
		}
		QueriesCacheCmd::Import => {
			let mut kept = 0_usize;
			for (n, line) in std::io::stdin().lock().lines().enumerate() {
				let v: sa_core::queries_cache::Version = serde_json::from_str(&line?).wrap_err_with(|| format!("line {}", n + 1))?;
				store
					.keep_query(v.scheme, v.key, &v.answer.to_string(), &v.request, None, v.fetched_at)
					.await?
					.wrap_err_with(|| format!("line {}: {} {}", n + 1, v.scheme, v.key))?;
				kept += 1;
			}
			tracing::info!(versions = kept, "imported");
		}
	}
	out.flush()?;
	Ok(())
}

/// What alerts go out through, as configured.
struct AlertSettings {
	telegram: Option<settings::TelegramSettings>,
	smtp: Option<Smtp>,
	discord: Option<Discord>,
}

/// Who signs people in.
enum Identity {
	Concierge(settings::SignInSettings),
	/// `PANEL_DEV_SIGN_IN`, already refused outside development and off loopback.
	Dev(settings::DevSignIn),
}

#[expect(clippy::too_many_arguments, reason = "each part of the environment serve runs with, named at its one call")]
async fn serve(
	panel: Panel,
	identity: Identity,
	services: Option<Services>,
	networks: AlertSettings,
	grafana: Option<settings::GrafanaSettings>,
	files: Option<Files>,
	browsers: Option<sa_server::browser::Connected>,
	model: sa_server::claude::Model,
	placement: Option<sa_server::placement::Placement>,
	readers: Vec<sa_panel::meters::google::Google>,
	production: bool,
	bind: SocketAddr,
) -> eyre::Result<()> {
	let keep_by_default = matches!(identity, Identity::Dev(_));
	let (concierge, config) = match identity {
		Identity::Concierge(s) => {
			tracing::info!(panel_origin = s.panel_origin, "signing in through concierge");
			let concierge = Concierge::try_new(&s.concierge_grpc, zeroize::Zeroizing::new(s.client_secret))?;
			// Before serving: a user's permissions name what this catalog defines.
			concierge
				.publish_catalog(&Catalog::collect("sa", s.build_epoch))
				.await
				.wrap_err_with(|| format!("publishing the sa catalog (version {}) to concierge", s.build_epoch))?;
			tracing::info!(version = s.build_epoch, "sa catalog published to concierge");
			let config = SignInConfig {
				panel_origin: s.panel_origin,
				concierge_origin: s.concierge_origin,
			};
			(concierge, config)
		}
		Identity::Dev(d) => {
			let who = DevIdentity {
				user_id: DevIdentity::user_of(&panel, &d.email).await?,
				permissions: d.permissions,
				email: d.email,
			};
			tracing::warn!(
				permissions = ?who.permissions,
				email = who.email,
				user_id = %who.user_id,
				panel_origin = d.panel_origin,
				"DEV SIGN-IN ON (PANEL_DEV_SIGN_IN): /auth/login signs anyone in as this user (or ?as=<email>), no concierge — development only"
			);
			let config = SignInConfig {
				concierge_origin: d.panel_origin.clone(),
				panel_origin: d.panel_origin,
			};
			(Concierge::dev(who), config)
		}
	};
	let (stop, stopped) = tokio::sync::watch::channel(false);
	let bus = panel.bus().clone();
	let cabinet = config.concierge_origin.clone();
	let origin = config.panel_origin.clone();
	let hub_panel = panel.clone();
	let hub_concierge = concierge.clone();
	let engine = panel.clone();
	let metered = panel.clone();
	let purger = panel.clone();
	let grafana_root = format!("{}/grafana/", config.panel_origin.trim_end_matches('/'));
	let mail = mail::Worker {
		panel: panel.clone(),
		model: model.clone(),
		by_itself: production,
		tell: Some(networks.telegram.as_ref().map_or(Locale::En, |tg| tg.locale)),
	};
	let sign_in = SignIn::new(panel, concierge, config);
	let forward = match &services {
		Some(services) => Some(Forward::new(sign_in.clone(), services.clone())),
		None => {
			tracing::warn!("forward not configured: no ingest, no /api/sites, /api/review_archive or /api/playbook, no live updates from sites");
			None
		}
	};
	let alerting = match &forward {
		Some(forward) => {
			let origin: url::Url = origin.parse().wrap_err("the panel's origin is not a URL")?;
			let mut outlets = Networks::try_new(&hub_panel.alerts_push_seed(), &origin)?;
			let (mut bot, mut locale) = (BotName::default(), Locale::En);
			if let Some(tg) = networks.telegram {
				outlets.telegram = Some(Bot::try_new(BASE, zeroize::Zeroizing::new(tg.token))?);
				(bot, locale) = (BotName::new(tg.username), tg.locale);
			}
			outlets.email = networks.smtp.map(Mailer::try_new).transpose()?;
			outlets.discord = networks.discord;
			tracing::info!(methods = ?outlets.methods(), "alerts on");
			let hub = Hub {
				panel: hub_panel,
				outlets,
				concierge: hub_concierge,
				feeds: forward.clone(),
				locale,
				cabinet: cabinet.parse().wrap_err("the concierge origin is not a URL")?,
				origin,
			};
			Some(Alerting { hub, bot })
		}
		None => None,
	};
	let grafana = match (grafana, &forward) {
		(Some(g), Some(forward)) => {
			tracing::info!(root_url = grafana_root, "grafana at /grafana/");
			Some((Grafana::new(forward.clone(), g.dir), g.server))
		}
		(Some(_), None) => unreachable!("refused before serve"),
		(None, _) => {
			tracing::warn!("PANEL_GRAFANA_SERVER unset: no Grafana, /grafana is a 404");
			None
		}
	};
	if readers.is_empty() {
		tracing::warn!("no Google account in the config file: no meter is read, no map, no Places names");
	}
	if browsers.is_none() {
		tracing::warn!("PANEL_BROWSERS_URL unset: no browsers, their routes answer 503");
	}
	if placement.is_none() {
		tracing::warn!("PANEL_PLACEMENT_DIR unset: no placement, its routes answer 503");
	}
	if model.owner().is_none() {
		tracing::warn!("CLAUDE_CODE_OAUTH_TOKEN unset: the assistant answers only on a person's own Claude");
	}
	let catalog = forward.as_ref().map(|f| f.catalog().clone());
	let browsers_followed = browsers.clone();
	let sandbox = sa_server::execute::Sandbox::new(std::env::current_exe().wrap_err("the conductor's own binary: the sandbox's workers")?);
	let app = http::app(
		sign_in,
		forward,
		grafana.as_ref().map(|(g, _)| g),
		http::Limits::default(),
		alerting.clone(),
		browsers,
		Some(model),
		sandbox.clone(),
		placement,
	);
	let app = match files {
		Some(files) => web::serve(app, files),
		None => {
			tracing::warn!("PANEL_WEB_DIR unset: serving the API alone, no front end");
			app
		}
	};
	let listener = tokio::net::TcpListener::bind(bind).await.wrap_err_with(|| format!("binding {bind}"))?;
	tracing::info!(%bind, "serving");
	let server = async {
		let served = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await.wrap_err("HTTP server");
		// The live sockets outlive the graceful shutdown (an upgraded connection is no longer the
		// server's): told to close, and given a moment for their close frames to go out.
		bus.publish(sa_panel::live::Signal::GoingAway);
		bus.drained(std::time::Duration::from_secs(2)).await;
		// The hubs stop with the server; with none running nobody listens, which is fine.
		let _sent = stop.send(true);
		served
	};
	let hubs = async {
		let alerting = async {
			if let Some(a) = alerting {
				alerts::run(a, stopped.clone()).await;
			}
		};
		let feed = async {
			if let Some(services) = &services {
				live::follow(services.clone(), bus.clone(), stopped.clone()).await;
			}
		};
		let usage = sa_server::usage::run(engine.clone(), services.clone(), browsers_followed, stopped.clone());
		let inboxes = sa_server::connections::follow(engine.clone(), stopped.clone());
		let mail = mail::run(mail, stopped.clone());
		let tokens = async {
			if let Some(services) = &services {
				tokens::run(engine, services.clone(), stopped.clone()).await;
			}
		};
		let grafana = async {
			if let Some((grafana, server)) = grafana {
				grafana.run(&server, &grafana_root, stopped.clone()).await;
			}
		};
		let purge = trash::run(purger, keep_by_default, stopped.clone());
		let catalog = async {
			if let (Some(catalog), Some(services)) = (&catalog, &services) {
				catalog.follow(services.clone(), stopped.clone()).await;
			}
		};
		// a dev's holds would overwrite what the guard holds prod's accounts to
		let meters = sa_server::meters::run(metered, readers, production, stopped.clone());
		let workers = sandbox.run(stopped.clone());
		tokio::join!(alerting, feed, usage, inboxes, mail, tokens, grafana, purge, meters, catalog, workers);
	};
	let (served, ()) = tokio::join!(server, hubs);
	served
}

async fn shutdown_signal() {
	let ctrl_c = async {
		if let Err(e) = tokio::signal::ctrl_c().await {
			tracing::error!(error = %e, "listening for Ctrl-C");
			std::future::pending::<()>().await;
		}
	};
	#[cfg(unix)]
	let term = async {
		match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
			Ok(mut s) => {
				s.recv().await;
			}
			Err(e) => {
				tracing::error!(error = %e, "listening for SIGTERM");
				std::future::pending::<()>().await;
			}
		}
	};
	#[cfg(not(unix))]
	let term = std::future::pending::<()>();
	tokio::select! {
		() = ctrl_c => {}
		() = term => {}
	}
	tracing::info!("shutting down");
}