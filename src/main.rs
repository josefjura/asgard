use std::process::ExitCode;

use asgard::cli::{self, Command};
use asgard::config::Config;
use asgard::db;
use asgard::domain::auth::{Auth, LINK_LIFETIME_HOURS};
use asgard::domain::clock::Clock;
use asgard::domain::members::Members;
use asgard::web::{self, AppState};
use sqlx::SqlitePool;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let command = match Command::parse(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("asgard: configuration error: {err}");
            return ExitCode::FAILURE;
        }
    };

    let result = match command {
        Command::Serve => run(config).await,
        Command::BootstrapAdmin { display_name } => bootstrap_admin(config, &display_name).await,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("asgard: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn open_database(config: &Config) -> Result<SqlitePool, Box<dyn std::error::Error>> {
    let pool = db::connect(config.database.clone())
        .await
        .map_err(|err| format!("cannot open the database at DATABASE_URL: {err}"))?;
    db::migrate(&pool)
        .await
        .map_err(|err| format!("cannot migrate the database at DATABASE_URL: {err}"))?;
    Ok(pool)
}

async fn bootstrap_admin(
    config: Config,
    display_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let pool = open_database(&config).await?;
    let members = Members::new(pool.clone());
    let auth = Auth::new(pool.clone(), Clock::system(), members, &config.public_url)?;
    let link = cli::bootstrap_admin(&auth, &config.public_url, display_name).await;
    pool.close().await;

    let link = link?;
    eprintln!("Open this link within {LINK_LIFETIME_HOURS} hours to join the Crew as its Admin:");
    println!("{link}");
    Ok(())
}

async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let pool = open_database(&config).await?;

    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|err| format!("cannot listen on {}: {err}", config.listen))?;
    tracing::info!(address = %config.listen, public_url = %config.public_url, "listening");

    let state = AppState::new(
        pool.clone(),
        Clock::system(),
        &config.public_url,
        config.vapid_private_key.public_key(),
    )?;
    axum::serve(listener, web::router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    pool.close().await;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}
