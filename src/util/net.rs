use std::net::IpAddr;
use systemstat::{Platform, System};

#[derive(Debug)]
pub enum NetErr {
    NoNetworkInterface,
}

pub fn select_host_address() -> Result<IpAddr, NetErr> {
    let system = System::new();
    let networks = system.networks().unwrap();

    for net in networks.values() {
        for n in &net.addrs {
            if let systemstat::IpAddr::V4(v) = n.addr {
                if !v.is_loopback() && !v.is_link_local() && !v.is_broadcast() {
                    return Ok(IpAddr::V4(v));
                }
            }
        }
    }

    Err(NetErr::NoNetworkInterface)
}
