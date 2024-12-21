use clap::ArgMatches;
use time::macros::format_description;
use tracing_subscriber::{fmt::time::UtcTime, EnvFilter, FmtSubscriber};

/// Set up logger
pub fn setup_logger<'a>(args: &'a ArgMatches) {
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::new(args.get_one::<String>("loglevel").unwrap()))
        .with_timer(UtcTime::new(format_description!(
            "[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]"
        )))
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("failed to start logger");
}
