use std::process::ExitCode;

use asgard::config::Config;
use asgard::db;
use asgard::domain::clock::Clock;
use asgard::web::{self, AppState, PublicOrigin};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("asgard: configuration error: {err}");
            return ExitCode::FAILURE;
        }
    };

    match run(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("asgard: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let pool = db::connect(config.database)
        .await
        .map_err(|err| format!("cannot open the database at DATABASE_URL: {err}"))?;
    db::migrate(&pool)
        .await
        .map_err(|err| format!("cannot migrate the database at DATABASE_URL: {err}"))?;

    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|err| format!("cannot listen on {}: {err}", config.listen))?;
    tracing::info!(address = %config.listen, public_url = %config.public_url, "listening");

    let state = AppState {
        db: pool.clone(),
        clock: Clock::system(),
        public_origin: PublicOrigin::of(&config.public_url),
    };
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
