use std::{
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use actix::prelude::*;
use log::{debug, error};
use trust_dns_resolver::{AsyncResolver, system_conf::read_system_conf};

use crate::{
    config::Config,
    discovery::Discovery,
};

/// An actor used for DNS based peer discovery.
///
/// This discovery system will check for new peers on a regular interval, emitting discovered
/// peers on a regular interval.
pub struct DnsDiscovery {
    config: Arc<Config>,
    discovery_dns_name: String,
    resolver: AsyncResolver,
    discovery: Addr<Discovery>,
}

impl DnsDiscovery {
    /// Create a new DNS peer discovery backend instance.
    pub fn new(discovery: Addr<Discovery>, discovery_dns_name: String, config: Arc<Config>) -> Self {
        // Build the async resolver.
        let (resolvercfg, resolveropts) = read_system_conf()
            .unwrap_or_else(|err| panic!("Failed to read system DNS config. On *nix systems, ensure your resolv.conf is present and properly formed. {}", err));
        let (resolver, f) = AsyncResolver::new(resolvercfg, resolveropts);
        Arbiter::spawn(f);

        Self{config, discovery_dns_name, resolver, discovery}
    }

    fn poll_dns(&mut self, ctx: &mut Context<Self>) {
        ctx.spawn(
            fut::wrap_future(self.resolver.lookup_ip(self.discovery_dns_name.as_str()))
                .map(|lookup_res, act: &mut Self, _| {
                    act.discovery.do_send(DnsAddrs(
                        lookup_res.iter().map(|rcd| SocketAddr::from((rcd, act.config.port))).collect()
                    ));
                })
                .map_err(|err, _, _| error!("Error from DNS peer discovery routine: {:?}", err))
                .timeout(Duration::from_secs(10), ()));
    }
}

impl Actor for DnsDiscovery {
    type Context = Context<Self>;

    /// Initialize this actor's internal behaviors on startup.
    ///
    /// This will begin the process of submitting DNS A/AAAA queries to discover cluster peers on the
    /// configured DNS name.
    fn started(&mut self, ctx: &mut Context<Self>) {
        debug!("Booting the DNS discovery backend.");
        self.poll_dns(ctx);
        ctx.run_interval(Duration::from_secs(10), |act, ctx| act.poll_dns(ctx));
    }
}

/// A type representing a payload of IP addresses from a DNS discovery probe.
pub struct DnsAddrs(pub Vec<SocketAddr>);

impl Message for DnsAddrs {
    type Result = ();
}
