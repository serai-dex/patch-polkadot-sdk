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

//! Transport that serves as a common ground for all connections.

use either::Either;
use libp2p::{
	core::{
		muxing::StreamMuxerBox,
		transport::{Boxed, OptionalTransport},
		upgrade,
	},
	identity, noise, tcp, PeerId, Transport,
};
use std::time::Duration;

/// Builds the transport that serves as a common ground for all connections.
///
/// If `memory_only` is true, then only communication within the same process are allowed. Only
/// addresses with the format `/memory/...` are allowed.
///
/// Returns a `BandwidthSinks` object that allows querying the average bandwidth produced by all
/// the connections spawned with this transport.
pub(crate) fn build_transport(
	keypair: identity::Keypair,
	memory_only: bool,
) -> (Boxed<(PeerId, StreamMuxerBox)>, libp2p::metrics::Registry) {
	// Build the base layer of the transport.
	let transport = if !memory_only {
		// Main transport: DNS(TCP)
		let tcp_config = tcp::Config::new().nodelay(true);
		let _tcp_trans = tcp::tokio::Transport::new(tcp_config.clone());
		// let dns_init = dns::tokio::Transport::system(tcp_trans);

		Either::Left(/* if let Ok(dns) = dns_init */ { /*
			// WS + WSS transport
			//
			// Main transport can't be used for `/wss` addresses because WSS transport needs
			// unresolved addresses (BUT WSS transport itself needs an instance of DNS transport to
			// resolve and dial addresses).
			let tcp_trans = tcp::tokio::Transport::new(tcp_config);
			let dns_for_wss = dns::tokio::Transport::system(tcp_trans)
				.expect("same system_conf & resolver to work");
			Either::Left(websocket::WsConfig::new(dns_for_wss).or_transport(dns))
		} else { */
			// In case DNS can't be constructed, fallback to TCP + WS (WSS won't work)
			let tcp_trans = tcp::tokio::Transport::new(tcp_config);
			/* let desktop_trans = websocket::WsConfig::new(tcp_trans)
				.or_transport(tcp::tokio::Transport::new(tcp_config));
			Either::Left(desktop_trans) */ tcp_trans
		})
	} else {
		Either::Right(OptionalTransport::some(libp2p::core::transport::MemoryTransport::default()))
	};

	let authentication_config = noise::Config::new(&keypair).expect("Can create noise config. qed");
	let multiplexing_config = libp2p::yamux::Config::default();

	let mut registry = libp2p::metrics::Registry::default();
	let transport = transport
		.upgrade(upgrade::Version::V1Lazy)
		.authenticate(authentication_config)
		.multiplex(multiplexing_config)
		.timeout(Duration::from_secs(20));
	let transport = libp2p::metrics::BandwidthTransport::new(transport, &mut registry);
	(transport.map(|(peer_id, conn), _| (peer_id, StreamMuxerBox::new(conn))).boxed(), registry)
}
