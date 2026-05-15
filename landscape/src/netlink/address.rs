use std::net::{IpAddr, Ipv4Addr};

use futures::stream::TryStreamExt;
use netlink_packet_route::address::{AddressAttribute, AddressMessage};
use netlink_packet_route::AddressFamily;
use rtnetlink::{Error as NetlinkError, Handle};
use serde::Serialize;

use super::handle::create_handle;

fn is_addr_exist_error(err: &NetlinkError) -> bool {
    matches!(err, NetlinkError::NetlinkError(msg) if msg.raw_code() == -libc::EEXIST)
}

async fn add_address_if_missing(
    handle: &Handle,
    link_index: u32,
    link_name: &str,
    ip: IpAddr,
    prefix_length: u8,
) -> Result<(), NetlinkError> {
    match handle.address().add(link_index, ip, prefix_length).execute().await {
        Ok(()) => Ok(()),
        Err(err) if is_addr_exist_error(&err) => {
            tracing::debug!(
                "address {ip}/{prefix_length} already exists on {link_name}, skip adding"
            );
            Ok(())
        }
        Err(err) => Err(err),
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct LandscapeSingleIpInfo {
    pub address: IpAddr,
    pub is_permanent: bool,
    pub prefix_len: u8,
    pub ifindex: u32,
}

impl LandscapeSingleIpInfo {
    fn new(msg: AddressMessage) -> Option<Self> {
        // In newer versions, flags structure changed - check for IFA_F_PERMANENT bit
        let is_permanent =
            msg.header.flags.contains(netlink_packet_route::address::AddressHeaderFlags::Permanent);
        let mut address = None;
        for each in msg.attributes.iter() {
            match each {
                netlink_packet_route::address::AddressAttribute::Address(ip_addr) => {
                    address = Some(ip_addr.clone())
                }
                _ => {}
            }
        }

        if let Some(address) = address {
            Some(LandscapeSingleIpInfo {
                ifindex: msg.header.index,
                address,
                is_permanent,
                prefix_len: msg.header.prefix_len,
            })
        } else {
            None
        }
    }
}

pub async fn addresses_by_iface_name(link: String) -> Vec<LandscapeSingleIpInfo> {
    let mut result = vec![];

    let handle = match create_handle() {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("err info: {e:?}");
            return result;
        }
    };

    let mut links = handle.link().get().match_name(link.clone()).execute();
    if let Some(link) = links.try_next().await.unwrap() {
        let mut addresses =
            handle.address().get().set_link_index_filter(link.header.index).execute();
        while let Some(msg) = addresses.try_next().await.unwrap() {
            if let Some(info) = LandscapeSingleIpInfo::new(msg) {
                result.push(info);
            }
        }
    } else {
        tracing::error!("link {link} not found");
    }

    result
}

pub async fn addresses_by_iface_id(iface_id: u32) -> Vec<LandscapeSingleIpInfo> {
    let mut result = vec![];

    let handle = match create_handle() {
        Ok(h) => h,
        Err(e) => {
            tracing::error!("err info: {e:?}");
            return result;
        }
    };

    let mut addresses = handle.address().get().set_link_index_filter(iface_id).execute();
    while let Some(msg) = addresses.try_next().await.unwrap() {
        if let Some(info) = LandscapeSingleIpInfo::new(msg) {
            result.push(info);
        }
    }

    result
}

pub async fn set_iface_ip(link_name: &str, ip: IpAddr, prefix_length: u8) -> bool {
    let handle = match create_handle() {
        Ok(h) => h,
        Err(_) => return false,
    };

    let mut links = handle.link().get().match_name(link_name.to_string()).execute();
    if let Some(link) = links.try_next().await.unwrap() {
        let mut addr_iter = handle.address().get().execute();

        let mut has_same_ip = false;
        'search_same_ip: while let Some(addr) = addr_iter.try_next().await.unwrap() {
            if addr.header.index == link.header.index && addr.header.prefix_len == prefix_length {
                for nla in addr.attributes.iter() {
                    if let AddressAttribute::Address(bytes) = nla {
                        has_same_ip = *bytes == ip;
                        if has_same_ip {
                            break 'search_same_ip;
                        }
                    }
                }
            }
        }

        if !has_same_ip {
            tracing::info!("without same ip, add it");
            add_address_if_missing(&handle, link.header.index, link_name, ip, prefix_length)
                .await
                .unwrap();
        }
        true
    } else {
        false
    }
}

pub async fn get_ppp_address(
    iface_name: &str,
) -> Option<(u32, Option<Ipv4Addr>, Option<Ipv4Addr>)> {
    let handle = create_handle().ok()?;
    let mut links = handle.link().get().match_name(iface_name.to_string()).execute();

    if let Ok(Some(link)) = links.try_next().await {
        let mut out_addr: Option<Ipv4Addr> = None;
        let mut peer_addr: Option<Ipv4Addr> = None;
        let mut addresses =
            handle.address().get().set_link_index_filter(link.header.index).execute();
        while let Ok(Some(msg)) = addresses.try_next().await {
            if matches!(msg.header.family, AddressFamily::Inet) {
                for attr in msg.attributes.iter() {
                    match attr {
                        netlink_packet_route::address::AddressAttribute::Local(addr) => {
                            if let IpAddr::V4(addr) = addr {
                                out_addr = Some(addr.clone());
                            }
                        }
                        netlink_packet_route::address::AddressAttribute::Address(addr) => {
                            if let IpAddr::V4(addr) = addr {
                                peer_addr = Some(addr.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        Some((link.header.index, out_addr, peer_addr))
    } else {
        None
    }
}

pub async fn add_address_with_handle(
    link_name: &str,
    ip: IpAddr,
    prefix_length: u8,
    handle: Handle,
) {
    let mut links = handle.link().get().match_name(link_name.to_string()).execute();
    if let Some(link) = links.try_next().await.unwrap() {
        let mut addr_iter =
            handle.address().get().set_link_index_filter(link.header.index).execute();
        // 与要添加的 ip 是否相同
        let mut need_create_ip = true;
        while let Some(addr) = addr_iter.try_next().await.unwrap() {
            let perfix_len_equal = addr.header.prefix_len == prefix_length;
            let mut ip_equal = false;

            for attr in addr.attributes.iter() {
                if let AddressAttribute::Address(addr) = attr {
                    if *addr == ip {
                        ip_equal = true;
                    }
                }
            }

            if ip_equal && perfix_len_equal {
                need_create_ip = false;
                break;
            }
        }

        if need_create_ip {
            // tracing::info!("need create ip: {need_create_ip:?}");
            add_address_if_missing(&handle, link.header.index, link_name, ip, prefix_length)
                .await
                .unwrap();
        }
    }
}
