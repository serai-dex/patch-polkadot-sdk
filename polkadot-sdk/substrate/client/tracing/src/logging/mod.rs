// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later WITH Classpath-exception-2.0

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

//! Substrate logging library.
//!
//! This crate uses tokio's [tracing](https://github.com/tokio-rs/tracing/) library for logging.

#![warn(missing_docs)]

mod directives;
mod event_format;
mod fast_local_time;
mod layers;
mod stderr_writer;

pub(crate) type DefaultLogger = stderr_writer::MakeStderrWriter;

pub use directives::*;
pub use sc_tracing_proc_macro::*;

use std::io::IsTerminal;
use std::io;
use tracing::Subscriber;
use tracing_subscriber::{
	filter::LevelFilter,
	fmt::{
		format, FormatEvent, FormatFields, Formatter, Layer as FmtLayer, MakeWriter,
		SubscriberBuilder,
	},
	layer::{self, SubscriberExt},
	registry::LookupSpan,
	EnvFilter, FmtSubscriber, Layer, Registry,
};

pub use event_format::*;
pub use fast_local_time::FastLocalTime;
pub use layers::*;

use stderr_writer::MakeStderrWriter;

/// Logging Result typedef.
pub type Result<T> = std::result::Result<T, Error>;

/// Logging errors.
#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
#[non_exhaustive]
#[error(transparent)]
pub enum Error {
	IoError(#[from] io::Error),
	SetGlobalDefaultError(#[from] tracing::subscriber::SetGlobalDefaultError),
	DirectiveParseError(#[from] tracing_subscriber::filter::ParseError),
	SetLoggerError(#[from] tracing_log::log_tracer::SetLoggerError),
}

macro_rules! enable_log_reloading {
	($builder:expr) => {{
		let builder = $builder.with_filter_reloading();
		let handle = builder.reload_handle();
		set_reload_handle(handle);
		builder
	}};
}

/// Convert a `Option<LevelFilter>` to a [`log::LevelFilter`].
///
/// `None` is interpreted as `Info`.
fn to_log_level_filter(level_filter: Option<LevelFilter>) -> log::LevelFilter {
	match level_filter {
		Some(LevelFilter::INFO) | None => log::LevelFilter::Info,
		Some(LevelFilter::TRACE) => log::LevelFilter::Trace,
		Some(LevelFilter::WARN) => log::LevelFilter::Warn,
		Some(LevelFilter::ERROR) => log::LevelFilter::Error,
		Some(LevelFilter::DEBUG) => log::LevelFilter::Debug,
		Some(LevelFilter::OFF) => log::LevelFilter::Off,
	}
}

/// Common implementation to get the subscriber.
fn prepare_subscriber<N, E, F, W>(
	directives: &str,
	profiling_targets: Option<&str>,
	force_colors: Option<bool>,
	detailed_output: bool,
	builder_hook: impl Fn(
		SubscriberBuilder<format::DefaultFields, EventFormat, EnvFilter, DefaultLogger>,
	) -> SubscriberBuilder<N, E, F, W>,
) -> Result<impl Subscriber + for<'a> LookupSpan<'a>>
where
	N: for<'writer> FormatFields<'writer> + 'static,
	E: FormatEvent<Registry, N> + 'static,
	W: for<'writer> MakeWriter<'writer> + 'static,
	F: layer::Layer<Formatter<N, E, W>> + Send + Sync + 'static,
	FmtLayer<Registry, N, E, W>: layer::Layer<Registry> + Send + Sync + 'static,
{
	// Accept all valid directives and print invalid ones
	fn parse_user_directives(mut env_filter: EnvFilter, dirs: &str) -> Result<EnvFilter> {
		for dir in dirs.split(',') {
			env_filter = env_filter.add_directive(parse_default_directive(dir)?);
		}
		Ok(env_filter)
	}

	// Initialize filter - ensure to use `parse_default_directive` for any defaults to persist
	// after log filter reloading by RPC
	let mut env_filter = EnvFilter::default()
		// Enable info
		.add_directive(parse_default_directive("info").expect("provided directive is valid"))
		// Disable info logging by default for some modules.
		.add_directive(parse_default_directive("ws=off").expect("provided directive is valid"))
		.add_directive(parse_default_directive("yamux=off").expect("provided directive is valid"))
		.add_directive(
			parse_default_directive("regalloc=off").expect("provided directive is valid"),
		)
		.add_directive(
			parse_default_directive("cranelift_codegen=off").expect("provided directive is valid"),
		)
		// Set warn logging by default for some modules.
		.add_directive(
			parse_default_directive("cranelift_wasm=warn").expect("provided directive is valid"),
		)
		.add_directive(parse_default_directive("hyper=warn").expect("provided directive is valid"))
		.add_directive(
			parse_default_directive("trust_dns_proto=off").expect("provided directive is valid"),
		)
		.add_directive(
			parse_default_directive("hickory_proto=off").expect("provided directive is valid"),
		)
		.add_directive(
			parse_default_directive("libp2p_mdns::behaviour::iface=off")
				.expect("provided directive is valid"),
		)
		// Disable annoying log messages from rustls
		.add_directive(
			parse_default_directive("rustls::common_state=off")
				.expect("provided directive is valid"),
		)
		.add_directive(
			parse_default_directive("rustls::conn=off").expect("provided directive is valid"),
		);

	if let Ok(lvl) = std::env::var("RUST_LOG") {
		if lvl != "" {
			env_filter = parse_user_directives(env_filter, &lvl)?;
		}
	}

	if directives != "" {
		env_filter = parse_user_directives(env_filter, directives)?;
	}

	if let Some(profiling_targets) = profiling_targets {
		env_filter = parse_user_directives(env_filter, profiling_targets)?;
		env_filter = env_filter.add_directive(
			parse_default_directive("sc_tracing=trace").expect("provided directive is valid"),
		);
	}

	let max_level_hint = Layer::<FmtSubscriber>::max_level_hint(&env_filter);
	let max_level = to_log_level_filter(max_level_hint);

	tracing_log::LogTracer::builder()
		.with_max_level(max_level)
		.init()?;

	// If we're only logging `INFO` entries then we'll use a simplified logging format.
	let detailed_output = match max_level_hint {
		Some(level) if level <= tracing_subscriber::filter::LevelFilter::INFO => false,
		_ => true,
	} || detailed_output;

	let enable_color = force_colors.unwrap_or_else(|| io::stderr().is_terminal());
	let timer = fast_local_time::FastLocalTime { with_fractional: detailed_output };

	// We need to set both together, because we are may printing to `stdout` and `stderr`.
	console::set_colors_enabled(enable_color);
	console::set_colors_enabled_stderr(enable_color);

	let event_format = EventFormat {
		timer,
		display_target: detailed_output,
		display_level: detailed_output,
		display_thread_name: detailed_output,
		dup_to_stdout: !io::stderr().is_terminal() && io::stdout().is_terminal(),
	};
	let builder = FmtSubscriber::builder().with_env_filter(env_filter);

	let builder = builder.with_span_events(format::FmtSpan::NONE);

	let builder = builder.with_writer(MakeStderrWriter::default());

	let builder = builder.event_format(event_format);

	let builder = builder_hook(builder);

	let subscriber = builder.finish().with(PrefixLayer);

	Ok(subscriber)
}

/// A builder that is used to initialize the global logger.
pub struct LoggerBuilder {
	directives: String,
	profiling: Option<(crate::TracingReceiver, String)>,
	custom_profiler: Option<Box<dyn crate::TraceHandler>>,
	log_reloading: bool,
	force_colors: Option<bool>,
	detailed_output: bool,
}

impl LoggerBuilder {
	/// Create a new [`LoggerBuilder`] which can be used to initialize the global logger.
	pub fn new<S: Into<String>>(directives: S) -> Self {
		Self {
			directives: directives.into(),
			profiling: None,
			custom_profiler: None,
			log_reloading: false,
			force_colors: None,
			detailed_output: false,
		}
	}

	/// Set up the profiling.
	pub fn with_profiling<S: Into<String>>(
		&mut self,
		tracing_receiver: crate::TracingReceiver,
		profiling_targets: S,
	) -> &mut Self {
		self.profiling = Some((tracing_receiver, profiling_targets.into()));
		self
	}

	/// Add a custom profiler.
	pub fn with_custom_profiling(
		&mut self,
		custom_profiler: Box<dyn crate::TraceHandler>,
	) -> &mut Self {
		self.custom_profiler = Some(custom_profiler);
		self
	}

	/// Wether or not to disable log reloading.
	pub fn with_log_reloading(&mut self, enabled: bool) -> &mut Self {
		self.log_reloading = enabled;
		self
	}

	/// Whether detailed log output should be enabled.
	///
	/// This includes showing the log target, log level and thread name.
	///
	/// This will be automatically enabled when there is a log level enabled that is higher than
	/// `info`.
	pub fn with_detailed_output(&mut self, detailed: bool) -> &mut Self {
		self.detailed_output = detailed;
		self
	}

	/// Force enable/disable colors.
	pub fn with_colors(&mut self, enable: bool) -> &mut Self {
		self.force_colors = Some(enable);
		self
	}

	/// Initialize the global logger
	///
	/// This sets various global logging and tracing instances and thus may only be called once.
	pub fn init(self) -> Result<()> {
		if let Some((tracing_receiver, profiling_targets)) = self.profiling {
			if self.log_reloading {
				let subscriber = prepare_subscriber(
					&self.directives,
					Some(&profiling_targets),
					self.force_colors,
					self.detailed_output,
					|builder| enable_log_reloading!(builder),
				)?;
				let mut profiling =
					crate::ProfilingLayer::new(tracing_receiver, &profiling_targets);

				self.custom_profiler
					.into_iter()
					.for_each(|profiler| profiling.add_handler(profiler));

				tracing::subscriber::set_global_default(subscriber.with(profiling))?;

				Ok(())
			} else {
				let subscriber = prepare_subscriber(
					&self.directives,
					Some(&profiling_targets),
					self.force_colors,
					self.detailed_output,
					|builder| builder,
				)?;
				let mut profiling =
					crate::ProfilingLayer::new(tracing_receiver, &profiling_targets);

				self.custom_profiler
					.into_iter()
					.for_each(|profiler| profiling.add_handler(profiler));

				tracing::subscriber::set_global_default(subscriber.with(profiling))?;

				Ok(())
			}
		} else if self.log_reloading {
			let subscriber = prepare_subscriber(
				&self.directives,
				None,
				self.force_colors,
				self.detailed_output,
				|builder| enable_log_reloading!(builder),
			)?;

			tracing::subscriber::set_global_default(subscriber)?;

			Ok(())
		} else {
			let subscriber = prepare_subscriber(
				&self.directives,
				None,
				self.force_colors,
				self.detailed_output,
				|builder| builder,
			)?;

			tracing::subscriber::set_global_default(subscriber)?;

			Ok(())
		}
	}
}

#[cfg(test)]
mod tests {
}
