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
use sa_panel::{Panel, alerts::Hub, seal::DataKey, store::Store};
use sa_server::{
	alerts::{self, Telegram},
	concierge::{Concierge, DevIdentity},
	forward::{Forward, Services},
	grafana::Grafana,
	http, live,
	signin::{SignIn, SignInConfig},
	telegram::{self, BotApi, BotName},
	tokens, trash,
	web::{self, Files},
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
	/// Create the database at PANEL_DB_PATH if need be and apply the migrations it lacks, then
	/// exit. Every command does as much on open, `serve` included; this one does nothing else.
	Migrate,
	/// The HTTP API.
	Serve {
		/// The dev shell sets PANEL_BIND to this machine's port.
		#[arg(long, env = "PANEL_BIND")]
		bind: SocketAddr,
	},
	/// Print a fresh PANEL_DATA_KEY.
	GenDataKey,
}

// Sentry must be initialised before the async runtime starts, hence no #[tokio::main].
fn main() -> eyre::Result<()> {
	color_eyre::install()?;

	// What the contract's `provides` must equal (the flake's `checks.provided-paths`).
	if std::env::args().nth(1).as_deref() == Some("--print-provided-paths") {
		for path in sa_server::PROVIDED_PATHS {
			println!("{path}");
		}
		return Ok(());
	}
	// What the flake's ingress and `consumes` must equal (its `checks.paths`).
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
		Ok(Arc::new(sa_places::Accounts::new(config.google)))
	};
	let connect = || async {
		let store = Store::open(settings.db_path()?, sa_panel::live::Bus::default()).await?;
		let accounts = accounts()?;
		let keys = sa_places::Keys::new(accounts.iter().filter_map(|a| Some((a.name.clone(), a.provides.places.as_deref()?.to_owned()))).collect());
		let rank = sa_panel::meters::ByStanding {
			store: store.clone(),
			accounts: accounts.clone(),
		};
		let google = sa_places::Google::try_new(keys, Arc::new(rank), Arc::new(store.clone()))?.spend(spend);
		eyre::Ok(Panel::new(store, settings.data_key()?, settings.renewal()?, google, accounts))
	};
	match cli.cmd {
		Cmd::Migrate => {
			Store::open(settings.db_path()?, sa_panel::live::Bus::default()).await?.pool().close().await;
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
			let telegram = settings.telegram()?;
			eyre::ensure!(
				telegram.is_none() || forward.is_some(),
				"TELEGRAM_BOT_TOKEN needs the forward (PANEL_ASSERTION_KEY and the services' URLs): the hub reads the services' feeds"
			);
			let grafana = settings.grafana()?;
			eyre::ensure!(
				grafana.is_none() || forward.is_some(),
				"PANEL_GRAFANA_SERVER needs the forward (PANEL_ASSERTION_KEY and the services' URLs): Grafana reads the services through it"
			);
			let panel = connect().await?;
			let readers = panel.accounts().iter().map(sa_panel::meters::google::Google::try_new).collect::<eyre::Result<Vec<_>>>()?;
			for a in panel.accounts().iter() {
				tracing::info!(
					account = a.name,
					export = a.export.table,
					places = a.provides.places.is_some(),
					browser = a.provides.browser.is_some(),
					business = a.provides.business.is_some(),
					allow = sa_panel::meters::allowance(a),
					"Google account"
				);
			}
			serve(
				panel,
				identity,
				forward,
				telegram,
				grafana,
				settings.web()?,
				settings.browsers()?,
				settings.model(),
				settings.sandbox()?,
				readers,
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
	telegram: Option<settings::TelegramSettings>,
	grafana: Option<settings::GrafanaSettings>,
	files: Option<Files>,
	browsers: Option<sa_server::browser::Connected>,
	model: sa_server::claude::Model,
	sandbox: Option<sa_server::execute::Sandbox>,
	readers: Vec<sa_panel::meters::google::Google>,
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
	let hub_panel = panel.clone();
	let hub_concierge = concierge.clone();
	let engine = panel.clone();
	let metered = panel.clone();
	let purger = panel.clone();
	let grafana_root = format!("{}/grafana/", config.panel_origin.trim_end_matches('/'));
	let sign_in = SignIn::new(panel, concierge, config);
	let forward = match &services {
		Some(services) => Some(Forward::new(sign_in.clone(), services.clone())),
		None => {
			tracing::warn!("forward not configured: no ingest, no /api/sites, /api/review_archive or /api/playbook, no live updates from sites");
			None
		}
	};
	let bot = match (telegram, &forward) {
		(Some(tg), Some(forward)) => {
			let hub = Hub {
				panel: hub_panel,
				bot: BotApi::try_new(telegram::API_BASE, zeroize::Zeroizing::new(tg.token))?,
				concierge: hub_concierge,
				feeds: forward.clone(),
				locale: tg.locale,
				cabinet: cabinet.parse().wrap_err("the concierge origin is not a URL")?,
			};
			tracing::info!("alerts on, by Telegram");
			Some(Telegram {
				hub,
				name: BotName::new(tg.username),
			})
		}
		(Some(_), None) => unreachable!("refused before serve"),
		(None, _) => {
			tracing::warn!("TELEGRAM_BOT_TOKEN unset: alerts off");
			None
		}
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
	if sandbox.is_none() {
		tracing::warn!("PANEL_SANDBOX_URL unset: no sandbox, the agents have no execute");
	}
	if model.owner().is_none() {
		tracing::warn!("CLAUDE_CODE_OAUTH_TOKEN unset: the assistant answers only on a person's own Claude");
	}
	let catalog = forward.as_ref().map(|f| f.catalog().clone());
	let browsers_followed = browsers.clone();
	let app = http::app(
		sign_in,
		forward,
		grafana.as_ref().map(|(g, _)| g),
		http::Limits::default(),
		bot.clone(),
		browsers,
		Some(model),
		sandbox,
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
		let bot = async {
			if let Some(t) = bot {
				alerts::run(t, stopped.clone()).await;
			}
		};
		let feed = async {
			if let Some(services) = &services {
				live::follow(services.clone(), bus.clone(), stopped.clone()).await;
			}
		};
		let usage = sa_server::usage::run(engine.clone(), services.clone(), browsers_followed, stopped.clone());
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
		let meters = sa_server::meters::run(metered, readers, stopped.clone());
		tokio::join!(bot, feed, usage, tokens, grafana, purge, meters, catalog);
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
